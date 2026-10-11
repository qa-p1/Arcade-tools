//! One blocking worker, OS file notifications, and deadlines only after changes.
use arcade_shortcuts::{build_index, conf, hyprland, labels, lua, BuildInputs, Index, ReadPolicy};
use arcade_tools_core::paths::Paths;
use notify::{
    event::{MetadataKind, ModifyKind},
    EventKind, RecommendedWatcher, RecursiveMode, Watcher,
};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{mpsc, Arc, Mutex},
    time::{Duration, Instant},
};

const DEBOUNCE: Duration = Duration::from_millis(300);
const RETRY_DELAY: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct Options {
    pub output: PathBuf,
    pub registry: PathBuf,
    pub catalog: PathBuf,
    pub docs: PathBuf,
    pub bundled_catalog: Option<PathBuf>,
    pub bundled_docs: Option<PathBuf>,
    pub labels: PathBuf,
    pub hyprland_dir: Option<PathBuf>,
    pub query_runtime: bool,
    /// Override the wall-clock backstop for an explicit Hyprland config tree.
    pub lua_duration_limit: Option<Duration>,
}

impl Options {
    pub fn discover(paths: &Paths, resources: Option<&Path>) -> std::io::Result<Self> {
        let output = arcade_shortcuts::default_index_path()?;
        let cache = output.parent().unwrap();
        Ok(Self {
            registry: paths.link.registry.clone(),
            catalog: cache.join("catalog/shortcuts"),
            docs: cache.join("docs"),
            bundled_catalog: resources.map(|p| p.join("catalog/shortcuts")),
            bundled_docs: resources.map(|p| p.join("docs")),
            labels: paths.state().join("shortcut-labels.json"),
            hyprland_dir: None,
            query_runtime: cfg!(target_os = "linux"),
            lua_duration_limit: None,
            output,
        })
    }

    fn roots(&self) -> Vec<PathBuf> {
        let mut roots = vec![
            self.registry.clone(),
            self.catalog.clone(),
            self.docs.clone(),
            self.labels.clone(),
        ];
        if cfg!(target_os = "linux") {
            let config = std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
            if let Some(hypr) = self
                .hyprland_dir
                .clone()
                .or_else(|| config.as_ref().map(|c| c.join("hypr")))
            {
                roots.push(hypr);
            }
            if self.hyprland_dir.is_none() {
                if let Some(config) = config {
                    roots.extend([config.join("caelestia"), config.join("hypremoji")]);
                }
            }
        }
        roots
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub generation: u64,
    pub path: PathBuf,
    pub built_at: String,
    pub apps: usize,
    pub wm_shortcuts: usize,
    pub warnings: Vec<String>,
    pub elapsed_ms: f64,
    pub error: Option<String>,
}

enum Event {
    Changed,
    Rebuild(mpsc::Sender<Result<Status, String>>),
    Sources { catalog: PathBuf, docs: PathBuf },
    Stop,
}

pub struct IndexService {
    tx: mpsc::Sender<Event>,
    status: Arc<Mutex<Status>>,
}

impl IndexService {
    pub fn start(
        options: Options,
        on_build: impl Fn(Status) + Send + 'static,
    ) -> std::io::Result<Arc<Self>> {
        let (tx, rx) = mpsc::channel();
        let status = Arc::new(Mutex::new(Status {
            path: options.output.clone(),
            ..Default::default()
        }));
        let service = Arc::new(Self {
            tx: tx.clone(),
            status: status.clone(),
        });
        std::thread::Builder::new()
            .name("tools-shortcut-index".into())
            .spawn(move || run(options, tx, rx, status, on_build))?;
        Ok(service)
    }

    pub fn changed(&self) {
        let _ = self.tx.send(Event::Changed);
    }

    /// For UI commands: call from spawn_blocking, never the UI thread.
    pub fn rebuild_index(&self) -> Result<Status, String> {
        let (tx, rx) = mpsc::channel();
        self.tx
            .send(Event::Rebuild(tx))
            .map_err(|_| "Shortcut worker stopped".to_owned())?;
        rx.recv()
            .map_err(|_| "Shortcut worker stopped".to_owned())?
    }

    pub fn set_sources(&self, catalog: PathBuf, docs: PathBuf) {
        let _ = self.tx.send(Event::Sources { catalog, docs });
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }
    pub fn stop(&self) {
        let _ = self.tx.send(Event::Stop);
    }
}
impl Drop for IndexService {
    fn drop(&mut self) {
        self.stop();
    }
}

fn changed(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(_)
    ) && !matches!(
        kind,
        EventKind::Modify(ModifyKind::Metadata(MetadataKind::AccessTime))
    )
}

fn existing_parent(path: &Path) -> Option<PathBuf> {
    path.ancestors().find(|p| p.is_dir()).map(Path::to_path_buf)
}

fn update_watches(
    watcher: &mut RecommendedWatcher,
    watched: &mut BTreeMap<PathBuf, bool>,
    roots: &[PathBuf],
) -> Vec<String> {
    let mut desired = BTreeMap::new();
    let mut warnings = Vec::new();
    for root in roots {
        if let Some(parent) = existing_parent(root) {
            let recursive = parent == *root && root.is_dir();
            desired
                .entry(parent)
                .and_modify(|r| *r |= recursive)
                .or_insert(recursive);
        }
    }
    for (old, mode) in watched.iter() {
        if desired.get(old) != Some(mode) {
            let _ = watcher.unwatch(old);
        }
    }
    watched.retain(|path, mode| desired.get(path) == Some(mode));
    for (new, recursive) in &desired {
        if watched.get(new) != Some(recursive) {
            // Never recursively watch a broad ancestor of an absent cache or
            // optional config directory. Upgrade after its creation instead.
            let mode = if *recursive {
                RecursiveMode::Recursive
            } else {
                RecursiveMode::NonRecursive
            };
            if let Err(error) = watcher.watch(new, mode) {
                warnings.push(format!("Could not watch {}: {error}", new.display()));
            } else {
                watched.insert(new.clone(), *recursive);
            }
        }
    }
    warnings
}

fn run(
    mut options: Options,
    tx: mpsc::Sender<Event>,
    rx: mpsc::Receiver<Event>,
    status: Arc<Mutex<Status>>,
    on_build: impl Fn(Status),
) {
    let interests = Arc::new(Mutex::new(options.roots()));
    let event_interests = interests.clone();
    let watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
        let Ok(event) = result else { return };
        if !changed(&event.kind) {
            return;
        }
        let roots = event_interests.lock().unwrap();
        if event
            .paths
            .iter()
            .any(|p| roots.iter().any(|r| p.starts_with(r) || r.starts_with(p)))
        {
            let _ = tx.send(Event::Changed);
        }
    });
    let watcher_error = watcher.as_ref().err().map(ToString::to_string);
    let mut watcher = watcher.ok();
    let mut watched = BTreeMap::new();
    if let Some(w) = &mut watcher {
        let _ = update_watches(w, &mut watched, &options.roots());
    }
    let mut deadline = Some(Instant::now());
    let mut retry = None;
    let mut retried = false;
    let mut waiters = Vec::new();
    loop {
        let next = deadline.into_iter().chain(retry).min();
        let event = match next {
            Some(at) => match rx.recv_timeout(at.saturating_duration_since(Instant::now())) {
                Ok(e) => Some(e),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            },
            None => match rx.recv() {
                Ok(e) => Some(e),
                Err(_) => break,
            },
        };
        match event {
            Some(Event::Stop) => break,
            Some(Event::Changed) => {
                deadline = Some(Instant::now() + DEBOUNCE);
                retry = None;
                retried = false;
            }
            Some(Event::Sources { catalog, docs }) => {
                options.catalog = catalog;
                options.docs = docs;
                deadline = Some(Instant::now() + DEBOUNCE);
                retry = None;
                retried = false;
            }
            Some(Event::Rebuild(waiter)) => {
                waiters.push(waiter);
                deadline = Some(Instant::now());
                retry = None;
                retried = false;
            }
            None => {
                deadline = None;
                retry = None;
                let start = Instant::now();
                let (mut index, files) = build(&options);
                let mut roots = options.roots();
                roots.extend(files);
                *interests.lock().unwrap() = roots.clone();
                if let Some(w) = &mut watcher {
                    index
                        .wm
                        .warnings
                        .extend(update_watches(w, &mut watched, &roots));
                } else {
                    index.wm.warnings.push(format!(
                        "Shortcut file watching is unavailable; use Rebuild to refresh: {}",
                        watcher_error.as_deref().unwrap_or("unknown error")
                    ));
                }
                let timed_out = index
                    .wm
                    .warnings
                    .iter()
                    .any(|w| w.contains("time limit exceeded"));
                let result = index
                    .write_index(&options.output)
                    .map_err(|e| e.to_string());
                let current = Status {
                    generation: status.lock().unwrap().generation + 1,
                    path: options.output.clone(),
                    built_at: index.built_at,
                    apps: index.apps.len(),
                    wm_shortcuts: index.wm.groups.iter().map(|g| g.shortcuts.len()).sum(),
                    warnings: index.wm.warnings,
                    elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
                    error: result.as_ref().err().cloned(),
                };
                *status.lock().unwrap() = current.clone();
                for waiter in waiters.drain(..) {
                    let _ = waiter.send(
                        result
                            .as_ref()
                            .map(|_| current.clone())
                            .map_err(Clone::clone),
                    );
                }
                on_build(current);
                // One retry after a time abort, with a blocking deadline;
                // successful builds and idle workers have no timer.
                if timed_out && !retried {
                    retried = true;
                    retry = Some(Instant::now() + RETRY_DELAY);
                }
            }
        }
    }
}

fn build(options: &Options) -> (Index, Vec<PathBuf>) {
    let report = if cfg!(target_os = "linux") {
        if let Some(dir) = &options.hyprland_dir {
            let (kind, path) = hyprland::select_config(dir);
            let config = match (kind, path) {
                (hyprland::ConfigKind::Lua, Some(path)) => {
                    let mut sandbox = lua::SandboxOptions::new(dir);
                    sandbox.read_policy = ReadPolicy::new([dir.clone()]);
                    if let Some(limit) = options.lua_duration_limit {
                        sandbox.duration_limit = limit;
                    }
                    lua::evaluate(&path, &sandbox)
                }
                (hyprland::ConfigKind::Conf, Some(path)) => conf::parse(
                    &path,
                    &conf::ConfOptions {
                        read_policy: ReadPolicy::new([dir.clone()]),
                        ..Default::default()
                    },
                ),
                _ => Default::default(),
            };
            Some(hyprland::merge(
                kind,
                config,
                None,
                &labels::UserLabels::new(),
            ))
        } else if options.query_runtime {
            Some(hyprland::read_machine(&labels::UserLabels::new()))
        } else {
            None
        }
    } else {
        None
    };
    let files = report.as_ref().map(|r| r.files.clone()).unwrap_or_default();
    let registry = arcade_link::registry::Registry::load(&arcade_link::Locations {
        registry: options.registry.clone(),
        ..arcade_link::Locations::under(&options.registry)
    });
    let docs = registry
        .apps()
        .iter()
        .filter_map(|app| {
            if !arcade_link::manifest::ids::APPS.contains(&app.id.as_str()) {
                return None;
            }
            let version = registry
                .additions(&app.id)
                .and_then(|a| a.docs.as_ref())
                .map(|d| d.version.as_str())
                .unwrap_or(&app.version);
            // IDs come from the restricted registry; versions still need to be a
            // single component before being joined to an owned docs cache.
            if Path::new(version).components().count() != 1
                || version == "."
                || version == ".."
                || version.contains(['/', '\\'])
            {
                return None;
            }
            let cached = options
                .docs
                .join(&app.id)
                .join(version)
                .join("shortcuts.json");
            if cached.is_file() {
                return Some(cached);
            }
            options
                .bundled_docs
                .as_ref()
                .map(|d| d.join(&app.id).join("shortcuts.json"))
                .filter(|p| p.is_file())
        })
        .collect();
    let catalog = if has_sheets(&options.catalog) {
        Some(options.catalog.clone())
    } else {
        options.bundled_catalog.clone().filter(|p| has_sheets(p))
    };
    (
        build_index(&BuildInputs {
            catalog_dir: catalog,
            arcade_docs: docs,
            registry_dir: Some(options.registry.clone()),
            hyprland: report,
            user_labels: options.labels.is_file().then(|| options.labels.clone()),
            ..Default::default()
        }),
        files,
    )
}

fn has_sheets(path: &Path) -> bool {
    std::fs::read_dir(path).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry.path().extension().is_some_and(|e| e == "json")
                && entry.file_name() != "index.json"
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(root: &Path) -> Options {
        Options {
            output: root.join("index.json"),
            registry: root.join("registry"),
            catalog: root.join("catalog"),
            docs: root.join("docs"),
            bundled_catalog: None,
            bundled_docs: None,
            labels: root.join("labels.json"),
            hyprland_dir: Some(root.join("hypr")),
            query_runtime: false,
            lua_duration_limit: None,
        }
    }

    #[test]
    fn empty_cache_uses_bundled_sheets_and_docs_follow_installed_version() {
        use arcade_link::manifest::{self, Docs, Manifest, ManifestDocument};
        let root = tempfile::tempdir().unwrap();
        let fixtures =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates/arcade-shortcuts/tests/fixtures");
        let mut opts = options(root.path());
        std::fs::create_dir_all(&opts.catalog).unwrap();
        opts.bundled_catalog = Some(fixtures.join("catalog"));
        opts.bundled_docs = Some(root.path().join("bundled-docs"));
        let bundled = opts
            .bundled_docs
            .as_ref()
            .unwrap()
            .join("arcade.find/shortcuts.json");
        std::fs::create_dir_all(bundled.parent().unwrap()).unwrap();
        std::fs::copy(fixtures.join("docs/find-shortcuts.json"), &bundled).unwrap();
        let mut document: ManifestDocument = Manifest::new(
            "arcade.find",
            "app-version",
            &std::env::current_exe().unwrap().to_string_lossy(),
        )
        .into();
        document.additions.docs = Some(Docs {
            version: "docs-version".into(),
        });
        let loc = arcade_link::Locations {
            registry: opts.registry.clone(),
            ..arcade_link::Locations::under(root.path())
        };
        manifest::write_document(&loc, &document).unwrap();
        let (index, _) = build(&opts);
        assert!(index.apps.iter().any(|app| app.id == "firefox"));
        assert!(index.apps.iter().any(|app| app.id == "arcade.find"));
        let cached = opts.docs.join("arcade.find/docs-version/shortcuts.json");
        std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
        let mut sheet: serde_json::Value =
            serde_json::from_slice(&std::fs::read(bundled).unwrap()).unwrap();
        // The manifest's name takes precedence, so use a shortcut's title to
        // prove the versioned cache replaced the bundled sheet.
        sheet["groups"][0]["shortcuts"][0]["title"] = "Versioned toggle".into();
        std::fs::write(cached, serde_json::to_vec(&sheet).unwrap()).unwrap();
        let (index, _) = build(&opts);
        let find = index
            .apps
            .iter()
            .find(|app| app.id == "arcade.find")
            .unwrap();
        assert_eq!(find.groups[0].shortcuts[0].title, "Versioned toggle");
    }

    #[test]
    fn absent_directories_watch_only_their_parent_then_upgrade() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("catalog/shortcuts");
        let mut watcher =
            notify::recommended_watcher(|_: notify::Result<notify::Event>| {}).unwrap();
        let mut watched = BTreeMap::new();
        assert!(
            update_watches(&mut watcher, &mut watched, std::slice::from_ref(&target)).is_empty()
        );
        assert_eq!(watched, BTreeMap::from([(root.path().to_owned(), false)]));
        std::fs::create_dir_all(&target).unwrap();
        assert!(
            update_watches(&mut watcher, &mut watched, std::slice::from_ref(&target)).is_empty()
        );
        assert_eq!(watched, BTreeMap::from([(target, true)]));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn time_abort_is_visible_and_retried_exactly_once() {
        let root = tempfile::tempdir().unwrap();
        let mut opts = options(root.path());
        std::fs::create_dir_all(root.path().join("hypr")).unwrap();
        std::fs::write(
            root.path().join("hypr/hyprland.lua"),
            "hl.bind('SUPER + A', hl.dsp.no_op()); for i=1,1000000 do local x=i*i end",
        )
        .unwrap();
        opts.lua_duration_limit = Some(Duration::ZERO);
        let (tx, rx) = mpsc::channel();
        let service = IndexService::start(opts, move |status| {
            let _ = tx.send(status);
        })
        .unwrap();
        let first = rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(first.wm_shortcuts, 1);
        assert!(first
            .warnings
            .iter()
            .any(|w| w.contains("time limit exceeded") && w.contains("incomplete")));
        let index: Index =
            serde_json::from_slice(&std::fs::read(root.path().join("index.json")).unwrap())
                .unwrap();
        assert_eq!(index.wm.warnings, first.warnings);
        let retry = rx
            .recv_timeout(RETRY_DELAY + Duration::from_secs(3))
            .unwrap();
        assert_eq!(retry.generation, 2);
        assert!(rx
            .recv_timeout(RETRY_DELAY + Duration::from_millis(300))
            .is_err());
        service.stop();
    }

    #[test]
    fn changing_sources_updates_watches_and_uses_the_new_cache() {
        let root = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel();
        let service = IndexService::start(options(root.path()), move |status| {
            let _ = tx.send(status);
        })
        .unwrap();
        rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let catalog = root.path().join("new/catalog");
        let docs = root.path().join("new/docs");
        std::fs::create_dir_all(&catalog).unwrap();
        std::fs::create_dir_all(&docs).unwrap();
        service.set_sources(catalog.clone(), docs.clone());
        rx.recv_timeout(Duration::from_secs(3)).unwrap();
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../crates/arcade-shortcuts/tests/fixtures/catalog/firefox.json"),
            catalog.join("firefox.json"),
        )
        .unwrap();
        let built = rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(built.apps, 1);
        std::fs::write(docs.join("trigger.json"), "{}").unwrap();
        rx.recv_timeout(Duration::from_secs(3)).unwrap();
        service.stop();
    }

    #[test]
    fn startup_debounce_config_catalog_registry_and_explicit_rebuild() {
        let root = tempfile::tempdir().unwrap();
        let at = |p: &str| root.path().join(p);
        for dir in ["hypr", "registry", "catalog", "docs"] {
            std::fs::create_dir_all(at(dir)).unwrap();
        }
        std::fs::write(at("hypr/hyprland.conf"), "bind = SUPER, T, exec, foot\n").unwrap();
        let (tx, rx) = mpsc::channel();
        let service = IndexService::start(
            Options {
                output: at("index.json"),
                registry: at("registry"),
                catalog: at("catalog"),
                docs: at("docs"),
                bundled_catalog: None,
                bundled_docs: None,
                labels: at("labels.json"),
                hyprland_dir: Some(at("hypr")),
                query_runtime: false,
                lua_duration_limit: None,
            },
            move |s| {
                let _ = tx.send(s);
            },
        )
        .unwrap();
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(3)).unwrap().generation,
            1
        );
        for _ in 0..5 {
            service.changed();
        }
        assert!(rx.recv_timeout(Duration::from_millis(150)).is_err());
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(3)).unwrap().generation,
            2
        );
        assert!(rx.recv_timeout(Duration::from_millis(400)).is_err());
        for file in [
            "hypr/hyprland.conf",
            "registry/test.json",
            "catalog/test.json",
            "docs/test.json",
        ] {
            std::fs::write(at(file), "{}\n").unwrap();
            let next = rx.recv_timeout(Duration::from_secs(3)).unwrap();
            assert!(next.error.is_none());
        }
        let current = service.rebuild_index().unwrap();
        assert!(current.generation >= 7);
        let index: Index =
            serde_json::from_slice(&std::fs::read(at("index.json")).unwrap()).unwrap();
        assert_eq!(index.schema, 1);
        service.stop();
    }

    #[test]
    fn read_events_and_index_publication_do_not_trigger_rebuilds() {
        assert!(!changed(&EventKind::Access(notify::event::AccessKind::Any)));
        assert!(!changed(&EventKind::Modify(ModifyKind::Metadata(
            MetadataKind::AccessTime
        ))));
        assert!(changed(&EventKind::Modify(ModifyKind::Data(
            notify::event::DataChange::Content
        ))));
    }
}

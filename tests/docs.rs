mod support;
use arcade_tools_core::{
    catalog::{Catalog, SNAPSHOT},
    docs::*,
    paths::Paths,
    source::{Fetch, HttpSource, Source},
};
use serde_json::json;
use std::path::Path;

fn write(root: &Path, path: &str, body: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}
fn bundle(root: &Path) {
    write(root, "arcade.look/1.0.0/index.md", "---\ntitle: Preview files\norder: 1\nsince: 1.0.0\n---\n# Preview\nPreview image and video files quickly.");
    write(
        root,
        "arcade.look/2.0.0/index.md",
        "# New preview\nPreview media files.",
    );
    write(
        root,
        "arcade.look/2.0.0/shortcuts.md",
        "# Shortcuts\nCopy clipboard content securely.",
    );
    let manifest = SnapshotManifest {
        schema: 1,
        entries: ["1.0.0", "2.0.0"]
            .iter()
            .map(|version| SnapshotEntry {
                id: "arcade.look".into(),
                repo: "qa-p1/arcade-look".into(),
                version: (*version).into(),
                reference: format!("v{version}"),
                status: "snapshotted".into(),
                files: vec![],
            })
            .collect(),
    };
    write(
        root,
        "manifest.json",
        &serde_json::to_string(&manifest).unwrap(),
    );
}
#[test]
fn resolves_installed_exact_cache_and_versioned_bundled_fallback_without_network() {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::under(root.path());
    let bundled = root.path().join("bundle");
    bundle(&bundled);
    let resolver = DocsResolver::new(paths.clone(), bundled.clone());
    let exact = resolver
        .resolve("arcade.look", Some("1.0.0"), None)
        .unwrap()
        .unwrap();
    assert_eq!(exact.source, DocSource::Bundled);
    assert_eq!(exact.version, "1.0.0");
    assert!(!exact.needs_fetch);
    let fallback = resolver
        .resolve("arcade.look", Some("3.0.0"), None)
        .unwrap()
        .unwrap();
    assert_eq!(fallback.version, "2.0.0");
    assert!(fallback.needs_fetch);
    write(
        &paths.docs(),
        "arcade.look/3.0.0/index.md",
        "# Cached exact docs",
    );
    let cached = resolver
        .resolve("arcade.look", Some("3.0.0"), None)
        .unwrap()
        .unwrap();
    assert_eq!(cached.source, DocSource::Cached);
    let installed_root = root.path().join("installed-docs");
    write(&installed_root, "index.md", "# Installed docs");
    assert_eq!(
        resolver
            .resolve("arcade.look", Some("3.0.0"), Some(&installed_root))
            .unwrap()
            .unwrap()
            .source,
        DocSource::Installed
    );
    assert!(resolver
        .resolve("arcade.box", Some("1.0.0"), None)
        .unwrap()
        .is_none());
    assert!(resolver.resolve("../escape", None, None).is_err());
    assert!(resolver
        .resolve("arcade.look", Some("../../escape"), None)
        .is_err());
    write(&bundled, "manifest.json", r#"{"schema":2,"entries":[]}"#);
    assert!(resolver.resolve("arcade.look", None, None).is_err());
}
#[test]
fn search_is_prefix_based_and_intersects_terms_and_ignores_front_matter() {
    let root = tempfile::tempdir().unwrap();
    bundle(root.path());
    let locations = [
        DocLocation {
            id: "arcade.look".into(),
            version: "1.0.0".into(),
            root: root.path().join("arcade.look/1.0.0"),
            source: DocSource::Bundled,
            needs_fetch: false,
        },
        DocLocation {
            id: "arcade.look".into(),
            version: "2.0.0".into(),
            root: root.path().join("arcade.look/2.0.0"),
            source: DocSource::Bundled,
            needs_fetch: false,
        },
    ];
    let index = SearchIndex::build(&locations).unwrap();
    assert_eq!(index.search("PREV", 10).len(), 2);
    assert_eq!(index.search("preview vid", 10)[0].version, "1.0.0");
    assert_eq!(index.search("cop sec", 10)[0].path, "shortcuts.md");
    assert!(index.search("since", 10).is_empty());
    assert!(index.search("preview clipboard", 10).is_empty());
    assert!(index.search("", 10).is_empty());
    assert_eq!(index.search("preview", 1).len(), 1);
    assert_eq!(index.search("vid", 10)[0].title, "Preview files");
    // Search is in memory after construction.
    std::fs::remove_dir_all(root.path().join("arcade.look")).unwrap();
    assert_eq!(index.search("PREV", 10).len(), 2);
}

struct TestDocsSource {
    source: Source,
    url: String,
}
impl HttpSource for TestDocsSource {
    fn fetch(&self, url: &str, etag: Option<&str>, limit: u64) -> arcade_tools_core::Result<Fetch> {
        let route = url
            .strip_prefix("https://api.github.com/")
            .map(|path| format!("api/{path}"))
            .or_else(|| {
                url.strip_prefix("https://raw.githubusercontent.com/")
                    .map(|path| format!("raw/{path}"))
            })
            .expect("only trusted computed docs URLs");
        self.source
            .fetch(&format!("{}{route}", self.url), etag, limit)
    }
}
#[test]
fn explicit_docs_fetch_is_bounded_and_preserves_cache_after_invalid_response() {
    let server = support::FakeRelease::new();
    let paths = Paths::under(&server.root.path().join("docs"));
    let resolver = DocsResolver::new(paths.clone(), server.root.path().join("bundle"));
    let source = TestDocsSource {
        source: Source::test_server(&server.url).unwrap(),
        url: server.url.clone(),
    };
    let catalog = Catalog::parse(SNAPSHOT.as_bytes(), "0.2.0").unwrap();
    let app = catalog.app("arcade.look").unwrap();
    let route = "/api/repos/qa-p1/arcade-look/contents/docs/user?ref=v2.0.0";
    let text = b"# Preview\nOffline documentation";
    server.routes.lock().unwrap().insert(route.into(), serde_json::to_vec(&json!([{"type":"file","path":"docs/user/index.md","size":text.len()}, {"type":"dir","path":"docs/user/images"}])).unwrap());
    server.routes.lock().unwrap().insert(
        "/api/repos/qa-p1/arcade-look/contents/docs/user/images?ref=v2.0.0".into(),
        serde_json::to_vec(&json!([{"type":"file","path":"docs/user/images/test.png","size":3}]))
            .unwrap(),
    );
    server.routes.lock().unwrap().insert(
        "/raw/qa-p1/arcade-look/v2.0.0/docs/user/index.md".into(),
        text.to_vec(),
    );
    server.routes.lock().unwrap().insert(
        "/raw/qa-p1/arcade-look/v2.0.0/docs/user/images/test.png".into(),
        b"png".to_vec(),
    );
    let location = resolver.fetch_version(app, "2.0.0", &source).unwrap();
    assert_eq!(location.source, DocSource::Cached);
    assert_eq!(
        std::fs::read(location.root.join("images/test.png")).unwrap(),
        b"png"
    );
    assert_eq!(
        SearchIndex::build(std::slice::from_ref(&location))
            .unwrap()
            .search("off", 1)
            .len(),
        1
    );
    let previous = std::fs::read(location.root.join("index.md")).unwrap();
    server.routes.lock().unwrap().insert(
        route.into(),
        serde_json::to_vec(&json!([{"type":"file","path":"docs/user/../escape.md","size":1}]))
            .unwrap(),
    );
    assert!(resolver.fetch_version(app, "2.0.0", &source).is_err());
    assert_eq!(
        std::fs::read(location.root.join("index.md")).unwrap(),
        previous
    );
    server.routes.lock().unwrap().insert(
        route.into(),
        serde_json::to_vec(
            &json!([{"type":"file","path":"docs/user/index.md","size":MAX_SNAPSHOT+1}]),
        )
        .unwrap(),
    );
    assert_eq!(
        resolver
            .fetch_version(app, "2.0.0", &source)
            .unwrap_err()
            .code,
        "too_large"
    );
    server.routes.lock().unwrap().remove(route);
    assert!(resolver.fetch_version(app, "2.0.0", &source).is_err());
    assert_eq!(
        resolver
            .resolve("arcade.look", Some("2.0.0"), None)
            .unwrap()
            .unwrap()
            .source,
        DocSource::Cached
    );
}
#[test]
fn snapshot_script_checks_local_and_remote_fixtures_and_the_budget() {
    let out = std::process::Command::new("python3")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/test-docs-snapshot.py"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

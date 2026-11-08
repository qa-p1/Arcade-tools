#![allow(dead_code)]
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use tempfile::TempDir;

/// Panics unless the test runs in Link's isolated session
/// (`tools/e2e.py run -- cargo test`): HOME and ARCADE_HOME must sit under the
/// session's root, wherever the system's temporary folder is.
pub fn require_isolated_session() -> PathBuf {
    let root = PathBuf::from(
        std::env::var_os("ARCADE_E2E_ROOT")
            .expect("Run through Arcade-link/tools/e2e.py run -- cargo test"),
    );
    let inside = |name: &str| {
        std::env::var_os(name).is_some_and(|value| Path::new(&value).starts_with(&root))
    };
    assert!(
        inside("HOME") && inside("ARCADE_HOME"),
        "HOME and ARCADE_HOME must be inside the isolated session at {}",
        root.display()
    );
    root
}

/// Both spellings Tools may ask GitHub about: Link's compiled list and the
/// catalog's, which can differ in case (GitHub ignores case; this server doesn't).
pub fn repos(id: &str) -> Vec<String> {
    let compiled = compiled_repo(id);
    let catalog = catalog_repo(id);
    if catalog == compiled {
        vec![compiled]
    } else {
        vec![compiled, catalog]
    }
}
fn compiled_repo(id: &str) -> String {
    arcade_tools_core::apps::releases_url(id)
        .trim_start_matches("https://github.com/")
        .trim_end_matches("/releases")
        .to_owned()
}
/// The repo as the catalog spells it, which is what the manager requests.
pub fn catalog_repo(id: &str) -> String {
    let catalog: serde_json::Value =
        serde_json::from_str(arcade_tools_core::catalog::SNAPSHOT).unwrap();
    catalog["apps"]
        .as_array()
        .unwrap()
        .iter()
        .chain(std::iter::once(&catalog["tools"]))
        .find(|app| app["id"] == id)
        .and_then(|app| app["repo"].as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| compiled_repo(id))
}

/// The secret half of the update key debug builds of Tools trust for loopback sources (`src/trust.rs`).
pub const TEST_SIGNING_KEY: &str = "untrusted comment: Arcade Tools test-only update key (debug loopback sources)
RWQAAEIyAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAxxy+UWLj3N5hFGnqkvKJsyvUHsbiNvymSc0dQvPwRQ0qOQJlAH/VsXVyZ0cOljgX0qZSCxHAYATThhnxx/MOXLNgDan0zp+s9J/D2nvWbyFBVm9hCUSptoWCRiy9cLyMrLk91Q4LofI=
";

/// The tag a fixture release gets: the Nth release of `version`'s base on a channel.
fn fixture_tag(version: &str, channel: &str, earlier: usize) -> String {
    let base = version.split(['-', '+']).next().unwrap();
    match channel {
        "stable" => format!("v{version}"),
        // A fixture version that already names its nightly build keeps it: `0.2.0-nightly.42` → `.42.1`.
        "nightly" => match version.split_once('-').map(|(_, pre)| pre) {
            Some(pre) if pre.starts_with("nightly.") => {
                format!("v{base}-{}.{}", pre.split('+').next().unwrap(), earlier + 1)
            }
            _ => format!("v{base}-nightly.20261107.{}", earlier + 1),
        },
        _ => format!("v{base}-rc.{}", earlier + 1),
    }
}

/// Writes `manifest.json` for every file in `dir` and, outside the test channel, its signature.
pub fn sign_release(dir: &Path, id: &str, version: &str, tag: &str, channel: &str) {
    use arcade_link::release as link;
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            let name = p.file_name().unwrap();
            name != link::MANIFEST_NAME && name != link::SIGNATURE_NAME
        })
        .collect();
    files.sort();
    let channel: link::Channel = match channel {
        "stable" | "nightly" => channel.parse().unwrap(),
        _ => link::Channel::Test,
    };
    let manifest = link::ReleaseManifest::build(id, version, tag, channel, &files).unwrap();
    let _ = std::fs::remove_file(dir.join(link::SIGNATURE_NAME));
    if channel == link::Channel::Test {
        std::fs::write(dir.join(link::MANIFEST_NAME), manifest.to_json()).unwrap();
    } else {
        let key = link::SecretKey::parse(TEST_SIGNING_KEY).unwrap();
        let (json, signature) = link::sign_manifest(&manifest, &key);
        std::fs::write(dir.join(link::MANIFEST_NAME), json).unwrap();
        std::fs::write(dir.join(link::SIGNATURE_NAME), signature).unwrap();
    }
}

#[derive(Clone)]
pub struct HttpReply {
    pub status: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}
pub type RecordedRequests = Arc<Mutex<Vec<(String, Vec<String>)>>>;

pub struct FakeRelease {
    pub root: TempDir,
    pub url: String,
    pub routes: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    pub replies: Arc<Mutex<HashMap<String, HttpReply>>>,
    pub requests: RecordedRequests,
    /// Published releases, newest first: (repo, channel, version, GitHub release JSON).
    releases: Mutex<Vec<(String, String, String, serde_json::Value)>>,
    worker: Option<JoinHandle<()>>,
}
impl FakeRelease {
    pub fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let routes: Arc<Mutex<HashMap<String, Vec<u8>>>> = Default::default();
        let server_routes = routes.clone();
        let replies: Arc<Mutex<HashMap<String, HttpReply>>> = Default::default();
        let server_replies = replies.clone();
        let requests: RecordedRequests = Default::default();
        let server_requests = requests.clone();
        let worker = std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                    .unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let route = line.split_whitespace().nth(1).unwrap_or("").to_owned();
                if route == "/__stop" {
                    break;
                }
                let mut headers = Vec::new();
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).unwrap() == 0 || header == "\r\n" {
                        break;
                    }
                    headers.push(header.trim().to_string());
                }
                server_requests
                    .lock()
                    .unwrap()
                    .push((route.clone(), headers.clone()));
                let bytes = server_routes.lock().unwrap().get(&route).cloned();
                let override_reply = server_replies.lock().unwrap().get(&route).cloned();
                let reply = override_reply.unwrap_or_else(|| {
                    bytes.map_or(
                        HttpReply {
                            status: "404 Not Found".into(),
                            headers: vec![],
                            body: vec![],
                        },
                        |body| {
                            let etag = format!("\"{:x}\"", Sha256::digest(&body));
                            let unchanged = headers.iter().any(|h| {
                                h.split_once(':').is_some_and(|(k, v)| {
                                    k.eq_ignore_ascii_case("if-none-match") && v.trim() == etag
                                })
                            });
                            if unchanged {
                                return HttpReply {
                                    status: "304 Not Modified".into(),
                                    headers: vec![("ETag".into(), etag)],
                                    body: vec![],
                                };
                            }
                            // One `Range: bytes=a-b` or `bytes=a-`, as GitHub's release host serves.
                            let range = headers.iter().find_map(|h| {
                                let (k, v) = h.split_once(':')?;
                                let spec = k.eq_ignore_ascii_case("range").then_some(v.trim())?;
                                let (a, b) = spec.strip_prefix("bytes=")?.split_once('-')?;
                                let start: usize = a.parse().ok()?;
                                let end = if b.is_empty() {
                                    body.len()
                                } else {
                                    b.parse::<usize>().ok()? + 1
                                };
                                (start < end && end <= body.len()).then_some(start..end)
                            });
                            match range {
                                Some(r) => HttpReply {
                                    status: "206 Partial Content".into(),
                                    headers: vec![
                                        ("ETag".into(), etag),
                                        (
                                            "Content-Range".into(),
                                            format!(
                                                "bytes {}-{}/{}",
                                                r.start,
                                                r.end - 1,
                                                body.len()
                                            ),
                                        ),
                                    ],
                                    body: body[r].to_vec(),
                                },
                                None => HttpReply {
                                    status: "200 OK".into(),
                                    headers: vec![("ETag".into(), etag)],
                                    body,
                                },
                            }
                        },
                    )
                });
                let extra = reply
                    .headers
                    .iter()
                    .map(|(k, v)| format!("{k}: {v}\r\n"))
                    .collect::<String>();
                write!(
                    stream,
                    "HTTP/1.1 {}\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n",
                    reply.status,
                    reply.body.len()
                )
                .unwrap();
                let _ = stream.write_all(&reply.body);
            }
        });
        let server = Self {
            root,
            url,
            routes,
            replies,
            requests,
            releases: Mutex::new(Vec::new()),
            worker: Some(worker),
        };
        // Legacy lifecycle fixtures represent releases usable by Tools 0.1.0.
        // Production snapshot keeps its actual minTools 0.2.0 and is tested separately.
        let mut catalog: serde_json::Value =
            serde_json::from_str(arcade_tools_core::catalog::SNAPSHOT).unwrap();
        for app in catalog["apps"].as_array_mut().unwrap() {
            app["minTools"] = serde_json::json!("0.1.0");
        }
        catalog["tools"]["minTools"] = serde_json::json!("0.1.0");
        server.routes.lock().unwrap().insert(
            "/catalog.json".into(),
            serde_json::to_vec(&catalog).unwrap(),
        );
        arcade_tools_core::catalog::CatalogClient::with_url(
            arcade_tools_core::paths::Paths::under(&server.root.path().join("user")),
            arcade_tools_core::source::Source::test_server(&server.url).unwrap(),
            &format!("{}catalog.json", server.url),
        )
        .unwrap()
        .refresh_catalog();
        server
    }
    pub fn publish(
        &self,
        id: &str,
        version: &str,
        channel: &str,
        files: &[(&str, &[u8])],
        installer: &str,
    ) -> PathBuf {
        let dir = self
            .root
            .path()
            .join(format!("{}-{version}-{channel}", id.replace('.', "-")));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, bytes) in files {
            std::fs::write(dir.join(name), bytes).unwrap();
        }
        let generator = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/arcade-release.py");
        let out = std::process::Command::new("python3")
            .arg(generator)
            .args([
                "--id",
                id,
                "--version",
                version,
                "--channel",
                channel,
                "--notes",
                "https://github.com/qa-p1/test/releases",
                "--windows-installer",
                installer,
            ])
            .arg(&dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        println!("{}", String::from_utf8_lossy(&out.stdout).trim());
        let earlier = self
            .releases
            .lock()
            .unwrap()
            .iter()
            .filter(|(r, c, v, _)| *r == repos(id)[0] && c == channel && v != version)
            .count();
        let tag = fixture_tag(version, channel, earlier);
        sign_release(&dir, id, version, &tag, channel);
        let prefix = format!("assets/{}-{version}-{channel}/", id.replace('.', "-"));
        let mut assets = Vec::new();
        let mut routes = self.routes.lock().unwrap();
        for entry in std::fs::read_dir(&dir).unwrap() {
            let p = entry.unwrap().path();
            let name = p.file_name().unwrap().to_str().unwrap();
            let route = format!("{prefix}{name}");
            let bytes = std::fs::read(&p).unwrap();
            assets.push(serde_json::json!({"name": name, "browser_download_url": format!("{}{route}", self.url),
                "size": bytes.len(), "digest": format!("sha256:{:x}", Sha256::digest(&bytes))}));
            routes.insert(format!("/{route}"), bytes);
        }
        drop(routes);
        let mut releases = self.releases.lock().unwrap();
        for repo in repos(id) {
            let api = serde_json::json!({
                "tag_name": tag, "draft": false, "prerelease": channel != "stable", "assets": assets,
            });
            releases.retain(|(r, c, v, _)| !(*r == repo && c == channel && v == version));
            releases.insert(0, (repo, channel.to_owned(), version.to_owned(), api));
        }
        drop(releases);
        self.relist(id);
        // Link v0.3.0's generator adds installArgs for AppImages and DMGs. These
        // fakes are plain scripts, so they publish as pre-0.3.0 releases; the hub
        // lifecycle tests opt back in with a receipt-writing installer.
        self.amend_manifest(id, version, channel, |manifest| {
            for asset in manifest["assets"].as_array_mut().into_iter().flatten() {
                if let Some(asset) = asset.as_object_mut() {
                    asset.remove("installArgs");
                }
            }
        });
        dir
    }
    /// Modify the test release's manifest and API validator together.
    pub fn amend_manifest(
        &self,
        id: &str,
        version: &str,
        channel: &str,
        edit: impl FnOnce(&mut serde_json::Value),
    ) {
        let prefix = format!("/assets/{}-{version}-{channel}/", id.replace('.', "-"));
        let route = format!("{prefix}arcade-release.json");
        let mut routes = self.routes.lock().unwrap();
        let mut manifest: serde_json::Value = serde_json::from_slice(&routes[&route]).unwrap();
        edit(&mut manifest);
        let bytes = serde_json::to_vec(&manifest).unwrap();
        routes.insert(route, bytes.clone());
        drop(routes);
        for (_, c, v, api) in self.releases.lock().unwrap().iter_mut() {
            if c != channel || v != version {
                continue;
            }
            for asset in api["assets"].as_array_mut().unwrap() {
                if asset["name"] == "arcade-release.json" {
                    asset["size"] = serde_json::json!(bytes.len());
                    asset["digest"] =
                        serde_json::json!(format!("sha256:{:x}", Sha256::digest(&bytes)));
                }
            }
        }
        self.relist(id);
    }
}
impl FakeRelease {
    /// Serve GitHub's release listing for each spelling of the app's repo, newest first.
    fn relist(&self, id: &str) {
        let releases = self.releases.lock().unwrap();
        let mut routes = self.routes.lock().unwrap();
        for repo in repos(id) {
            let listing: Vec<&serde_json::Value> = releases
                .iter()
                .filter(|(r, _, _, _)| *r == repo)
                .map(|(_, _, _, api)| api)
                .collect();
            routes.insert(
                format!("/repos/{repo}/releases?per_page=30"),
                serde_json::to_vec(&listing).unwrap(),
            );
        }
    }
}
impl Drop for FakeRelease {
    fn drop(&mut self) {
        let host = self.url.trim_start_matches("http://").trim_end_matches('/');
        if let Ok(mut stream) = TcpStream::connect(host) {
            let _ = stream.write_all(b"GET /__stop HTTP/1.1\r\n\r\n");
        }
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}

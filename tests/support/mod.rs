#![allow(dead_code)]
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use tempfile::TempDir;

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
                            HttpReply {
                                status: if unchanged {
                                    "304 Not Modified"
                                } else {
                                    "200 OK"
                                }
                                .into(),
                                headers: vec![("ETag".into(), etag)],
                                body: if unchanged { vec![] } else { body },
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
        let repo = arcade_tools_core::apps::releases_url(id)
            .trim_start_matches("https://github.com/")
            .trim_end_matches("/releases");
        let endpoint = if channel == "stable" {
            "latest"
        } else {
            "tags/nightly"
        };
        routes.insert(format!("/repos/{repo}/releases/{endpoint}"), serde_json::to_vec(&serde_json::json!({
            "tag_name": if channel == "stable" { format!("v{version}") } else { "nightly".into() },
            "draft": false, "prerelease": channel == "nightly", "assets": assets,
        })).unwrap());
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
        let repo = arcade_tools_core::apps::releases_url(id)
            .trim_start_matches("https://github.com/")
            .trim_end_matches("/releases");
        let endpoint = format!(
            "/repos/{repo}/releases/{}",
            if channel == "stable" {
                "latest"
            } else {
                "tags/nightly"
            }
        );
        let mut api: serde_json::Value = serde_json::from_slice(&routes[&endpoint]).unwrap();
        for asset in api["assets"].as_array_mut().unwrap() {
            if asset["name"] == "arcade-release.json" {
                asset["size"] = serde_json::json!(bytes.len());
                asset["digest"] = serde_json::json!(format!("sha256:{:x}", Sha256::digest(&bytes)));
            }
        }
        routes.insert(endpoint, serde_json::to_vec(&api).unwrap());
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

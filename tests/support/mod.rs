#![allow(dead_code)]
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use tempfile::TempDir;

pub struct FakeRelease {
    pub root: TempDir,
    pub url: String,
    pub routes: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    worker: Option<JoinHandle<()>>,
}
impl FakeRelease {
    pub fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let routes: Arc<Mutex<HashMap<String, Vec<u8>>>> = Default::default();
        let server_routes = routes.clone();
        let worker = std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                    .unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let route = line.split_whitespace().nth(1).unwrap_or("");
                if route == "/__stop" {
                    break;
                }
                let bytes = server_routes.lock().unwrap().get(route).cloned();
                let (status, body) = bytes.map_or(("404 Not Found", Vec::new()), |b| ("200 OK", b));
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                let _ = stream.write_all(&body);
            }
        });
        Self {
            root,
            url,
            routes,
            worker: Some(worker),
        }
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
        let generator =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../Arcade-link/tools/arcade-release.py");
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
            routes.insert(format!("/{route}"), std::fs::read(&p).unwrap());
            assets.push(serde_json::json!({"name": name, "browser_download_url": format!("{}{route}", self.url)}));
        }
        let repo = arcade_link::manifest::releases_url(id)
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

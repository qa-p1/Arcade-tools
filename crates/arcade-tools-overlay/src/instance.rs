//! Per-user lock + authenticated local socket, adapted from Arcade Find's
//! `instance.rs` (MIT OR Apache-2.0). This instance exists only while open.
//! fs2 supplies the lock on Rust 1.88 (std File::try_lock needs newer Rust).

use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use fs2::FileExt;
use interprocess::local_socket::{prelude::*, ListenerOptions, Stream};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Command {
    Toggle,
    Show,
    Hide,
}

#[derive(Serialize, Deserialize)]
struct Envelope {
    token: String,
    command: Command,
}

#[derive(Serialize, Deserialize)]
struct Info {
    token: String,
    pid: u32,
}

pub struct Instance {
    _lock: std::fs::File,
    directory: PathBuf,
    token: String,
}

pub fn runtime_directory() -> io::Result<PathBuf> {
    if let Some(root) = std::env::var_os("ARCADE_HOME") {
        return Ok(PathBuf::from(root).join("run/tools-overlay"));
    }
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
        return Ok(PathBuf::from(runtime).join("arcade-tools-overlay"));
    }
    #[cfg(unix)]
    {
        // SAFETY: getuid takes no pointers and has no side effects.
        Ok(
            std::env::temp_dir().join(format!("arcade-tools-overlay-{}", unsafe {
                libc::getuid()
            })),
        )
    }
    #[cfg(not(unix))]
    {
        directories::BaseDirs::new()
            .map(|d| d.cache_dir().join("arcade/tools-overlay/run"))
            .ok_or_else(|| io::Error::other("cannot locate the runtime directory"))
    }
}

fn private_directory(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let metadata = std::fs::symlink_metadata(path)?;
        // SAFETY: getuid has no side effects.
        if !metadata.is_dir() || metadata.uid() != unsafe { libc::getuid() } {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "runtime directory is not owned by this user",
            ));
        }
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(unix)]
fn socket_path(directory: &Path) -> io::Result<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let path = directory.join("instance.sock");
    if path.as_os_str().as_bytes().len() < 100 {
        return Ok(path);
    }
    // Long test/profile paths use a short private runtime socket, as in Find.
    // SAFETY: getuid has no side effects.
    let short = std::env::temp_dir().join(format!("arcade-tools-overlay-{}", unsafe {
        libc::getuid()
    }));
    private_directory(&short)?;
    Ok(short.join(format!("{:016x}.sock", path_hash(directory))))
}

#[cfg(any(unix, windows))]
fn path_hash(path: &Path) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    hasher.finish()
}

fn socket_name(directory: &Path) -> io::Result<interprocess::local_socket::Name<'static>> {
    #[cfg(unix)]
    {
        socket_path(directory)?
            .to_fs_name::<interprocess::local_socket::GenericFilePath>()
            .map(|n| n.into_owned())
    }
    #[cfg(windows)]
    {
        format!("arcade-tools-overlay-{:016x}", path_hash(directory))
            .to_ns_name::<interprocess::local_socket::GenericNamespaced>()
            .map(|n| n.into_owned())
    }
}

impl Instance {
    pub fn acquire(directory: PathBuf) -> io::Result<Option<Self>> {
        private_directory(&directory)?;
        let mut options = std::fs::OpenOptions::new();
        options.create(true).truncate(false).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let file = options.open(directory.join("instance.lock"))?;
        match FileExt::try_lock_exclusive(&file) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(None),
            Err(error) => return Err(error),
        }
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).map_err(|e| io::Error::other(e.to_string()))?;
        let token: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        let info = serde_json::to_vec(&Info {
            token: token.clone(),
            pid: std::process::id(),
        })?;
        // The containing folder is private; replacing prevents stale symlinks.
        let path = directory.join("instance.json");
        let _ = std::fs::remove_file(&path);
        let mut info_options = std::fs::OpenOptions::new();
        info_options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            info_options.mode(0o600);
        }
        info_options.open(path)?.write_all(&info)?;
        Ok(Some(Self {
            _lock: file,
            directory,
            token,
        }))
    }

    pub fn serve(&self, handler: Arc<dyn Fn(Command) + Send + Sync>) -> io::Result<()> {
        #[cfg(unix)]
        {
            let _ = std::fs::remove_file(socket_path(&self.directory)?);
        }
        let listener = ListenerOptions::new()
            .name(socket_name(&self.directory)?)
            .create_sync()?;
        let token = self.token.clone();
        std::thread::Builder::new()
            .name("overlay-instance".into())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    let _ = stream.set_recv_timeout(Some(Duration::from_millis(100)));
                    let mut line = String::new();
                    if BufReader::new((&stream).take(4096))
                        .read_line(&mut line)
                        .is_err()
                    {
                        continue;
                    }
                    let ok = if let Ok(request) = serde_json::from_str::<Envelope>(&line) {
                        if constant_eq(&request.token, &token) {
                            handler(request.command);
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    };
                    let _ = (&stream).write_all(if ok { b"ok\n" } else { b"denied\n" });
                }
            })?;
        Ok(())
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.directory.join("instance.json"));
        #[cfg(unix)]
        if let Ok(path) = socket_path(&self.directory) {
            let _ = std::fs::remove_file(path);
        }
        // Keep the lock inode: unlinking it can permit simultaneous lock owners.
    }
}

fn constant_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |value, (a, b)| value | (a ^ b))
            == 0
}

/// Bounded retries cover the short lock-to-listener startup race.
pub fn send(directory: &Path, command: Command) -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_millis(250);
    loop {
        match send_once(directory, command) {
            Ok(()) => return Ok(()),
            Err(error) if Instant::now() >= deadline => return Err(error),
            Err(_) => std::thread::sleep(Duration::from_millis(5)),
        }
    }
}

fn send_once(directory: &Path, command: Command) -> io::Result<()> {
    let info: Info = serde_json::from_slice(&std::fs::read(directory.join("instance.json"))?)?;
    let stream = Stream::connect(socket_name(directory)?)?;
    stream.set_recv_timeout(Some(Duration::from_millis(50)))?;
    let request = serde_json::to_string(&Envelope {
        token: info.token,
        command,
    })?;
    writeln!(&stream, "{request}")?;
    let mut reply = String::new();
    BufReader::new((&stream).take(64)).read_line(&mut reply)?;
    if reply.trim() == "ok" {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "overlay command denied",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_instance_authenticated_commands_and_stale_restart() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("runtime");
        let instance = Instance::acquire(dir.clone()).unwrap().unwrap();
        assert!(Instance::acquire(dir.clone()).unwrap().is_none());
        let (tx, rx) = std::sync::mpsc::channel();
        instance
            .serve(Arc::new(move |command| {
                let _ = tx.send(command);
            }))
            .unwrap();
        send(&dir, Command::Toggle).unwrap();
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            Command::Toggle
        );
        std::fs::write(dir.join("instance.json"), r#"{"token":"wrong","pid":1}"#).unwrap();
        assert_eq!(
            send_once(&dir, Command::Hide).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert!(rx.try_recv().is_err());
        drop(instance);
        assert!(!dir.join("instance.json").exists());
        assert!(Instance::acquire(dir).unwrap().is_some());
    }
}

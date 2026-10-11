//! Per-user lock + authenticated local socket, adapted from Arcade Find's
//! `instance.rs` (MIT OR Apache-2.0). This instance exists only while open.
//! fs2 supplies the lock on Rust 1.88 (std File::try_lock needs newer Rust).

use std::io::{self, Read, Write};
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
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.raw_os_error() == fs2::lock_contended_error().raw_os_error() =>
            {
                return Ok(None)
            }
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
                    let Ok(line) = read_line(&stream, 4096, Duration::from_millis(100)) else {
                        continue;
                    };
                    let command = if let Ok(request) = serde_json::from_str::<Envelope>(&line) {
                        if constant_eq(&request.token, &token) {
                            Some(request.command)
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    if let Some(command) = command {
                        // A close event may end the whole process immediately.
                        // Wait until the launcher has read our acknowledgement
                        // before waking the UI, including on Windows where pipe
                        // buffers can disappear with the last server handle.
                        if (&stream).write_all(b"ok\n").is_ok()
                            && read_line(&stream, 16, Duration::from_millis(100))
                                .is_ok_and(|receipt| receipt == "received")
                        {
                            handler(command);
                        }
                    } else {
                        let _ = (&stream).write_all(b"denied\n");
                    }
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
    // Named-pipe connects can block, and interprocess does not implement
    // receive timeouts on Windows. Bound the entire forwarding operation.
    let directory = directory.to_owned();
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = tx.send(send_retry(&directory, command));
    });
    rx.recv_timeout(Duration::from_millis(300))
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "overlay instance did not answer"))?
}

fn send_retry(directory: &Path, command: Command) -> io::Result<()> {
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
    stream.set_nonblocking(cfg!(unix))?;
    let request = serde_json::to_string(&Envelope {
        token: info.token,
        command,
    })?;
    writeln!(&stream, "{request}")?;
    let reply = read_line(&stream, 64, Duration::from_millis(50))?;
    if reply.trim() == "ok" {
        writeln!(&stream, "received")?;
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "overlay command denied",
        ))
    }
}

/// Bounded reads on Unix sockets and Windows named pipes. Sleeps occur only
/// during a live request; the listener blocks in accept while idle.
fn read_line(stream: &Stream, limit: usize, timeout: Duration) -> io::Result<String> {
    stream.set_nonblocking(cfg!(unix))?;
    let deadline = Instant::now() + timeout;
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 256];
    while bytes.len() < limit {
        let length = buffer.len().min(limit - bytes.len());
        match read_available(stream, &mut buffer[..length]) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "closed overlay socket",
                ))
            }
            Ok(count) => {
                bytes.extend_from_slice(&buffer[..count]);
                if let Some(end) = bytes.iter().position(|byte| *byte == b'\n') {
                    return String::from_utf8(bytes[..end].to_vec()).map_err(io::Error::other);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "overlay socket read timed out",
                    ));
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "overlay request is too large",
    ))
}

#[cfg(unix)]
fn read_available(stream: &Stream, buffer: &mut [u8]) -> io::Result<usize> {
    (&*stream).read(buffer)
}

#[cfg(windows)]
fn read_available(stream: &Stream, buffer: &mut [u8]) -> io::Result<usize> {
    use std::os::windows::io::{AsHandle, AsRawHandle};
    use windows_sys::Win32::System::Pipes::PeekNamedPipe;

    let Stream::NamedPipe(pipe) = stream;
    let mut available = 0;
    // PIPE_NOWAIT reports ERROR_NO_DATA for an empty pipe, which interprocess
    // can translate to EOF. Peek without consuming bytes, then read only the
    // available data. Interprocess creates overlapped handles, and this is the
    // sole reader of this connection, so no pending read races with the peek.
    // https://learn.microsoft.com/en-us/windows/win32/api/namedpipeapi/nf-namedpipeapi-peeknamedpipe
    // SAFETY: the borrowed handle stays open and the output pointer is valid.
    let ok = unsafe {
        PeekNamedPipe(
            pipe.as_handle().as_raw_handle(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            &mut available,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    if available == 0 {
        return Err(io::ErrorKind::WouldBlock.into());
    }
    let length = buffer.len().min(available as usize);
    (&*stream).read(&mut buffer[..length])
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
        let stream = Stream::connect(socket_name(&dir).unwrap()).unwrap();
        let writer = interprocess::TryClone::try_clone(&stream).unwrap();
        let request = serde_json::to_string(&Envelope {
            token: instance.token.clone(),
            command: Command::Show,
        })
        .unwrap();
        let delayed = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(25));
            writeln!(&writer, "{request}").unwrap();
        });
        assert_eq!(
            read_line(&stream, 64, Duration::from_secs(1)).unwrap(),
            "ok"
        );
        assert!(
            rx.try_recv().is_err(),
            "UI command ran before acknowledgement receipt"
        );
        writeln!(&stream, "received").unwrap();
        delayed.join().unwrap();
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            Command::Show
        );
        drop(stream);
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

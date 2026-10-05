#![cfg(target_os = "linux")]
mod support;
use arcade_link::server::{Handler, InvokeContext, Job, Reply, Server, ServerConfig};
use arcade_link::wire::{InvokeRequest, InvokeResult};
use arcade_link::{Action, LinkError};
use arcade_tools_core::manager::{me, LaunchMode, Manager, Operation, Request};
use arcade_tools_core::paths::Paths;
use arcade_tools_core::release::Os;
use arcade_tools_core::source::Source;
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use support::FakeRelease;

struct Peer {
    server: Mutex<Option<Server>>,
    job: Mutex<Option<Job>>,
    mode: Option<bool>,
    quitting: AtomicBool,
    refuse: bool,
}
impl Handler for Peer {
    fn describe(&self) -> Vec<Action> {
        vec![Action::new("test.busy", "busy", "busy")]
    }
    fn invoke(&self, _: InvokeRequest, ctx: &InvokeContext) -> Result<Reply, LinkError> {
        let job = ctx.start_job();
        let ticket = job.ticket();
        *self.job.lock().unwrap() = Some(job);
        Ok(Reply::Job(ticket))
    }
    fn status(&self) -> serde_json::Value {
        self.mode.map_or(json!({}), |b| json!({"background": b}))
    }
    fn activate(&self) -> Result<(), LinkError> {
        Ok(())
    }
    fn quit(&self) -> Result<(), LinkError> {
        self.quitting.store(true, Ordering::SeqCst);
        if !self.refuse {
            self.server.lock().unwrap().take();
        }
        Ok(())
    }
}
fn fake_script(path: &std::path::Path, version: &str) -> Vec<u8> {
    format!(
        "#!/bin/sh\necho '{}:'\"$*\" >> '{}'\n",
        version,
        path.display()
    )
    .into_bytes()
}
fn installed(mode: Option<bool>, refuse: bool) -> (FakeRelease, Manager, Arc<Peer>) {
    assert!(
        std::env::var("HOME").unwrap().starts_with("/tmp/"),
        "Use the isolated runner"
    );
    let server = FakeRelease::new();
    let paths = Paths::under(&server.root.path().join("user"));
    let manager = Manager::new(paths.clone(), Source::test_server(&server.url).unwrap());
    server.publish(
        "arcade.look",
        "1.0.0",
        "stable",
        &[(
            "Look_x64.AppImage",
            &fake_script(&server.root.path().join("launches"), "old"),
        )],
        "nsis",
    );
    manager
        .operate(
            &Request {
                id: "arcade.look".into(),
                operation: Operation::Install,
                mode: Some(LaunchMode::Background),
                remove_data: false,
                app_closed: false,
            },
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
    let exe = paths.executable("arcade.look", Os::Linux, false);
    arcade_link::manifest::write_manifest(
        &paths.link,
        &arcade_link::Manifest::new("arcade.look", "1.0.0", exe.to_str().unwrap()),
    )
    .unwrap();
    let peer = Arc::new(Peer {
        server: Mutex::new(None),
        job: Mutex::new(None),
        mode,
        quitting: AtomicBool::new(false),
        refuse,
    });
    let resident = Server::start(
        ServerConfig {
            app: arcade_link::wire::PeerInfo {
                id: "arcade.look".into(),
                version: "1.0.0".into(),
            },
            locations: paths.link.clone(),
        },
        peer.clone(),
    )
    .unwrap();
    *peer.server.lock().unwrap() = Some(resident);
    server.publish(
        "arcade.look",
        "2.0.0",
        "stable",
        &[(
            "Look_x64.AppImage",
            &fake_script(&server.root.path().join("launches"), "new"),
        )],
        "nsis",
    );
    (server, manager, peer)
}
fn update() -> Request {
    Request {
        id: "arcade.look".into(),
        operation: Operation::Update,
        mode: None,
        remove_data: false,
        app_closed: false,
    }
}

#[test]
fn busy_is_retryable_and_update_restores_the_reported_background_mode() {
    let (server, manager, peer) = installed(Some(true), false);
    let mut client =
        arcade_link::client::Client::connect(&manager.paths.link, "arcade.look", &me()).unwrap();
    client
        .call(
            "invoke",
            serde_json::to_value(InvokeRequest::new("test.busy", "arcade.tools")).unwrap(),
        )
        .unwrap();
    assert_eq!(
        manager
            .operate(&update(), &AtomicBool::new(false), &mut |_| {})
            .unwrap_err()
            .code,
        "busy"
    );
    assert!(!peer.quitting.load(Ordering::SeqCst));
    assert_eq!(
        manager.record("arcade.look").unwrap().unwrap().version,
        "1.0.0"
    );
    peer.job
        .lock()
        .unwrap()
        .take()
        .unwrap()
        .finish(Ok(InvokeResult::default()));
    drop(client);
    let mut verified = false;
    manager
        .operate(&update(), &AtomicBool::new(false), &mut |p| {
            if p.phase == "Verified SHA-256" {
                assert!(!peer.quitting.load(Ordering::SeqCst));
                verified = true;
            }
            if p.phase == "Installing" {
                assert!(verified && peer.quitting.load(Ordering::SeqCst));
            }
        })
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while !std::fs::read_to_string(server.root.path().join("launches"))
        .is_ok_and(|s| s.contains("new:--background"))
    {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    println!("real Link busy → retry, verified download before app.quit, update resumes --background: PASS");
}
#[test]
fn unknown_mode_requires_a_choice_and_foreground_is_preserved() {
    let (server, manager, peer) = installed(None, false);
    assert_eq!(
        manager
            .operate(&update(), &AtomicBool::new(false), &mut |_| {})
            .unwrap_err()
            .code,
        "mode_required"
    );
    assert!(!peer.quitting.load(Ordering::SeqCst));
    let mut request = update();
    request.mode = Some(LaunchMode::Foreground);
    manager
        .operate(&request, &AtomicBool::new(false), &mut |_| {})
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while !std::fs::read_to_string(server.root.path().join("launches"))
        .is_ok_and(|s| s.lines().any(|s| s == "new:"))
    {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    println!("unknown app mode asks before quit; explicit foreground relaunch: PASS");
}
#[test]
fn quit_timeout_leaves_the_original_installation_intact() {
    let (_server, manager, peer) = installed(Some(true), true);
    let exe = manager.paths.executable("arcade.look", Os::Linux, false);
    let bytes = std::fs::read(&exe).unwrap();
    assert_eq!(
        manager
            .operate(&update(), &AtomicBool::new(false), &mut |_| {})
            .unwrap_err()
            .code,
        "timeout"
    );
    assert_eq!(std::fs::read(&exe).unwrap(), bytes);
    peer.server.lock().unwrap().take();
    println!("app.quit accepted but endpoint remains → bounded timeout, no replacement: PASS");
}

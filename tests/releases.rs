mod support;
use arcade_tools_core::release::{checksum, verify, Arch, Channel, Kind, Os, Release};
use arcade_tools_core::source::Source;
use std::sync::atomic::AtomicBool;
use support::FakeRelease;

#[test]
fn canonical_generator_and_local_github_release_download() {
    let server = FakeRelease::new();
    let dir = server.publish(
        "arcade.look",
        "0.4.0",
        "stable",
        &[
            ("Look_amd64.AppImage", b"verified appimage"),
            ("Look_arm64.AppImage", b"arm"),
            ("Look_x64-setup.exe", b"nsis"),
            ("Look_universal.dmg", b"dmg"),
            ("Look_x64.deb", b"deb"),
            ("Look_x64.rpm", b"rpm"),
            ("Look_arm64.msi", b"msi"),
        ],
        "nsis",
    );
    let release = Release::parse(&std::fs::read(dir.join("arcade-release.json")).unwrap()).unwrap();
    assert_eq!(
        release.select(Os::Linux, Arch::X64).unwrap().kind,
        Kind::Appimage
    );
    assert_eq!(
        release.select(Os::Linux, Arch::Arm64).unwrap().file,
        "Look_arm64.AppImage"
    );
    assert_eq!(
        release
            .select(Os::Windows, Arch::X64)
            .unwrap()
            .silent
            .unwrap(),
        ["/S"]
    );
    assert_eq!(
        release.select(Os::Macos, Arch::Arm64).unwrap().arch,
        Arch::Universal
    );
    assert!(release.select(Os::Windows, Arch::Arm64).is_err()); // MSI isn't a per-user contract.
    let source = Source::test_server(&server.url).unwrap();
    let checked = source
        .check("arcade.look", Channel::Stable, Os::Linux, Arch::X64)
        .unwrap();
    let downloaded = source
        .download(
            &checked,
            server.root.path(),
            &AtomicBool::new(false),
            &mut |_, _| {},
        )
        .unwrap();
    verify(downloaded.path(), &checked.asset).unwrap();
    assert_eq!(checksum(downloaded.path()).unwrap(), checked.asset.sha256);
    std::fs::write(downloaded.path(), b"tampered").unwrap();
    assert_eq!(
        verify(downloaded.path(), &checked.asset).unwrap_err().code,
        "checksum"
    );
    let cancelled = source
        .download(
            &checked,
            server.root.path(),
            &AtomicBool::new(true),
            &mut |_, _| {},
        )
        .unwrap_err();
    assert_eq!(cancelled.code, "cancelled");
    println!(
        "canonical manifest + 3-platform asset selection + good/bad checksum + cancellation: PASS"
    );
}

#[test]
fn schema_validation_and_install_policy_are_separate() {
    let server = FakeRelease::new();
    let dir = server.publish(
        "arcade.look",
        "1.0.0",
        "stable",
        &[("Look_x64.AppImage", b"app")],
        "nsis",
    );
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("arcade-release.json")).unwrap()).unwrap();
    for (key, invalid) in [
        ("schema", serde_json::json!(2)),
        ("id", serde_json::json!("other.look")),
        ("version", serde_json::json!("v1")),
        ("channel", serde_json::json!("beta")),
        ("assets", serde_json::json!([])),
        ("linkProtocol", serde_json::json!([0])),
    ] {
        let mut bad = value.clone();
        bad[key] = invalid;
        assert!(
            Release::parse(&serde_json::to_vec(&bad).unwrap()).is_err(),
            "{key}"
        );
    }
    for (key, invalid) in [
        ("sha256", serde_json::json!("A".repeat(64))),
        ("file", serde_json::json!("../bad")),
        ("os", serde_json::json!("android")),
        ("arch", serde_json::json!("x86")),
        ("kind", serde_json::json!("zip")),
        ("size", serde_json::Value::Null),
        ("silent", serde_json::Value::Null),
    ] {
        let mut bad = value.clone();
        bad["assets"][0][key] = invalid;
        assert!(
            Release::parse(&serde_json::to_vec(&bad).unwrap()).is_err(),
            "{key}"
        );
    }
    for key in [
        "schema",
        "id",
        "version",
        "channel",
        "linkProtocol",
        "notes",
        "assets",
    ] {
        let mut bad = value.clone();
        bad.as_object_mut().unwrap().remove(key);
        assert!(
            Release::parse(&serde_json::to_vec(&bad).unwrap()).is_err(),
            "missing {key}"
        );
    }
    let mut compatible = value.clone();
    compatible["extra"] = serde_json::json!(true);
    compatible["version"] = serde_json::json!("1-nightly-extra");
    compatible["assets"][0]["kind"] = serde_json::json!("deb");
    compatible["assets"][0]
        .as_object_mut()
        .unwrap()
        .remove("size");
    let parsed = Release::parse(&serde_json::to_vec(&compatible).unwrap()).unwrap();
    assert!(parsed.select(Os::Linux, Arch::X64).is_err());
    assert!(parsed.compatible("arcade.box", Channel::Stable).is_err());
    println!("canonical schema: invalid inputs rejected, optional/unknown fields accepted: PASS");
}

#[test]
fn nightly_and_clipboard_tarball_from_canonical_generator() {
    let server = FakeRelease::new();
    server.publish(
        "arcade.clipboard",
        "1.2.0-nightly",
        "nightly",
        &[
            ("clipboard-linux-x64.tar.gz", b"tar"),
            ("Clipboard_arm64-setup.exe", b"inno"),
        ],
        "inno",
    );
    let source = Source::test_server(&server.url).unwrap();
    let linux = source
        .check("arcade.clipboard", Channel::Nightly, Os::Linux, Arch::X64)
        .unwrap();
    assert_eq!(linux.asset.kind, Kind::Tarball);
    let windows = source
        .check(
            "arcade.clipboard",
            Channel::Nightly,
            Os::Windows,
            Arch::Arm64,
        )
        .unwrap();
    assert_eq!(
        windows.asset.silent.unwrap(),
        [
            "/VERYSILENT",
            "/SUPPRESSMSGBOXES",
            "/NORESTART",
            "/CURRENTUSER"
        ]
    );
    assert!(source
        .check("arcade.clipboard", Channel::Stable, Os::Linux, Arch::X64)
        .is_err());
    assert!(Source::test_server("https://untrusted.example/").is_err());
}

/// PLAN exit gate "a delta update from the previous release works through Tools, the signed manifest verifies, and
/// a tampered chunk is rejected", run on published files by the Gates workflow: `ARCADE_TOOLS_GATE_DELTA=OLD:NEW`
/// names the previous rc's AppImage and this rc's, both downloaded from their releases and checked against their
/// manifests. NEW is served by a local release host and signed with the test key (rc manifests are unsigned, and the
/// stable key is the owner's, B1); Tools' own source then checks the signature, builds the delta from OLD and
/// assembles NEW byte for byte. `ARCADE_TOOLS_GATE_APP` names the app the files are (default `arcade.tools`), so
/// every app's Gates run Tools' updater on its own published files. Without `ARCADE_TOOLS_GATE_DELTA` this test
/// does nothing.
#[test]
fn gate_published_files_delta_signature_and_tampered_chunk() {
    let Ok(spec) = std::env::var("ARCADE_TOOLS_GATE_DELTA") else {
        return;
    };
    let (old, new) = spec
        .split_once(':')
        .expect("ARCADE_TOOLS_GATE_DELTA=OLD:NEW");
    let app = std::env::var("ARCADE_TOOLS_GATE_APP").unwrap_or_else(|_| "arcade.tools".to_owned());
    delta_gate(&app, old, new);
}

/// The same gate for another app than Tools, on generated files: 8 MiB of pseudo-random bytes and the same with
/// 100 KiB changed in the middle, as an app's AppImage between two releases.
#[test]
fn delta_gate_runs_for_an_app_on_generated_files() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    let old: Vec<u8> = (0..8 << 20)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect();
    let mut new = old.clone();
    for byte in &mut new[3 << 20..(3 << 20) + 100 * 1024] {
        *byte ^= 0xa5;
    }
    let old_path = dir.path().join("old.AppImage");
    let new_path = dir.path().join("Arcade-Box-1.0.0-x86_64.AppImage");
    std::fs::write(&old_path, &old).unwrap();
    std::fs::write(&new_path, &new).unwrap();
    delta_gate(
        "arcade.box",
        old_path.to_str().unwrap(),
        new_path.to_str().unwrap(),
    );
}

/// The gate's other path: a new package that shares nothing with the old one (as Lens's single-binary AppImage
/// between builds), so Tools rightly downloads the whole file.
#[test]
fn delta_gate_takes_the_whole_file_when_a_delta_saves_too_little() {
    let dir = tempfile::tempdir().unwrap();
    let bytes = |mut state: u64| -> Vec<u8> {
        (0..4 << 20)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state as u8
            })
            .collect()
    };
    let old_path = dir.path().join("old.AppImage");
    let new_path = dir.path().join("ArcadeLens-1.0.0-linux-x86_64.AppImage");
    std::fs::write(&old_path, bytes(0x9e37_79b9_7f4a_7c15)).unwrap();
    std::fs::write(&new_path, bytes(0x2545_f491_4f6c_dd1d)).unwrap();
    delta_gate(
        "arcade.lens",
        old_path.to_str().unwrap(),
        new_path.to_str().unwrap(),
    );
}

fn delta_gate(app: &str, old: &str, new: &str) {
    use sha2::Digest;
    let assets = format!("/assets/{}-1.0.0-stable", app.replace('.', "-"));
    let new_bytes = std::fs::read(new).unwrap();
    let name = std::path::Path::new(new)
        .file_name()
        .unwrap()
        .to_str()
        .unwrap();
    let digest = format!("{:x}", sha2::Sha256::digest(&new_bytes));
    let server = FakeRelease::new();
    // Tools checks the app's repository the way its updater does: by the catalog's `repo`.
    let repo = support::catalog_repo(app);
    server.publish(app, "1.0.0", "stable", &[(name, &new_bytes)], "nsis");
    let route = format!("{assets}/{name}");
    let have = || {
        let mut index = arcade_link::release::ChunkIndex::default();
        index.add_file(std::path::Path::new(old)).unwrap();
        index
    };
    // Each request for the installer: its `Range: bytes=` value, or None for a plain whole-file request.
    let asked = |server: &FakeRelease| -> Vec<Option<String>> {
        server
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(r, _)| *r == route)
            .map(|(_, headers)| {
                headers.iter().find_map(|h| {
                    h.to_ascii_lowercase()
                        .strip_prefix("range: bytes=")
                        .map(str::to_owned)
                })
            })
            .collect()
    };
    let bounded = |r: &Option<String>| r.as_deref().is_some_and(|r| !r.ends_with('-'));

    // The signed manifest verifies, and the delta from OLD fetches only the changed chunks.
    let source = Source::test_server(&server.url).unwrap();
    let checked = source
        .check_repo(app, &repo, Channel::Stable, Os::Linux, Arch::X64)
        .unwrap();
    assert!(
        checked.entry.is_some(),
        "the verified manifest has the file's chunks"
    );
    // Tools takes a delta only when it saves enough (D5, `delta::plan`). An app whose whole package changes with
    // every build (Lens: its AppImage is one Rust binary, and a day-old nightly shares 6% of rc.3's bytes) gets the
    // whole file, which is the right download; the gate then checks that path instead of demanding a delta.
    if arcade_tools_core::delta::plan(&have(), checked.entry.as_ref().unwrap()).is_none() {
        whole_file_gate(
            &server,
            &source,
            &checked,
            &have(),
            &route,
            &new_bytes,
            &digest,
            &asked,
        );
        signature_gate(&server, &assets, app, &repo);
        return;
    }
    let cache = tempfile::tempdir().unwrap();
    let file = source
        .download_from(
            &checked,
            cache.path(),
            &have(),
            &AtomicBool::new(false),
            &mut |_, _| {},
        )
        .unwrap();
    assert_eq!(checksum(file.path()).unwrap(), digest);
    let ranges = asked(&server);
    assert!(
        !ranges.is_empty() && ranges.iter().all(bounded),
        "only bounded delta ranges: {ranges:?}"
    );
    let total: u64 = ranges
        .iter()
        .flatten()
        .filter_map(|r| r.split_once('-'))
        .map(|(a, b)| b.parse::<u64>().unwrap() - a.parse::<u64>().unwrap() + 1)
        .sum();
    assert!(
        total * 2 < new_bytes.len() as u64,
        "the delta fetched {total} of {} bytes",
        new_bytes.len()
    );
    println!(
        "delta: {total} of {} bytes in {} requests",
        new_bytes.len(),
        ranges.len()
    );

    // A tampered chunk is rejected: the assembled file fails its check and the intact whole file is used instead.
    let tampered: Vec<u8> = new_bytes.iter().map(|b| b ^ 0x5a).collect();
    server
        .tampered_ranges
        .lock()
        .unwrap()
        .insert(route.clone(), tampered);
    server.requests.lock().unwrap().clear();
    let cache = tempfile::tempdir().unwrap();
    let file = source
        .download_from(
            &checked,
            cache.path(),
            &have(),
            &AtomicBool::new(false),
            &mut |_, _| {},
        )
        .unwrap();
    assert_eq!(
        checksum(file.path()).unwrap(),
        digest,
        "the published file, never the tampered one"
    );
    let ranges = asked(&server);
    assert!(
        ranges.iter().any(bounded),
        "the delta was tried: {ranges:?}"
    );
    assert!(
        ranges.iter().any(|r| !bounded(r)),
        "then the whole file was fetched: {ranges:?}"
    );

    signature_gate(&server, &assets, app, &repo);
}

/// When a delta does not pay: Tools fetches the whole file in plain requests (no chunk ranges), it matches the signed
/// manifest, and a tampered whole file is refused rather than kept.
#[allow(clippy::too_many_arguments)]
fn whole_file_gate(
    server: &FakeRelease,
    source: &Source,
    checked: &arcade_tools_core::source::CheckedRelease,
    have: &arcade_link::release::ChunkIndex,
    route: &str,
    new_bytes: &[u8],
    digest: &str,
    asked: &dyn Fn(&FakeRelease) -> Vec<Option<String>>,
) {
    let cache = tempfile::tempdir().unwrap();
    let file = source
        .download_from(
            checked,
            cache.path(),
            have,
            &AtomicBool::new(false),
            &mut |_, _| {},
        )
        .unwrap();
    assert_eq!(checksum(file.path()).unwrap(), digest);
    let ranges = asked(server);
    assert!(
        !ranges.is_empty() && ranges.iter().all(Option::is_none),
        "the whole file in plain requests: {ranges:?}"
    );
    println!(
        "delta: not worth it (Tools' rule); the whole file, {} bytes, in {} request(s)",
        new_bytes.len(),
        ranges.len()
    );

    // A tampered whole file fails its check against the signed manifest and is not returned.
    let tampered: Vec<u8> = new_bytes.iter().map(|b| b ^ 0x5a).collect();
    let published = server
        .routes
        .lock()
        .unwrap()
        .insert(route.to_owned(), tampered)
        .unwrap();
    let cache = tempfile::tempdir().unwrap();
    assert!(
        source
            .download_from(
                checked,
                cache.path(),
                have,
                &AtomicBool::new(false),
                &mut |_, _| {},
            )
            .is_err(),
        "a tampered whole file is refused"
    );
    server
        .routes
        .lock()
        .unwrap()
        .insert(route.to_owned(), published);
}

/// A manifest whose signature does not match is refused before anything is downloaded.
fn signature_gate(server: &FakeRelease, assets: &str, app: &str, repo: &str) {
    let signature = format!("{assets}/manifest.json.sig");
    let mut routes = server.routes.lock().unwrap();
    let text = String::from_utf8(routes[&signature].clone()).unwrap();
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let mut sig = lines[1].clone().into_bytes();
    sig[10] = if sig[10] == b'A' { b'B' } else { b'A' };
    lines[1] = String::from_utf8(sig).unwrap();
    routes.insert(signature, (lines.join("\n") + "\n").into_bytes());
    drop(routes);
    assert!(Source::test_server(&server.url)
        .unwrap()
        .check_repo(app, repo, Channel::Stable, Os::Linux, Arch::X64)
        .is_err());
}

mod support;
use arcade_tools_core::{
    catalog::{Catalog, CatalogClient, CATALOG_URL, SNAPSHOT},
    paths::Paths,
    source::{HttpSource, Source},
};
use serde_json::json;

#[test]
fn discovery_env_probe() {
    let Some(root) = std::env::var_os("ARCADE_TOOLS_TEST_PATH_PROBE") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let paths = Paths::discover().unwrap();
    for path in [
        &paths.home,
        &paths.config,
        &paths.cache,
        &paths.data,
        &paths.local,
        &paths.roaming,
        &paths.link.registry,
    ] {
        assert!(
            path.starts_with(&root),
            "{} escaped isolated ARCADE_HOME",
            path.display()
        );
    }
    assert!(paths.isolated);
    let install = arcade_link::install::Environment::under(&root);
    assert_eq!(paths.home, install.home);
    assert_eq!(paths.config, install.config);
    assert_eq!(paths.data, install.data);
    assert_eq!(paths.local, install.local_app_data);
    assert_eq!(paths.link.registry, install.locations.registry);
}
#[test]
fn arcade_home_discovery_stays_in_the_childs_isolated_environment() {
    let root = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "discovery_env_probe", "--nocapture"])
        .env("ARCADE_HOME", root.path())
        .env("ARCADE_TOOLS_TEST_PATH_PROBE", root.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn snapshot_owner_filter_semver_and_unknown_fields() {
    let mut value: serde_json::Value = serde_json::from_str(SNAPSHOT).unwrap();
    let mut malicious = value["apps"][0].clone();
    malicious["id"] = json!("arcade.bad");
    malicious["repo"] = json!("evil/Arcade-box");
    value["apps"].as_array_mut().unwrap().push(malicious);
    value["extra"] = json!({"future": true});
    let parse = |value: &serde_json::Value, version| {
        Catalog::parse(&serde_json::to_vec(value).unwrap(), version)
    };
    let catalog = parse(&value, "0.1.0").unwrap();
    assert_eq!(catalog.apps.len(), 7);
    assert_eq!(catalog.warnings.len(), 1);
    assert!(catalog
        .apps
        .iter()
        .all(|a| a.install_reason.as_deref() == Some("Update Arcade Tools to install this")));
    assert!(parse(&value, "0.10.0")
        .unwrap()
        .apps
        .iter()
        .all(|a| a.install_reason.is_none()));
    assert!(parse(&value, "0.2.0-beta.1")
        .unwrap()
        .apps
        .iter()
        .all(|a| a.install_reason.is_some()));
    value["apps"][0]["minTools"] = json!("0.2.0+build.99");
    assert!(parse(&value, "0.2.0+build.1").unwrap().apps[0]
        .install_reason
        .is_none());
    value["schema"] = json!(2);
    assert!(parse(&value, "0.2.0").is_err());
}

#[test]
fn conditional_catalog_preserves_last_good_copy_and_offline_snapshot() {
    let server = support::FakeRelease::new();
    let paths = Paths::under(&server.root.path().join("catalog-client"));
    let client = CatalogClient::with_url(
        paths.clone(),
        Source::test_server(&server.url).unwrap(),
        &format!("{}catalog.json", server.url),
    )
    .unwrap();
    assert_eq!(client.cached_catalog().apps.len(), 7);
    let mut value: serde_json::Value = serde_json::from_str(SNAPSHOT).unwrap();
    value["apps"].as_array_mut().unwrap().truncate(1);
    value["apps"][0]["name"] = json!("Catalog display name");
    server
        .routes
        .lock()
        .unwrap()
        .insert("/catalog.json".into(), serde_json::to_vec(&value).unwrap());
    assert_eq!(
        client.refresh_catalog().apps[0].name,
        "Catalog display name"
    );
    assert_eq!(client.refresh_catalog().apps.len(), 1);
    let requests = server.requests.lock().unwrap();
    assert!(requests
        .last()
        .unwrap()
        .1
        .iter()
        .any(|h| h.to_lowercase().starts_with("if-none-match:")));
    drop(requests);
    let last_good = std::fs::read(paths.catalog().join("catalog.json")).unwrap();
    server
        .routes
        .lock()
        .unwrap()
        .insert("/catalog.json".into(), br#"{"schema":9}"#.to_vec());
    assert_eq!(client.refresh_catalog().apps.len(), 1);
    assert_eq!(
        std::fs::read(paths.catalog().join("catalog.json")).unwrap(),
        last_good
    );
    server.routes.lock().unwrap().remove("/catalog.json");
    assert_eq!(client.refresh_catalog().apps.len(), 1);
    let manager = arcade_tools_core::manager::Manager::new(
        paths.clone(),
        Source::test_server(&server.url).unwrap(),
    );
    let views = manager.list(false).unwrap();
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].name, "Catalog display name");
    assert!(manager
        .check("arcade.box")
        .unwrap_err()
        .message
        .contains("Update Arcade Tools"));
    std::fs::write(paths.catalog().join("catalog.json"), "bad cache").unwrap();
    assert_eq!(client.cached_catalog().apps.len(), 7);
    assert_eq!(
        CatalogClient::new(paths, Source::github().unwrap())
            .unwrap()
            .cached_catalog()
            .apps
            .len(),
        7
    );
    assert!(CATALOG_URL.starts_with("https://raw.githubusercontent.com/qa-p1/"));
}

#[test]
fn engines_shortcuts_limits_and_source_boundary() {
    let server = support::FakeRelease::new();
    let paths = Paths::under(&server.root.path().join("catalog-client"));
    let client = CatalogClient::with_url(
        paths,
        Source::test_server(&server.url).unwrap(),
        &format!("{}catalog.json", server.url),
    )
    .unwrap();
    assert_eq!(client.engines(false).unwrap().engines.len(), 11);
    server.routes.lock().unwrap().insert(
        "/engines.json".into(),
        arcade_tools_core::catalog::ENGINE_SNAPSHOT
            .as_bytes()
            .to_vec(),
    );
    assert_eq!(client.engines(true).unwrap().engines.len(), 11);
    server.routes.lock().unwrap().insert("/shortcuts/index.json".into(), serde_json::to_vec(&json!({"schema":1,"sheets":[{"id":"test","name":"Test","file":"shortcuts/test.json","match":{"linux":{"class":["test"]}}}]})).unwrap());
    server.routes.lock().unwrap().insert("/shortcuts/test.json".into(), serde_json::to_vec(&json!({"schema":1,"id":"test","name":"Test","checkedVersion":"1","sources":[],"match":{},"groups":[{"title":"General","context":"app","shortcuts":[{"id":"open","title":"Open","keys":{"default":"Ctrl+O","macos":null}}]}]})).unwrap());
    let index = client.shortcut_index(true).unwrap();
    assert_eq!(
        client
            .shortcut_sheet(&index.sheets[0], true)
            .unwrap()
            .groups
            .len(),
        1
    );
    server.routes.lock().unwrap().clear();
    assert_eq!(client.shortcut_index(false).unwrap().sheets.len(), 1);
    assert_eq!(
        client.shortcut_sheet(&index.sheets[0], false).unwrap().id,
        "test"
    );
    let mut bad = index.sheets[0].clone();
    bad.file = "../escape".into();
    assert!(client.shortcut_sheet(&bad, true).is_err());
    server
        .routes
        .lock()
        .unwrap()
        .insert("/oversized".into(), vec![0; 1025]);
    let source = Source::test_server(&server.url).unwrap();
    assert_eq!(
        source
            .fetch(&format!("{}oversized", server.url), None, 1024)
            .unwrap_err()
            .code,
        "too_large"
    );
    for url in [
        "http://raw.githubusercontent.com/qa-p1/test",
        "https://evil.example/test",
        "https://api.github.com.evil.example/test",
    ] {
        assert_eq!(
            Source::github()
                .unwrap()
                .fetch(url, None, 100)
                .unwrap_err()
                .code,
            "source"
        );
    }
    server.replies.lock().unwrap().insert(
        "/redirect".into(),
        support::HttpReply {
            status: "302 Found".into(),
            headers: vec![("Location".into(), "https://evil.example/".into())],
            body: vec![],
        },
    );
    assert!(source
        .fetch(&format!("{}redirect", server.url), None, 100)
        .is_err());
}

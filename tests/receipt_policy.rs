use arcade_link::receipt::{InstallMethod, Integration, ManagedBy, Receipt, Store};
#[cfg(unix)]
use arcade_tools_core::installation::cleanup_missing_bundle;
use arcade_tools_core::installation::{claim, record_autostart, validate_receipt};
use arcade_tools_core::paths::Paths;
use arcade_tools_core::release::Channel;
use serde_json::{json, Value};
use std::fs;

fn receipt(paths: &Paths, method: InstallMethod) -> Receipt {
    Receipt {
        schema: 1,
        id: "arcade.look".into(),
        version: "0.2.0".into(),
        channel: "stable".into(),
        method,
        managed_by: ManagedBy::SelfManaged,
        path: paths.home.join("Applications/Arcade Look.app"),
        integration: Integration::default(),
        previous: None,
        installed_at: "2026-10-01T00:00:00Z".into(),
        updated_at: "2026-10-01T00:00:00Z".into(),
    }
}

#[test]
fn claiming_and_login_toggles_preserve_installer_extension_fields() {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::under(root.path());
    let receipt = receipt(&paths, InstallMethod::WindowsInstaller);
    let store = Store::new(&paths.link);
    store.write(&receipt).unwrap();
    let file = store.path(&receipt.id).unwrap();
    let mut document: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    document["integration"]["desktopShortcut"] = json!(paths.home.join("Desktop/Arcade Look.lnk"));
    document["futureField"] = json!({"keep": true});
    fs::write(&file, serde_json::to_vec(&document).unwrap()).unwrap();
    let claimed = claim(&paths, receipt.clone(), None).unwrap();
    assert_eq!(claimed.managed_by, ManagedBy::Tools);
    let login = paths.config.join("autostart/arcade-look.desktop");
    record_autostart(&paths, &receipt.id, Some(login.clone())).unwrap();
    assert_eq!(
        store
            .read(&receipt.id)
            .unwrap()
            .unwrap()
            .integration
            .autostart,
        Some(login)
    );
    record_autostart(&paths, &receipt.id, None).unwrap();
    let saved: Value = serde_json::from_slice(&fs::read(file).unwrap()).unwrap();
    assert_eq!(
        saved["integration"]["desktopShortcut"],
        document["integration"]["desktopShortcut"]
    );
    assert_eq!(saved["futureField"], document["futureField"]);
    assert!(saved["integration"]["autostart"].is_null());
}

#[test]
fn custom_windows_uninstallers_must_stay_beside_the_executable_in_the_profile() {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::under(root.path());
    let mut receipt = receipt(&paths, InstallMethod::WindowsInstaller);
    receipt.path = paths.home.join("My Apps/Look/arcade-look.exe");
    fs::create_dir_all(receipt.path.parent().unwrap()).unwrap();
    fs::write(&receipt.path, "fixture").unwrap();
    for name in ["uninstall.exe", "unins000.exe"] {
        receipt.integration.uninstaller = Some(receipt.path.parent().unwrap().join(name));
        validate_receipt(&paths, &receipt, &receipt.id, "0.2.0", Channel::Stable).unwrap();
    }
    receipt.integration.uninstaller = Some(paths.home.join("Other App/uninstall.exe"));
    assert!(validate_receipt(&paths, &receipt, &receipt.id, "0.2.0", Channel::Stable).is_err());
    receipt.integration.uninstaller = Some(receipt.path.parent().unwrap().join("unrelated.exe"));
    assert!(validate_receipt(&paths, &receipt, &receipt.id, "0.2.0", Channel::Stable).is_err());
    receipt.path = root.path().parent().unwrap().join("Other/arcade-look.exe");
    assert!(validate_receipt(&paths, &receipt, &receipt.id, "0.2.0", Channel::Stable).is_err());
}

#[test]
fn native_tools_views_explain_the_first_windows_update_rollback_limit() {
    use arcade_tools_core::{manager::Manager, source::Source};
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::under(root.path());
    let mut receipt = receipt(&paths, InstallMethod::WindowsInstaller);
    receipt.id = "arcade.tools".into();
    receipt.path = paths.home.join("My Apps/Tools/arcade-tools.exe");
    fs::create_dir_all(receipt.path.parent().unwrap()).unwrap();
    fs::write(&receipt.path, "fixture").unwrap();
    Store::new(&paths.link).write(&receipt).unwrap();
    let manager = Manager::new(paths, Source::github().unwrap());
    let view = manager.tools_view().unwrap().unwrap();
    assert!(view.installed);
    assert!(view.rollback_version.is_none());
    assert!(view.reason.unwrap().contains("first Windows update"));
}

#[cfg(unix)]
#[test]
fn app_login_writers_accept_link_entries_and_update_receipts() {
    use arcade_tools_core::{platform, release::Os};
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::under(root.path());
    let mut receipt = receipt(&paths, InstallMethod::Appimage);
    receipt.path = paths.home.join("Applications/Look.AppImage");
    fs::create_dir_all(receipt.path.parent().unwrap()).unwrap();
    fs::write(&receipt.path, "fixture").unwrap();
    let store = Store::new(&paths.link);
    store.write(&receipt).unwrap();
    let path = paths.autostart(&receipt.id, Os::current());
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let text = if Os::current() == Os::Linux {
        platform::desktop_entry(&receipt.id, &receipt.path).unwrap()
            + "X-Arcade-Id=arcade.look\nOnlyShowIn=GNOME;\n"
    } else {
        platform::launch_agent(&receipt.id, &receipt.path)
    };
    fs::write(&path, text).unwrap();
    platform::set_autostart(&paths, &receipt.id, &receipt.path, true).unwrap();
    assert_eq!(
        store
            .read(&receipt.id)
            .unwrap()
            .unwrap()
            .integration
            .autostart,
        Some(path.clone())
    );
    platform::set_autostart(&paths, &receipt.id, &receipt.path, false).unwrap();
    assert!(!path.exists());
    assert!(store
        .read(&receipt.id)
        .unwrap()
        .unwrap()
        .integration
        .autostart
        .is_none());
}

#[cfg(unix)]
#[test]
fn missing_bundles_clean_recorded_integration_and_keep_retargeted_links() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::under(root.path());
    let mut receipt = receipt(&paths, InstallMethod::MacosBundle);
    let cli = paths.home.join(".local/bin/arcade-look");
    let login = paths.home.join("Library/LaunchAgents/arcade.look.plist");
    fs::create_dir_all(cli.parent().unwrap()).unwrap();
    fs::create_dir_all(login.parent().unwrap()).unwrap();
    symlink(receipt.path.join("Contents/MacOS/arcade-look"), &cli).unwrap();
    fs::write(&login, "owned login item").unwrap();
    let unrelated = login.parent().unwrap().join("unrelated.plist");
    fs::write(&unrelated, "keep").unwrap();
    receipt.integration.cli = Some(cli.clone());
    receipt.integration.autostart = Some(login.clone());
    cleanup_missing_bundle(&paths, &receipt).unwrap();
    assert!(fs::symlink_metadata(&cli).is_err());
    assert!(!login.exists());
    assert!(unrelated.exists());
    symlink(receipt.path.join("../Other.app/executable"), &cli).unwrap();
    symlink(paths.home.join("Other.app/executable"), &login).unwrap();
    cleanup_missing_bundle(&paths, &receipt).unwrap();
    assert!(fs::symlink_metadata(&cli).unwrap().file_type().is_symlink());
    assert!(fs::symlink_metadata(&login)
        .unwrap()
        .file_type()
        .is_symlink());
}

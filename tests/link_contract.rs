use arcade_link::wire::InvokeRequest;
use arcade_link::Content;
use arcade_tools_core::link::{install_target, manifest};
use serde_json::json;

#[test]
fn get_handoff_validates_before_showing_confirmation() {
    let request =
        InvokeRequest::new("tools.install", "arcade.look").options(json!({"app": "arcade.box"}));
    assert_eq!(install_target(&request).unwrap(), "arcade.box");
    let text =
        InvokeRequest::new("tools.install", "arcade.look").input(Content::plain("arcade.lens"));
    assert_eq!(install_target(&text).unwrap(), "arcade.lens");
    assert!(install_target(
        &InvokeRequest::new("tools.install", "test").options(json!({"app":"../../escape"}))
    )
    .is_err());
    let mut background = request.clone();
    background.context.interactive = false;
    assert_eq!(
        install_target(&background).unwrap_err().code.as_str(),
        "denied"
    );
    let big = InvokeRequest::new("tools.install", "test").input(Content::plain("x".repeat(65)));
    assert_eq!(install_target(&big).unwrap_err().code.as_str(), "too_large");
    let mut wrong = request.clone();
    wrong.version = Some(2);
    assert_eq!(
        install_target(&wrong).unwrap_err().code.as_str(),
        "version_mismatch"
    );
    let disabled = manifest(false);
    assert!(!disabled.settings.link_enabled && disabled.actions.is_empty());
    let action = manifest(true).actions.remove(0);
    assert!(action.interactive && action.effects == ["opens-ui"] && action.max_bytes == Some(64));
}
#[test]
fn changing_a_desktop_login_entry_preserves_unrelated_settings() {
    let original = "[Desktop Entry]\nType=Application\nName=My Look\nComment=keep this\nExec=old\nHidden=true\nX-Custom=keep\n";
    let proposed = arcade_tools_core::platform::desktop_entry(
        "arcade.look",
        std::path::Path::new("/home/u/Applications/Arcade-Look.AppImage"),
    )
    .unwrap();
    let updated = arcade_tools_core::platform::update_desktop(original, &proposed).unwrap();
    assert!(
        updated.contains("Comment=keep this")
            && updated.contains("Name=My Look")
            && updated.contains("X-Custom=keep")
    );
    assert!(!updated.contains("Exec=old") && !updated.contains("Hidden=true"));
    assert!(updated.contains("X-GNOME-Autostart-enabled=true"));
}

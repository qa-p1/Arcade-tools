//! What an app's settings export may carry into a Tools backup: the handed-off file must be the asking app's own,
//! inside the handoff root, and its JSON must hold no secrets, identities or app-private data (keys, history,
//! indexes). Used by `backup.rs`.
use arcade_link::content::Content;
use arcade_link::manifest::ids;
use serde_json::Value;
use std::fs;
use std::path::{Component, Path, PathBuf};

/// Resolves an app's settings export to a file inside the handoff root.
/// `app_id` is the app Tools asked for the export. Some apps (Wheel) omit
/// `owner`; the export is then attributed to the app that answered the call.
/// An owner naming a different app is always rejected.
pub(crate) fn handoff_source(root: &Path, content: &Content, app_id: &str) -> Option<PathBuf> {
    if content
        .owner
        .as_deref()
        .is_some_and(|owner| owner != app_id)
        || !content.kind.starts_with("file/")
        || content.kind.ends_with("[]")
        || !content.paths.is_empty()
        || content.text.is_some()
        || content.html.is_some()
        || content.data.is_some()
    {
        return None;
    }
    let path = PathBuf::from(content.path.as_deref()?);
    if !path.is_absolute() || contains_symlink_component(&path).ok()? {
        return None;
    }
    let root = root.canonicalize().ok()?;
    let path = path.canonicalize().ok()?;
    if !path.starts_with(root) || !fs::metadata(&path).ok()?.is_file() {
        return None;
    }
    let size = fs::metadata(&path).ok()?.len();
    if content.size.is_some_and(|expected| expected != size) {
        return None;
    }
    Some(path)
}

/// Classifies an app's settings export: `Some(false)` for JSON that passed the
/// excluded-data scan, `Some(true)` for a non-JSON app-defined format (Shelf's
/// binary export) and `None` for JSON holding excluded data. Tools cannot
/// interpret opaque formats, so their privacy boundary is the app's own export
/// contract; Tools only bounds, hashes and returns the bytes unchanged.
pub(crate) fn export_is_opaque(bytes: &[u8], app_id: &str) -> Option<bool> {
    match serde_json::from_slice::<Value>(bytes) {
        Ok(value) if contains_excluded_data(&value, app_id) => None,
        Ok(_) => Some(false),
        Err(_) => Some(true),
    }
}

fn contains_excluded_data(value: &Value, app_id: &str) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(key, child)| {
            let normalized: String = key
                .chars()
                .filter(|character| character.is_ascii_alphanumeric())
                .flat_map(char::to_lowercase)
                .collect();
            let generic_exclusion = [
                "token",
                "secret",
                "password",
                "credential",
                "authorization",
                "apikey",
                "accesskey",
                "privatekey",
                "deviceid",
                "deviceidentity",
                "keyring",
                "identity",
            ]
            .iter()
            .any(|needle| normalized.contains(needle))
                // A boolean, number or null under a secret-sounding name is a
                // preference such as Lens's `privacy.guard_secrets`, not
                // secret material. Strings, arrays (bytes) and objects stay
                // excluded.
                && holds_material(child);
            let app_exclusion = app_id == ids::CLIPBOARD
                && ["key", "keys", "history", "device"].contains(&normalized.as_str())
                || app_id == ids::FIND
                    && ["index", "cache", "caches"].contains(&normalized.as_str());
            generic_exclusion || app_exclusion || contains_excluded_data(child, app_id)
        }),
        Value::Array(items) => items
            .iter()
            .any(|child| contains_excluded_data(child, app_id)),
        Value::String(text) => crate::diagnostics_archive::has_secret_markers(text),
        _ => false,
    }
}

fn holds_material(value: &Value) -> bool {
    matches!(value, Value::String(_) | Value::Array(_) | Value::Object(_))
}

fn contains_symlink_component(path: &Path) -> crate::Result<bool> {
    if !path.is_absolute() {
        return Ok(false);
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            // A bare drive prefix (`C:` or canonical `\\?\C:`) is not a path
            // that can be stat'ed on its own; check from the root onward.
            Component::Prefix(prefix) => {
                current.push(prefix.as_os_str());
                continue;
            }
            Component::RootDir => current.push(component.as_os_str()),
            Component::Normal(part) => current.push(part),
            Component::CurDir => continue,
            Component::ParentDir => return Ok(true),
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata)
                if metadata.file_type().is_symlink() && !crate::paths::system_alias(&current) =>
            {
                return Ok(true)
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rejects_clipboard_identities_keys_history_find_index_and_credentials() {
        assert!(contains_excluded_data(
            &json!({ "device": "phone", "keys": ["key"] }),
            ids::CLIPBOARD
        ));
        assert!(contains_excluded_data(
            &json!({ "history": [] }),
            ids::CLIPBOARD
        ));
        assert!(contains_excluded_data(&json!({ "index": {} }), ids::FIND));
        assert!(contains_excluded_data(
            &json!({ "credential": "x" }),
            ids::FIND
        ));
    }

    #[test]
    fn lens_boolean_secret_toggle_is_not_excluded() {
        let lens = json!({
            "privacy": {
                "history_enabled": false,
                "learn_action_usage": true,
                "guard_secrets": true
            },
            "secret_scan_limit": 4096,
            "token_budget": null
        });
        assert!(!contains_excluded_data(&lens, ids::LENS));
        assert_eq!(
            export_is_opaque(&serde_json::to_vec(&lens).unwrap(), ids::LENS),
            Some(false)
        );
    }

    #[test]
    fn real_secret_material_is_still_excluded() {
        for value in [
            json!({ "privacy": { "guard_secrets": "s3cr3t-value" } }),
            json!({ "api_token": "abc123" }),
            json!({ "password": "hunter2" }),
            json!({ "private_key": [1, 2, 3] }),
            json!({ "credentials": { "user": "me" } }),
            json!({ "device_identity": "phone-1" }),
            json!({ "notes": "ghp_0123456789abcdefghijklmnopqrstuvwxyz" }),
        ] {
            assert!(contains_excluded_data(&value, ids::LENS), "{value}");
            assert_eq!(
                export_is_opaque(&serde_json::to_vec(&value).unwrap(), ids::LENS),
                None
            );
        }
    }

    #[test]
    fn missing_owner_is_attributed_to_the_requested_app() {
        let root = std::env::temp_dir().join(format!("tools-handoff-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("wheel-settings.json");
        fs::write(&path, br#"{"ring":[]}"#).unwrap();
        let mut content = Content::file(&path);
        content.kind = "file/any".into();
        content.owner = None;
        assert!(handoff_source(&root, &content, ids::WHEEL).is_some());
        content.owner = Some(ids::WHEEL.into());
        assert!(handoff_source(&root, &content, ids::WHEEL).is_some());
        content.owner = Some(ids::LENS.into());
        assert!(handoff_source(&root, &content, ids::WHEEL).is_none());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_non_json_export_is_opaque() {
        let mut payload = b"ARCADE-SHELF-PORTABLE\n".to_vec();
        payload.extend_from_slice(&[0, 0, 0, 2, b'{', b'}', 0xff, 0xfe, 0x00]);
        assert_eq!(export_is_opaque(&payload, ids::SHELF), Some(true));
    }
}

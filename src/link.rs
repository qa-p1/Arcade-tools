//! `tools.install` hands off to a confirmation screen; it never installs silently.
use arcade_link::manifest::{self, ids, Action, Manifest};
use arcade_link::wire::InvokeRequest;
use arcade_link::{ErrorCode, LinkError};

pub fn actions() -> Vec<Action> {
    let mut action = Action::new("tools.install", "Install an Arcade app", "install")
        .accepts(&["text/plain"])
        .effects(&["opens-ui"])
        .interactive(true);
    action.max_bytes = Some(64);
    vec![action]
}
pub fn manifest(enabled: bool) -> Manifest {
    let mut manifest = Manifest::new(
        ids::TOOLS,
        env!("CARGO_PKG_VERSION"),
        &manifest::current_executable(),
    );
    manifest.settings.link_enabled = enabled;
    if enabled {
        manifest.actions = actions();
    }
    manifest
}
pub fn install_target(request: &InvokeRequest) -> Result<String, LinkError> {
    if request.action != "tools.install" {
        return Err(LinkError::unavailable("Unknown Tools action."));
    }
    if request.version.is_some_and(|v| v != 1) {
        return Err(LinkError::new(
            ErrorCode::VersionMismatch,
            "tools.install requires action version 1",
        ));
    }
    if !request.context.interactive {
        return Err(LinkError::denied("user_cancelled"));
    }
    if request.inputs.len() > 1 {
        return Err(LinkError::unsupported("Pass one Arcade app ID."));
    }
    let input = request.inputs.first();
    if input.is_some_and(|c| c.kind != "text/plain" || c.path.is_some() || c.text.is_none()) {
        return Err(LinkError::unsupported(
            "tools.install accepts an app ID as text/plain or options.app.",
        ));
    }
    let options = request
        .options
        .get("app")
        .or_else(|| request.options.get("id"));
    if options.is_some_and(|v| !v.is_string()) {
        return Err(LinkError::unsupported("options.app must be a string."));
    }
    let id = options
        .and_then(|v| v.as_str())
        .or_else(|| input.and_then(|c| c.text.as_deref()))
        .ok_or_else(|| {
            LinkError::unsupported("tools.install needs options.app or a text/plain app ID.")
        })?;
    if id.len() > 64
        || input
            .and_then(|c| c.text.as_ref())
            .is_some_and(|s| s.len() > 64)
    {
        return Err(LinkError::too_large(64));
    }
    if !crate::apps::APPS.contains(&id) {
        return Err(LinkError::unsupported("Unknown Arcade app."));
    }
    Ok(id.into())
}

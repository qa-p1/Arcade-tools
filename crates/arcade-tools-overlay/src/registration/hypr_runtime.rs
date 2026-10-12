//! Runtime-only Hyprland binds; no configuration or startup files are written.
use super::{hyprland, HotKey, Modifiers};
use std::{
    io::Read,
    process::{Command, Stdio},
    time::Duration,
};
use wait_timeout::ChildExt;

pub(super) struct Active;
impl Drop for Active {
    fn drop(&mut self) {
        let _ = command(&["eval".into(), "if __arcade_tools_shortcut then __arcade_tools_shortcut:unbind(); __arcade_tools_shortcut=nil end".into()]);
    }
}

pub(super) fn register(key: HotKey, executable: &std::path::Path) -> Result<Active, String> {
    let binds: serde_json::Value = serde_json::from_str(&command(&["-j".into(), "binds".into()])?)
        .map_err(|e| e.to_string())?;
    let (mods, symbol) = hyprland::keys(key)?;
    let mask = [
        (Modifiers::SUPER, 128),
        (Modifiers::CONTROL, 4),
        (Modifiers::ALT, 8),
        (Modifiers::SHIFT, 1),
    ]
    .iter()
    .filter(|(modifier, _)| key.mods.contains(*modifier))
    .map(|(_, mask)| mask)
    .sum::<u64>();
    for bind in binds.as_array().into_iter().flatten() {
        let same = bind["modmask"].as_u64() == Some(mask)
            && bind["key"]
                .as_str()
                .is_some_and(|k| k.eq_ignore_ascii_case(&symbol));
        if same
            && bind["submap"].as_str().unwrap_or_default().is_empty()
            && bind["description"] != "Arcade Tools shortcuts"
        {
            return Err(
                "The overlay shortcut is already used by Hyprland. Choose another shortcut.".into(),
            );
        }
    }
    let chord = format!("{} + {symbol}", mods.replace(' ', " + "));
    let launch = format!(
        "{} --toggle",
        hyprland::shell_quote(&executable.to_string_lossy())
    );
    let lua = format!("if __arcade_tools_shortcut then __arcade_tools_shortcut:unbind() end; __arcade_tools_shortcut=hl.bind({}, hl.dsp.exec_cmd({}), {{description=\"Arcade Tools shortcuts\"}})",hyprland::lua_string(&chord),hyprland::lua_string(&launch));
    let result = command(&["eval".into(), lua]);
    match result {
        Ok(output) if !output.to_ascii_lowercase().contains("error") => Ok(Active),
        Ok(output) => Err(output),
        Err(error) => Err(error),
    }
}

fn command(args: &[String]) -> Result<String, String> {
    let mut child = Command::new("hyprctl")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let out = child.stdout.take().unwrap();
    let err = child.stderr.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = out.take(1024 * 1024).read_to_end(&mut bytes);
        bytes
    });
    let errors = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = err.take(64 * 1024).read_to_end(&mut bytes);
        bytes
    });
    let status = match child
        .wait_timeout(Duration::from_secs(3))
        .map_err(|e| e.to_string())?
    {
        Some(status) => status,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Hyprland shortcut registration timed out.".into());
        }
    };
    let stdout = String::from_utf8_lossy(&reader.join().unwrap_or_default()).into_owned();
    let stderr = String::from_utf8_lossy(&errors.join().unwrap_or_default()).into_owned();
    if status.success() {
        Ok(stdout)
    } else {
        Err(format!(
            "Hyprland shortcut registration failed: {stdout}{stderr}"
        ))
    }
}

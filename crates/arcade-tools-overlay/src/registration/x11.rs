//! A blocking X11 reader; global-hotkey's Linux reader polls every 50 ms.
use super::{Code, HotKey, Modifiers};
use std::{os::fd::AsRawFd, sync::Arc};
use x11rb::{
    connection::Connection,
    protocol::{
        xkb::{self, PerClientFlag},
        xproto::{ConnectionExt, GrabMode, ModMask},
        Event,
    },
    rust_connection::RustConnection,
};

pub(super) struct Active(Arc<RustConnection>);

impl Drop for Active {
    fn drop(&mut self) {
        // SAFETY: the Arc keeps this X11 socket alive through shutdown. This
        // wakes wait_for_event and releases the connection's passive grabs.
        unsafe { libc::shutdown(self.0.stream().as_raw_fd(), libc::SHUT_RDWR) };
    }
}

pub(super) fn register(
    key: HotKey,
    pressed: impl Fn() + Send + Sync + 'static,
) -> Result<Active, String> {
    let symbol = keysym(key.key).ok_or("Unsupported X11 shortcut key")?;
    let (connection, screen) = x11rb::connect(None).map_err(|error| error.to_string())?;
    let connection = Arc::new(connection);
    let setup = connection.setup();
    let root = setup.roots[screen].root;
    let first = setup.min_keycode;
    let count = setup.max_keycode - first + 1;
    let mapping = connection
        .get_keyboard_mapping(first, count)
        .map_err(|error| error.to_string())?
        .reply()
        .map_err(|error| error.to_string())?;
    let code = mapping
        .keysyms
        .chunks(usize::from(mapping.keysyms_per_keycode))
        .position(|symbols| symbols.contains(&symbol))
        .map(|offset| first + offset as u8)
        .ok_or("Shortcut key is missing from the X11 keyboard map")?;
    let mut mask = ModMask::default();
    for (modifier, x11) in [
        (Modifiers::SHIFT, ModMask::SHIFT),
        (Modifiers::CONTROL, ModMask::CONTROL),
        (Modifiers::ALT, ModMask::M1),
        (Modifiers::SUPER, ModMask::M4),
    ] {
        if key.mods.contains(modifier) {
            mask |= x11;
        }
    }
    for ignored in [
        ModMask::default(),
        ModMask::LOCK,
        ModMask::M2,
        ModMask::LOCK | ModMask::M2,
    ] {
        connection
            .grab_key(
                false,
                root,
                mask | ignored,
                code,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
            )
            .map_err(|error| error.to_string())?
            .check()
            .map_err(|error| format!("Could not register shortcut (it may be in use): {error}"))?;
    }
    xkb::use_extension(&*connection, 1, 0)
        .map_err(|error| error.to_string())?
        .reply()
        .map_err(|error| error.to_string())?;
    xkb::per_client_flags(
        &*connection,
        0x100,
        PerClientFlag::DETECTABLE_AUTO_REPEAT,
        PerClientFlag::DETECTABLE_AUTO_REPEAT,
        0u32.into(),
        0u32.into(),
        0u32.into(),
    )
    .map_err(|error| error.to_string())?
    .reply()
    .map_err(|error| error.to_string())?;
    connection.flush().map_err(|error| error.to_string())?;
    let active = Active(connection.clone());
    std::thread::Builder::new()
        .name("tools-shortcut-x11".into())
        .spawn(move || {
            let mut down = false;
            while let Ok(event) = connection.wait_for_event() {
                match event {
                    Event::KeyPress(event) if event.detail == code => {
                        let modifiers =
                            u16::from(event.state) & !u16::from(ModMask::LOCK | ModMask::M2);
                        if modifiers == u16::from(mask) && !down {
                            down = true;
                            pressed();
                        }
                    }
                    Event::KeyRelease(event) if event.detail == code => down = false,
                    _ => {}
                }
            }
        })
        .map_err(|error| error.to_string())?;
    Ok(active)
}

fn keysym(code: Code) -> Option<u32> {
    let name = format!("{code:?}");
    if let Some(letter) = name.strip_prefix("Key").filter(|letter| letter.len() == 1) {
        return Some(u32::from(letter.as_bytes()[0].to_ascii_lowercase()));
    }
    if let Some(digit) = name.strip_prefix("Digit").filter(|digit| digit.len() == 1) {
        return Some(u32::from(digit.as_bytes()[0]));
    }
    if let Some(number) = name
        .strip_prefix('F')
        .and_then(|number| number.parse::<u32>().ok())
        .filter(|number| (1..=24).contains(number))
    {
        return Some(0xffbd + number);
    }
    Some(match code {
        Code::Slash => u32::from(b'/'),
        Code::Space => u32::from(b' '),
        Code::Minus => u32::from(b'-'),
        Code::Equal => u32::from(b'='),
        Code::Comma => u32::from(b','),
        Code::Period => u32::from(b'.'),
        Code::Backquote => u32::from(b'`'),
        Code::Semicolon => u32::from(b';'),
        Code::Quote => u32::from(b'\''),
        Code::BracketLeft => u32::from(b'['),
        Code::BracketRight => u32::from(b']'),
        Code::Backslash => u32::from(b'\\'),
        Code::Tab => 0xff09,
        Code::Enter => 0xff0d,
        Code::Escape => 0xff1b,
        Code::Backspace => 0xff08,
        Code::Delete => 0xffff,
        Code::Insert => 0xff63,
        Code::ArrowLeft => 0xff51,
        Code::ArrowUp => 0xff52,
        Code::ArrowRight => 0xff53,
        Code::ArrowDown => 0xff54,
        Code::Home => 0xff50,
        Code::End => 0xff57,
        Code::PageUp => 0xff55,
        Code::PageDown => 0xff56,
        _ => return None,
    })
}

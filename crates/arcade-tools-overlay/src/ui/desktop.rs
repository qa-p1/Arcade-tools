//! Borderless desktop window adapted from Arcade Find's `ui/desktop.rs`
//! (MIT OR Apache-2.0). No resident event loop remains after close.

use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key as WinitKey, ModifiersState, NamedKey};
use winit::window::{Window, WindowId, WindowLevel};

use super::{Key, Overlay};
use crate::{
    focus::Focus,
    instance::{Command, Instance},
    layout::{HEIGHT, WIDTH},
};

struct App {
    overlay: Overlay,
    focus: Focus,
    window: Option<Rc<Window>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    mods: ModifiersState,
    cursor: (f32, f32),
    focused: bool,
    leave_at: Option<Instant>,
    error: Option<String>,
    ime: bool,
}

pub fn run(overlay: Overlay, focus: Focus, instance: &Instance) -> Result<(), String> {
    let began = Instant::now();
    #[allow(unused_mut)]
    let mut builder = EventLoop::<Command>::with_user_event();
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        builder.with_activation_policy(ActivationPolicy::Accessory);
    }
    let event_loop = builder.build().map_err(|e| e.to_string())?;
    let proxy = Arc::new(Mutex::new(event_loop.create_proxy()));
    instance
        .serve(Arc::new(move |command| {
            if let Ok(proxy) = proxy.lock() {
                let _ = proxy.send_event(command);
            }
        }))
        .map_err(|e| e.to_string())?;
    overlay.timing.phase("desktop-init", began);
    let mut app = App {
        overlay,
        focus,
        window: None,
        surface: None,
        mods: ModifiersState::empty(),
        cursor: (0.0, 0.0),
        focused: false,
        leave_at: None,
        error: None,
        ime: false,
    };
    event_loop.run_app(&mut app).map_err(|e| e.to_string())?;
    app.surface = None;
    app.window = None;
    if app.overlay.restore_focus {
        crate::focus::restore(&app.focus);
    }
    app.error.map_or(Ok(()), Err)
}

impl App {
    fn close(&mut self, event_loop: &ActiveEventLoop, restore: bool) {
        self.overlay.restore_focus = restore;
        if let Some(window) = &self.window {
            window.set_visible(false);
        }
        event_loop.exit();
    }

    fn redraw(&self) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn draw(&mut self) -> Result<(), String> {
        let Some(window) = &self.window else {
            return Ok(());
        };
        let size = window.inner_size();
        let (Some(width), Some(height)) =
            (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            return Ok(());
        };
        self.overlay.width = size.width;
        self.overlay.height = size.height;
        self.overlay.scale = window.scale_factor() as f32;
        let frame = self.overlay.draw()?;
        let surface = self.surface.as_mut().ok_or("missing drawing surface")?;
        surface.resize(width, height).map_err(|e| e.to_string())?;
        let mut pixels = surface.buffer_mut().map_err(|e| e.to_string())?;
        let background = if self.overlay.renderer.dark {
            0x14171c
        } else {
            0xffffff
        };
        for (dst, rgba) in pixels
            .iter_mut()
            .zip(frame.data().as_chunks::<4>().0.iter())
        {
            // Desktop surface is opaque, including the thin panel margin.
            *dst = if rgba[3] == 0 {
                background
            } else {
                (u32::from(rgba[0]) << 16) | (u32::from(rgba[1]) << 8) | u32::from(rgba[2])
            };
        }
        window.pre_present_notify();
        pixels.present().map_err(|e| e.to_string())?;
        self.overlay.presented(if crate::focus::is_wayland() {
            "wayland-window"
        } else {
            "desktop"
        });
        Ok(())
    }
}

impl ApplicationHandler<Command> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let began = Instant::now();
        #[allow(unused_mut)]
        let mut attributes = Window::default_attributes()
            .with_title("Arcade Tools Shortcuts")
            .with_decorations(false)
            .with_resizable(false)
            .with_visible(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_inner_size(LogicalSize::new(WIDTH as f64, HEIGHT as f64));
        #[cfg(target_os = "linux")]
        {
            use winit::platform::x11::{WindowAttributesExtX11, WindowType};
            attributes = attributes
                .with_name("arcade-tools-overlay", "Arcade Tools Shortcuts")
                .with_x11_window_type(vec![WindowType::Utility]);
        }
        #[cfg(windows)]
        {
            use winit::platform::windows::WindowAttributesExtWindows;
            attributes = attributes.with_skip_taskbar(true);
        }
        let result = (|| {
            let window = Rc::new(
                event_loop
                    .create_window(attributes)
                    .map_err(|e| e.to_string())?,
            );
            #[cfg(any(windows, target_os = "macos"))]
            if let Some(theme) = window.theme() {
                self.overlay.renderer.dark = theme == winit::window::Theme::Dark;
            }
            if let Some(monitor) = window
                .current_monitor()
                .or_else(|| window.primary_monitor())
            {
                let size = monitor.size();
                let scale = window.scale_factor();
                let width = (WIDTH as f64)
                    .min(size.width as f64 / scale - 24.0)
                    .max(300.0);
                let height = (HEIGHT as f64)
                    .min(size.height as f64 / scale - 24.0)
                    .max(280.0);
                let _ = window.request_inner_size(LogicalSize::new(width, height));
                let position = monitor.position();
                window.set_outer_position(PhysicalPosition::new(
                    position.x + (size.width as i32 - (width * scale) as i32) / 2,
                    position.y + (size.height as i32 - (height * scale) as i32) / 2,
                ));
            }
            let context = softbuffer::Context::new(window.clone()).map_err(|e| e.to_string())?;
            self.surface = Some(
                softbuffer::Surface::new(&context, window.clone()).map_err(|e| e.to_string())?,
            );
            window.set_ime_allowed(true);
            window.set_visible(true);
            window.focus_window();
            window.request_redraw();
            self.window = Some(window);
            Ok::<_, String>(())
        })();
        self.overlay.timing.phase("window", began);
        if let Err(error) = result {
            self.error = Some(error);
            event_loop.exit();
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, command: Command) {
        match command {
            Command::Toggle | Command::Hide => self.close(event_loop, true),
            Command::Show => {
                if let Some(window) = &self.window {
                    window.focus_window();
                }
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => self.close(event_loop, true),
            WindowEvent::RedrawRequested => {
                if let Err(error) = self.draw() {
                    self.error = Some(error);
                    self.close(event_loop, true);
                }
            }
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => self.redraw(),
            WindowEvent::ModifiersChanged(mods) => self.mods = mods.state(),
            WindowEvent::ThemeChanged(theme) => {
                self.overlay.renderer.dark = theme == winit::window::Theme::Dark;
                self.redraw();
            }
            WindowEvent::Focused(true) => {
                self.focused = true;
                self.leave_at = None;
                event_loop.set_control_flow(ControlFlow::Wait);
            }
            WindowEvent::Focused(false) if self.focused => {
                self.leave_at = Some(Instant::now() + Duration::from_millis(150));
                event_loop.set_control_flow(ControlFlow::WaitUntil(self.leave_at.unwrap()));
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (
                    position.x as f32 / self.overlay.scale,
                    position.y as f32 / self.overlay.scale,
                );
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                if self.overlay.click(self.cursor.0, self.cursor.1) {
                    self.close(event_loop, true);
                } else {
                    self.redraw();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let delta = match delta {
                    MouseScrollDelta::LineDelta(_, lines) => -lines * crate::layout::ROW_HEIGHT,
                    MouseScrollDelta::PixelDelta(pixels) => -pixels.y as f32 / self.overlay.scale,
                };
                self.overlay.scroll(delta);
                self.redraw();
            }
            // Enabling the IME does not mean text is being composed. Ordinary
            // key events still carry text (including on X11 with XIM enabled).
            WindowEvent::Ime(Ime::Preedit(text, _)) => self.ime = !text.is_empty(),
            WindowEvent::Ime(Ime::Disabled) => self.ime = false,
            WindowEvent::Ime(Ime::Commit(text)) => {
                self.ime = false;
                self.overlay.key(Key::Text(text), false);
                self.redraw();
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                let key = match &event.logical_key {
                    WinitKey::Named(NamedKey::Escape) => Some(Key::Escape),
                    WinitKey::Named(NamedKey::Tab) => Some(Key::Tab),
                    WinitKey::Named(NamedKey::Backspace) => Some(if self.mods.control_key() {
                        Key::Clear
                    } else {
                        Key::Backspace
                    }),
                    WinitKey::Named(NamedKey::Enter) => Some(Key::Enter),
                    WinitKey::Named(NamedKey::ArrowUp | NamedKey::ArrowLeft) => Some(Key::Up),
                    WinitKey::Named(NamedKey::ArrowDown | NamedKey::ArrowRight) => Some(Key::Down),
                    WinitKey::Named(NamedKey::PageUp) => Some(Key::PageUp),
                    WinitKey::Named(NamedKey::PageDown) => Some(Key::PageDown),
                    WinitKey::Named(NamedKey::Home) => Some(Key::Home),
                    WinitKey::Named(NamedKey::End) => Some(Key::End),
                    WinitKey::Character(text)
                        if self.mods.control_key() && text.eq_ignore_ascii_case("p") =>
                    {
                        Some(Key::Picker)
                    }
                    WinitKey::Character(text)
                        if self.mods.control_key() && text.eq_ignore_ascii_case("u") =>
                    {
                        Some(Key::Clear)
                    }
                    _ if !self.ime
                        && !self.mods.control_key()
                        && !self.mods.super_key()
                        && !self.mods.alt_key() =>
                    {
                        event.text.map(|text| Key::Text(text.to_string()))
                    }
                    _ => None,
                };
                if let Some(key) = key {
                    if self.overlay.key(key, self.mods.shift_key()) {
                        self.close(event_loop, true);
                    } else {
                        self.redraw();
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self
            .leave_at
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.close(event_loop, false);
        }
    }
}

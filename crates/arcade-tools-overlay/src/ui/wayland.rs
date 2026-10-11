//! Layer-shell lifecycle, keyboard and fractional scaling adapted from
//! Arcade Find's `ui/wayland.rs` (MIT OR Apache-2.0). The full-output surface
//! catches outside clicks; the centered sheet's background is fully opaque.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use smithay_client_toolkit::reexports::calloop::channel::{channel, Event as ChannelEvent};
use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::reexports::calloop::{EventLoop, LoopHandle, RegistrationToken};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::protocols::wp::fractional_scale::v1::client::{
    wp_fractional_scale_manager_v1, wp_fractional_scale_v1,
};
use smithay_client_toolkit::reexports::protocols::wp::viewporter::client::{
    wp_viewport, wp_viewporter,
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_pointer,
    delegate_registry, delegate_seat, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use wayland_client::globals::{registry_queue_init, GlobalList};
use wayland_client::protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle};

use super::{Key, Overlay};
use crate::instance::{Command, Instance};

pub struct Setup {
    connection: Connection,
    globals: GlobalList,
    queue: EventQueue<Wl>,
}

pub fn prepare() -> Result<Option<Setup>, String> {
    let connection = Connection::connect_to_env().map_err(|e| e.to_string())?;
    let (globals, queue) = registry_queue_init::<Wl>(&connection).map_err(|e| e.to_string())?;
    let available = globals.contents().with_list(|list| {
        list.iter()
            .any(|global| global.interface == "zwlr_layer_shell_v1")
    });
    Ok(available.then_some(Setup {
        connection,
        globals,
        queue,
    }))
}

struct Shown {
    layer: LayerSurface,
    viewport: Option<wp_viewport::WpViewport>,
    fractional: Option<wp_fractional_scale_v1::WpFractionalScaleV1>,
}

struct Wl {
    overlay: Overlay,
    registry: RegistryState,
    seats: SeatState,
    outputs: OutputState,
    shm: Shm,
    pool: SlotPool,
    shown: Option<Shown>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    mods: Modifiers,
    int_scale: i32,
    frac_scale: Option<u32>,
    logical_size: (u32, u32),
    configured: bool,
    focus: bool,
    exit: bool,
    error: Option<String>,
    qh: QueueHandle<Wl>,
    handle: LoopHandle<'static, Wl>,
    leave_timer: Option<RegistrationToken>,
    frame_requested: bool,
}

pub fn run(overlay: Overlay, instance: &Instance, setup: Setup) -> Result<(), String> {
    let began = Instant::now();
    let Setup {
        connection,
        globals,
        queue,
    } = setup;
    let qh = queue.handle();
    let mut event_loop: EventLoop<'static, Wl> = EventLoop::try_new().map_err(|e| e.to_string())?;
    let handle = event_loop.handle();
    WaylandSource::new(connection.clone(), queue)
        .insert(handle.clone())
        .map_err(|e| e.to_string())?;
    let compositor = CompositorState::bind(&globals, &qh).map_err(|e| e.to_string())?;
    let shell = LayerShell::bind(&globals, &qh).map_err(|e| e.to_string())?;
    let shm = Shm::bind(&globals, &qh).map_err(|e| e.to_string())?;
    let pool = SlotPool::new(4 * 1024 * 1024, &shm).map_err(|e| e.to_string())?;
    let surface = compositor.create_surface(&qh);
    let manager = globals
        .bind::<wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1, _, _>(&qh, 1..=1, ())
        .ok();
    let viewporter = globals
        .bind::<wp_viewporter::WpViewporter, _, _>(&qh, 1..=1, ())
        .ok();
    let fractional = manager.map(|manager| manager.get_fractional_scale(&surface, &qh, ()));
    let viewport = viewporter.map(|manager| manager.get_viewport(&surface, &qh, ()));
    let layer = shell.create_layer_surface(
        &qh,
        surface,
        Layer::Overlay,
        Some("arcade-tools-overlay"),
        None,
    );
    layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
    layer.set_size(0, 0);
    layer.set_exclusive_zone(-1);
    layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
    layer.commit();
    let (tx, rx) = channel::<Command>();
    let tx = Mutex::new(tx);
    instance
        .serve(Arc::new(move |command| {
            if let Ok(tx) = tx.lock() {
                let _ = tx.send(command);
            }
        }))
        .map_err(|e| e.to_string())?;
    handle
        .insert_source(rx, |event, _, state: &mut Wl| {
            if let ChannelEvent::Msg(Command::Toggle | Command::Hide) = event {
                state.exit = true;
            }
        })
        .map_err(|e| e.to_string())?;
    overlay.timing.phase("wayland-init", began);
    let mut state = Wl {
        overlay,
        registry: RegistryState::new(&globals),
        seats: SeatState::new(&globals, &qh),
        outputs: OutputState::new(&globals, &qh),
        shm,
        pool,
        shown: Some(Shown {
            layer,
            viewport,
            fractional,
        }),
        keyboard: None,
        pointer: None,
        mods: Modifiers::default(),
        int_scale: 1,
        frac_scale: None,
        logical_size: (crate::layout::WIDTH, crate::layout::HEIGHT),
        configured: false,
        focus: false,
        exit: false,
        error: None,
        qh,
        handle,
        leave_timer: None,
        frame_requested: false,
    };
    while !state.exit {
        event_loop
            .dispatch(None, &mut state)
            .map_err(|e| e.to_string())?;
    }
    if let Some(shown) = state.shown.take() {
        shown
            .layer
            .set_keyboard_interactivity(KeyboardInteractivity::None);
        if let Some(viewport) = shown.viewport {
            viewport.destroy();
        }
        if let Some(fractional) = shown.fractional {
            fractional.destroy();
        }
        drop(shown.layer);
    }
    let _ = connection.flush();
    state.error.map_or(Ok(()), Err)
}

impl Wl {
    fn draw(&mut self) {
        if !self.configured || self.exit {
            return;
        }
        if let Err(error) = self.draw_frame() {
            self.error = Some(error);
            self.exit = true;
        }
    }

    fn draw_frame(&mut self) -> Result<(), String> {
        let fractional =
            self.shown.as_ref().is_some_and(|s| s.viewport.is_some()) && self.frac_scale.is_some();
        let scale = if fractional {
            self.frac_scale.unwrap() as f32 / 120.0
        } else {
            self.int_scale as f32
        };
        self.overlay.scale = scale;
        self.overlay.width = (self.logical_size.0 as f32 * scale).round() as u32;
        self.overlay.height = (self.logical_size.1 as f32 * scale).round() as u32;
        let pixmap = self.overlay.draw()?;
        let width = pixmap.width() as i32;
        let height = pixmap.height() as i32;
        let (buffer, canvas) = self
            .pool
            .create_buffer(width, height, width * 4, wl_shm::Format::Argb8888)
            .map_err(|e| e.to_string())?;
        for (dst, src) in canvas
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(pixmap.data().as_chunks::<4>().0.iter())
        {
            dst.copy_from_slice(&[src[2], src[1], src[0], src[3]]);
        }
        let shown = self.shown.as_ref().ok_or("layer surface closed")?;
        let surface = shown.layer.wl_surface();
        if fractional {
            surface.set_buffer_scale(1);
            shown
                .viewport
                .as_ref()
                .unwrap()
                .set_destination(self.logical_size.0 as i32, self.logical_size.1 as i32);
        } else {
            surface.set_buffer_scale(self.int_scale);
            if let Some(viewport) = &shown.viewport {
                viewport.set_destination(-1, -1);
            }
        }
        surface.damage_buffer(0, 0, width, height);
        if !self.frame_requested {
            surface.frame(&self.qh, surface.clone());
            self.frame_requested = true;
        }
        buffer.attach_to(surface).map_err(|e| e.to_string())?;
        shown.layer.commit();
        Ok(())
    }

    fn key(&mut self, event: KeyEvent) {
        let key = match event.keysym {
            Keysym::Escape => Some(Key::Escape),
            Keysym::Tab | Keysym::ISO_Left_Tab => Some(Key::Tab),
            Keysym::BackSpace => Some(if self.mods.ctrl {
                Key::Clear
            } else {
                Key::Backspace
            }),
            Keysym::Return | Keysym::KP_Enter => Some(Key::Enter),
            Keysym::Up | Keysym::Left => Some(Key::Up),
            Keysym::Down | Keysym::Right => Some(Key::Down),
            Keysym::Page_Up => Some(Key::PageUp),
            Keysym::Page_Down => Some(Key::PageDown),
            Keysym::Home => Some(Key::Home),
            Keysym::End => Some(Key::End),
            Keysym::p | Keysym::P if self.mods.ctrl => Some(Key::Picker),
            Keysym::u | Keysym::U if self.mods.ctrl => Some(Key::Clear),
            _ if !self.mods.ctrl && !self.mods.alt && !self.mods.logo => event.utf8.map(Key::Text),
            _ => None,
        };
        if let Some(key) = key {
            self.exit |= self.overlay.key(key, self.mods.shift);
            self.draw();
        }
    }
}

impl CompositorHandler for Wl {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        factor: i32,
    ) {
        self.int_scale = factor.max(1);
        self.draw();
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.overlay.presented("wayland-layer");
    }
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl LayerShellHandler for Wl {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.exit = true;
    }
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &LayerSurface,
        config: LayerSurfaceConfigure,
        _: u32,
    ) {
        if config.new_size.0 > 0 {
            self.logical_size.0 = config.new_size.0;
        }
        if config.new_size.1 > 0 {
            self.logical_size.1 = config.new_size.1;
        }
        self.configured = true;
        self.draw();
    }
}

impl OutputHandler for Wl {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl SeatHandler for Wl {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seats
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            self.keyboard = self
                .seats
                .get_keyboard_with_repeat(
                    qh,
                    &seat,
                    None,
                    self.handle.clone(),
                    Box::new(|state: &mut Wl, _, event| state.key(event)),
                )
                .ok();
        }
        if capability == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seats.get_pointer(qh, &seat).ok();
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard {
            if let Some(keyboard) = self.keyboard.take() {
                keyboard.release();
            }
        }
        if capability == Capability::Pointer {
            if let Some(pointer) = self.pointer.take() {
                pointer.release();
            }
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for Wl {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
        self.focus = true;
        if let Some(timer) = self.leave_timer.take() {
            self.handle.remove(timer);
        }
    }
    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
        if !self.focus {
            return;
        }
        self.focus = false;
        if let Some(timer) = self.leave_timer.take() {
            self.handle.remove(timer);
        }
        self.leave_timer = self
            .handle
            .insert_source(
                Timer::from_duration(Duration::from_millis(150)),
                |_, _, state: &mut Wl| {
                    state.leave_timer = None;
                    if !state.focus {
                        state.exit = true;
                    }
                    TimeoutAction::Drop
                },
            )
            .ok();
    }
    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.key(event);
    }
    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }
    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        mods: Modifiers,
        _: u32,
    ) {
        self.mods = mods;
    }
}

impl PointerHandler for Wl {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            match event.kind {
                PointerEventKind::Press { button: 0x110, .. } => {
                    self.exit |= self
                        .overlay
                        .click(event.position.0 as f32, event.position.1 as f32);
                    self.draw();
                }
                PointerEventKind::Axis { vertical, .. } => {
                    self.overlay.scroll(vertical.absolute as f32);
                    self.draw();
                }
                _ => {}
            }
        }
    }
}

impl ShmHandler for Wl {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}
impl ProvidesRegistryState for Wl {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState, SeatState];
}

impl Dispatch<wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1, ()> for Wl {
    fn event(
        _: &mut Self,
        _: &wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
        _: wp_fractional_scale_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl Dispatch<wp_fractional_scale_v1::WpFractionalScaleV1, ()> for Wl {
    fn event(
        state: &mut Self,
        _: &wp_fractional_scale_v1::WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            state.frac_scale = Some(scale.max(1));
            state.draw();
        }
    }
}
impl Dispatch<wp_viewporter::WpViewporter, ()> for Wl {
    fn event(
        _: &mut Self,
        _: &wp_viewporter::WpViewporter,
        _: wp_viewporter::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl Dispatch<wp_viewport::WpViewport, ()> for Wl {
    fn event(
        _: &mut Self,
        _: &wp_viewport::WpViewport,
        _: wp_viewport::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

delegate_compositor!(Wl);
delegate_output!(Wl);
delegate_seat!(Wl);
delegate_keyboard!(Wl);
delegate_pointer!(Wl);
delegate_layer!(Wl);
delegate_shm!(Wl);
delegate_registry!(Wl);

//! Isolated test pointer adapted from Arcade Find's `examples/e2e-pointer.rs`
//! (MIT OR Apache-2.0). Never run against the user's desktop.

#[cfg(not(target_os = "linux"))]
fn main() {}

#[cfg(target_os = "linux")]
fn main() {
    use wayland_client::globals::{registry_queue_init, GlobalListContents};
    use wayland_client::protocol::{wl_pointer::ButtonState, wl_registry, wl_seat};
    use wayland_client::{Connection, Dispatch, QueueHandle};
    use wayland_protocols_wlr::virtual_pointer::v1::client::{
        zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1 as Manager,
        zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1 as Pointer,
    };

    assert_eq!(
        std::env::var("WLR_BACKENDS").as_deref(),
        Ok("headless"),
        "only an isolated headless compositor is supported"
    );
    struct State;
    impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
        fn event(
            _: &mut Self,
            _: &wl_registry::WlRegistry,
            _: wl_registry::Event,
            _: &GlobalListContents,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }
    impl Dispatch<wl_seat::WlSeat, ()> for State {
        fn event(
            _: &mut Self,
            _: &wl_seat::WlSeat,
            _: wl_seat::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }
    impl Dispatch<Manager, ()> for State {
        fn event(
            _: &mut Self,
            _: &Manager,
            _: <Manager as wayland_client::Proxy>::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }
    impl Dispatch<Pointer, ()> for State {
        fn event(
            _: &mut Self,
            _: &Pointer,
            _: <Pointer as wayland_client::Proxy>::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }
    let connection = Connection::connect_to_env().expect("isolated Wayland display");
    let (globals, mut queue) = registry_queue_init::<State>(&connection).expect("registry");
    let qh = queue.handle();
    let seat: wl_seat::WlSeat = globals.bind(&qh, 1..=7, ()).expect("seat");
    let manager: Manager = globals
        .bind(&qh, 1..=2, ())
        .expect("virtual pointer protocol");
    let pointer = manager.create_virtual_pointer(Some(&seat), &qh, ());
    // Outside the centered panel at either 100% or 150% scale.
    pointer.motion_absolute(0, 4, 4, 1920, 1080);
    pointer.frame();
    queue.roundtrip(&mut State).expect("pointer motion");
    std::thread::sleep(std::time::Duration::from_millis(30));
    pointer.button(30, 0x110, ButtonState::Pressed);
    pointer.frame();
    pointer.button(31, 0x110, ButtonState::Released);
    pointer.frame();
    queue.roundtrip(&mut State).expect("outside click");
}

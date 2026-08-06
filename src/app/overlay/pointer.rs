//! Where the pointer is on the whole screen — which winit does not tell us.
//!
//! The overlay wants to be click-through everywhere except its own toolbar. On
//! Wayland the compositor is told exactly that once (an input region, see
//! [`super::layer_shell`]) and decides every click itself. An ordinary window
//! has no such thing: winit offers `set_cursor_hittest`, and egui
//! `ViewportCommand::MousePassthrough`, both of which are all-or-nothing.
//!
//! So the overlay flips that one switch itself, based on where the pointer is —
//! and to do that it has to know, including while it is click-through and
//! therefore receiving no pointer events at all. Hence one system call per
//! platform.
//!
//! Where there is no implementation the answer is `None`, and the overlay falls
//! back to being click-through unless it is being moved — the behaviour it had
//! before any of this.

use eframe::egui::{Pos2, pos2};

/// The pointer's position in physical screen pixels, or `None` where the
/// platform has no implementation here.
pub fn on_screen() -> Option<Pos2> {
    platform::on_screen()
}

#[cfg(windows)]
mod platform {
    use super::*;
    use windows_sys::Win32::{Foundation::POINT, UI::WindowsAndMessaging::GetCursorPos};

    pub fn on_screen() -> Option<Pos2> {
        let mut point = POINT { x: 0, y: 0 };
        // Safe: `GetCursorPos` only writes the POINT we hand it, and reports
        // failure through its return value rather than the buffer.
        let ok = unsafe { GetCursorPos(&mut point) };
        (ok != 0).then(|| pos2(point.x as f32, point.y as f32))
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use std::sync::OnceLock;
    use x11rb::{connection::Connection, protocol::xproto::ConnectionExt, rust_connection::RustConnection};

    /// The X connection and the root window to ask about the pointer. Opened
    /// once: a connection per frame would be an absurd price for one query, and
    /// in a Wayland session there is nothing to connect to, which is stored as
    /// `None` so it is not retried every frame either.
    static X11: OnceLock<Option<(RustConnection, u32)>> = OnceLock::new();

    pub fn on_screen() -> Option<Pos2> {
        let (connection, root) = X11
            .get_or_init(|| {
                let (connection, screen) = x11rb::connect(None).ok()?;
                let root = connection.setup().roots.get(screen)?.root;
                Some((connection, root))
            })
            .as_ref()?;
        let pointer = connection.query_pointer(*root).ok()?.reply().ok()?;
        Some(pos2(pointer.root_x as f32, pointer.root_y as f32))
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod platform {
    use super::*;

    pub fn on_screen() -> Option<Pos2> {
        None
    }
}

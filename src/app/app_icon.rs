//! The application icon, in the two shapes the app needs it: the file the
//! desktop installer writes, and the window icon eframe is handed at startup.
//!
//! On a Wayland desktop the frame around the window is drawn by the compositor,
//! which takes its icon from the desktop entry rather than from here — see
//! `desktop_install`.
//!
//! One copy of the bytes lives here; see `icon/build.py` for how the file itself
//! is drawn.

use std::io::Cursor;

use eframe::egui::IconData;

/// The icon as it ships, written verbatim by the desktop installer.
pub const PNG: &[u8] = include_bytes!("../../icon/icon.png");

/// The icon as raw RGBA, with its size.
fn decode() -> (Vec<u8>, usize, usize) {
    let mut reader = png::Decoder::new(Cursor::new(PNG))
        .read_info()
        .expect("the icon ships with the binary and is a valid png");
    let mut data = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut data).unwrap();
    assert_eq!(info.color_type, png::ColorType::Rgba);
    (data, info.width as usize, info.height as usize)
}

/// The window and taskbar icon, handed to eframe before the window is built.
pub fn window_icon() -> IconData {
    let (rgba, width, height) = decode();
    IconData {
        rgba,
        width: width as u32,
        height: height as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icon_decodes_to_a_square_rgba_image() {
        let (rgba, width, height) = decode();
        assert_eq!(width, height, "the icon is square");
        assert_eq!(rgba.len(), width * height * 4, "four channels per pixel");
    }
}

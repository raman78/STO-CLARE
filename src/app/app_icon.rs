//! The application icon, in the three shapes the app needs it: the file the
//! desktop installer writes, the window icon eframe is handed at startup, and a
//! texture for the app's own toolbar.
//!
//! One copy of the bytes lives here; see `icon/build.py` for how the file itself
//! is drawn.

use std::io::Cursor;

use eframe::egui::{Color32, ColorImage, Context, IconData, TextureHandle, TextureOptions};

/// The icon as it ships, written verbatim by the desktop installer.
pub const PNG: &[u8] = include_bytes!("../../icon/icon.png");

/// Side of the texture the toolbar draws from.
///
/// The icon is 512px and the bar draws it about twenty points tall. Sampling
/// straight from 512px comes out ragged — mipmaps would fix it, but epaint only
/// has them on the glow backend and this app renders through wgpu — so the
/// image is box-filtered down to something close to its drawn size first.
const TEXTURE_SIDE: usize = 64;

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

/// The icon as a texture, for the app to draw in its own toolbar. Upload it
/// once and keep the handle; every call re-uploads.
pub fn texture(ctx: &Context) -> TextureHandle {
    let (rgba, width, height) = decode();
    ctx.load_texture(
        "app-icon",
        downscale(&rgba, width, height, TEXTURE_SIDE),
        TextureOptions::LINEAR,
    )
}

/// Average each block of source pixels into one target pixel.
///
/// Averaging happens with the colours premultiplied by their alpha, so the
/// transparent corners of the tile do not bleed their colour into the edge as
/// they would if the four channels were averaged on their own.
fn downscale(rgba: &[u8], width: usize, height: usize, side: usize) -> ColorImage {
    let mut pixels = Vec::with_capacity(side * side);
    for y in 0..side {
        let y0 = y * height / side;
        let y1 = ((y + 1) * height / side).max(y0 + 1);
        for x in 0..side {
            let x0 = x * width / side;
            let x1 = ((x + 1) * width / side).max(x0 + 1);

            let (mut r, mut g, mut b, mut a) = (0u32, 0u32, 0u32, 0u32);
            for source_y in y0..y1 {
                for source_x in x0..x1 {
                    let i = (source_y * width + source_x) * 4;
                    let alpha = rgba[i + 3] as u32;
                    r += rgba[i] as u32 * alpha / 255;
                    g += rgba[i + 1] as u32 * alpha / 255;
                    b += rgba[i + 2] as u32 * alpha / 255;
                    a += alpha;
                }
            }

            let count = ((x1 - x0) * (y1 - y0)) as u32;
            pixels.push(Color32::from_rgba_premultiplied(
                (r / count) as u8,
                (g / count) as u8,
                (b / count) as u8,
                (a / count) as u8,
            ));
        }
    }

    ColorImage::new([side, side], pixels)
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

    #[test]
    fn downscaling_keeps_the_corners_transparent_and_the_middle_opaque() {
        let (rgba, width, height) = decode();
        let small = downscale(&rgba, width, height, TEXTURE_SIDE);
        assert_eq!([TEXTURE_SIDE, TEXTURE_SIDE], small.size);

        let at = |x: usize, y: usize| small.pixels[y * TEXTURE_SIDE + x];
        assert_eq!(0, at(0, 0).a(), "the rounded corner stays cut out");
        assert_eq!(
            255,
            at(TEXTURE_SIDE / 2, TEXTURE_SIDE / 2).a(),
            "the middle of the tile stays solid"
        );
    }

    /// Averaging premultiplied is what keeps the edge of the tile from picking
    /// up the colour of the fully transparent pixels beside it.
    #[test]
    fn a_transparent_neighbour_does_not_tint_the_edge() {
        // Two pixels: opaque blue, and transparent white.
        let rgba = [0, 0, 255, 255, 255, 255, 255, 0];
        let one = downscale(&rgba, 2, 1, 1);
        let pixel = one.pixels[0];
        assert_eq!(127, pixel.a(), "half of the block was transparent");
        assert_eq!(0, pixel.r(), "no white bled in");
        assert_eq!(0, pixel.g(), "no white bled in");
    }
}

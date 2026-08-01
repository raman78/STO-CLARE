//! The bold face used to set a few values apart from the ones next to them.
//!
//! egui's `RichText::strong()` only picks a brighter color — the bundled fonts
//! have no bold face at all (epaint ships Ubuntu-Light, Hack and two emoji
//! fonts). Anything that has to read as *bold* therefore needs a face of its
//! own, installed as a named family beside the defaults.

use std::sync::Arc;

use eframe::egui::*;

/// Name of both the font data entry and the family it is bound to.
const BOLD: &str = "Ubuntu-Bold";

/// Ubuntu Bold, the matching weight of the Ubuntu-Light epaint uses for
/// [`FontFamily::Proportional`], so bold text has the same shapes as the text
/// around it. Licensed under the Ubuntu Font Licence (`assets/fonts/UFL.txt`).
const UBUNTU_BOLD: &[u8] = include_bytes!("../../assets/fonts/Ubuntu-Bold.ttf");

/// The family to ask for bold text: `RichText::new(..).family(bold_family())`.
/// Only bound on contexts that went through [`install`].
pub fn bold_family() -> FontFamily {
    FontFamily::Name(BOLD.into())
}

/// Binds [`bold_family`] on `ctx`. Must run before anything asks for it —
/// epaint panics on a family it does not know.
pub fn install(ctx: &Context) {
    ctx.set_fonts(definitions());
}

/// The default fonts plus the bold family: the bold face first, then the
/// regular proportional list, so glyphs the bold face lacks (the arrows and
/// emoji of the default fonts) still render.
fn definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        BOLD.to_owned(),
        Arc::new(FontData::from_static(UBUNTU_BOLD)),
    );

    let mut family = vec![BOLD.to_owned()];
    family.extend(
        fonts
            .families
            .get(&FontFamily::Proportional)
            .cloned()
            .unwrap_or_default(),
    );
    fonts.families.insert(bold_family(), family);

    fonts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bold_family_is_bound_to_the_bold_face_first() {
        let fonts = definitions();
        let family = fonts.families.get(&bold_family()).unwrap();
        assert_eq!(family.first().unwrap(), BOLD);
        assert!(fonts.font_data.contains_key(BOLD));
    }

    #[test]
    fn bold_family_falls_back_to_the_default_proportional_fonts() {
        let fonts = definitions();
        let family = fonts.families.get(&bold_family()).unwrap();
        assert!(family.iter().any(|font| font == "Ubuntu-Light"));
    }

    /// The face really is heavier than the regular one, which is the whole
    /// point: laid out at the same size, the same digits come out wider.
    #[test]
    fn bold_digits_are_wider_than_regular_ones() {
        let ctx = Context::default();
        install(&ctx);
        // The fonts only exist once a pass has run.
        let _ = ctx.run_ui(Default::default(), |_| {});

        let width = |family: FontFamily| {
            ctx.fonts_mut(|fonts| {
                fonts
                    .layout_no_wrap(
                        "123456789".to_owned(),
                        FontId::new(14.0, family),
                        Color32::WHITE,
                    )
                    .size()
                    .x
            })
        };

        assert!(width(bold_family()) > width(FontFamily::Proportional));
    }
}

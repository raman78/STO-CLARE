//! One place for how the app looks.
//!
//! Three things live here and nowhere else:
//!
//! 1. **The themes on offer** — [`Theme`] and the [`THEMES`] registry. Adding one
//!    is a variant and a registry entry, both in this file; the settings tab
//!    builds its list from the registry, so nothing else has to be touched.
//! 2. **The colours the app paints itself** — [`Palette`]. egui's own widget
//!    colours come from [`egui::Visuals`], but the deltas in the compare table,
//!    the warning marks, the status icons and the chart series are ours, and
//!    each theme carries its own set.
//! 3. **The text sizes** — [`TEXT_SIZES`], spelled out rather than inherited, so
//!    changing how big the app writes is one table.
//!
//! [`apply`] is the only way any of it reaches the screen.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use eframe::{
    egui::{
        Color32, Context, CornerRadius, FontFamily, FontId, Stroke, Style, TextStyle, Visuals,
        style::Selection,
    },
    epaint::{Rgba, Shadow},
};
use serde::{Deserialize, Serialize};

/// Which look the app is set to. Stored in the settings file by variant name,
/// so a variant may be added but not renamed.
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Theme {
    Dark,
    #[default]
    LightDark,
    Light,
    Nebula,
    FrostLight,
}

/// The colours the app itself paints, on top of egui's widget colours.
pub struct Palette {
    /// A metric that moved in the better direction.
    pub improve: Color32,
    /// A metric that moved in the worse direction.
    pub worse: Color32,
    /// Something the user should look at, but which is not an error.
    pub warn: Color32,
    /// Finished, working, failed — the status and upload marks.
    pub ok: Color32,
    pub busy: Color32,
    pub error: Color32,
    /// Chart series, taken in order. See [`series_color`].
    pub series: &'static [Color32],
    /// The same, for a reader with colour-vision deficiency. Picked to stay
    /// apart under protanopia and deuteranopia across the *whole* set rather
    /// than only between neighbours — see [`set_color_blind_series`].
    pub color_blind_series: &'static [Color32],
}

/// One entry of the registry: everything a theme is.
pub struct ThemeEntry {
    pub theme: Theme,
    /// What the settings tab calls it.
    pub name: &'static str,
    visuals: fn() -> Visuals,
    pub palette: &'static Palette,
}

/// Every theme the app offers, in the order the settings tab lists them.
pub const THEMES: &[ThemeEntry] = &[
    ThemeEntry {
        theme: Theme::Dark,
        name: "Dark",
        visuals: dark_visuals,
        palette: &DARK_PALETTE,
    },
    ThemeEntry {
        theme: Theme::LightDark,
        name: "Light Dark",
        visuals: light_dark_visuals,
        palette: &DARK_PALETTE,
    },
    ThemeEntry {
        theme: Theme::Light,
        name: "Light",
        visuals: light_visuals,
        palette: &LIGHT_PALETTE,
    },
    ThemeEntry {
        theme: Theme::Nebula,
        name: "Nebula",
        visuals: nebula_visuals,
        palette: &DARK_PALETTE,
    },
    ThemeEntry {
        theme: Theme::FrostLight,
        name: "Frost Light",
        visuals: frost_light_visuals,
        palette: &LIGHT_PALETTE,
    },
];

/// Text sizes, in the order they read from smallest to largest. Written out
/// rather than left to egui so there is one place to change them; these are the
/// sizes the app has always used.
const TEXT_SIZES: &[(TextStyle, f32, bool)] = &[
    // (style, size, monospace)
    (TextStyle::Small, 9.0, false),
    (TextStyle::Body, 13.0, false),
    (TextStyle::Button, 13.0, false),
    (TextStyle::Monospace, 13.0, true),
    (TextStyle::Heading, 18.0, false),
];

/// Eight hues for chart series, stepped for a dark chart surface.
///
/// Validated as a set: every hue sits in the dark lightness band, clears the
/// chroma floor, and neighbouring hues stay apart under protanopia,
/// deuteranopia and tritanopia as well as normal vision. Series are also named
/// in the legend and on hover, so colour is never the only thing telling two
/// apart — which is what makes the pair that sits below 3:1 against the very
/// light "Light Dark" surface acceptable.
const DARK_SERIES: &[Color32] = &[
    Color32::from_rgb(0x39, 0x87, 0xe5), // blue
    Color32::from_rgb(0xd9, 0x59, 0x26), // orange
    Color32::from_rgb(0x19, 0x9e, 0x70), // aqua
    Color32::from_rgb(0xc9, 0x85, 0x00), // yellow
    Color32::from_rgb(0xd5, 0x51, 0x81), // magenta
    Color32::from_rgb(0x00, 0x83, 0x00), // green
    Color32::from_rgb(0x90, 0x85, 0xe9), // violet
    Color32::from_rgb(0xe6, 0x67, 0x67), // red
];

/// The same eight hues, stepped for a light chart surface.
const LIGHT_SERIES: &[Color32] = &[
    Color32::from_rgb(0x2a, 0x78, 0xd6), // blue
    Color32::from_rgb(0xeb, 0x68, 0x34), // orange
    Color32::from_rgb(0x1b, 0xaf, 0x7a), // aqua
    Color32::from_rgb(0xed, 0xa1, 0x00), // yellow
    Color32::from_rgb(0xe8, 0x7b, 0xa4), // magenta
    Color32::from_rgb(0x00, 0x83, 0x00), // green
    Color32::from_rgb(0x4a, 0x3a, 0xa7), // violet
    Color32::from_rgb(0xe3, 0x49, 0x48), // red
];

/// Eight series for a dark chart surface that stay apart for a reader with
/// colour-vision deficiency, offered instead of [`DARK_SERIES`] when the
/// setting is on.
///
/// Seven of them are the Okabe–Ito set, which was designed and published for
/// exactly this and is the one most widely checked against real viewers; only
/// two changes were needed for this app:
///
/// - its blue is lightened from L\*46 to L\*56, because at its published value
///   it sits at 2.0:1 against the lightest of our dark surfaces (the "Light
///   Dark" chart plate) — below anything else the app draws there;
/// - its eighth colour is black, which is invisible on a dark plate, so a light
///   neutral takes that slot. It is not what sets the floor below, so the swap
///   costs no separation.
///
/// Where the ordinary palette leans on hue, this one leans on **lightness**:
/// that is the axis a dichromat keeps, so the set spans L\*54 to L\*89 instead
/// of the ordinary palette's narrow band.
const CVD_DARK_SERIES: &[Color32] = &[
    Color32::from_rgb(0xe6, 0x9f, 0x00), // orange
    Color32::from_rgb(0x56, 0xb4, 0xe9), // sky blue
    Color32::from_rgb(0x00, 0x9e, 0x73), // bluish green
    Color32::from_rgb(0xf0, 0xe4, 0x42), // yellow
    Color32::from_rgb(0x3a, 0x8b, 0xce), // blue, lightened for the plate
    Color32::from_rgb(0xd5, 0x5e, 0x00), // vermillion
    Color32::from_rgb(0xcc, 0x79, 0xa7), // reddish purple
    Color32::from_rgb(0xdc, 0xdc, 0xdc), // light neutral, in place of black
];

/// The same, for a light chart surface.
///
/// Okabe–Ito again, with its yellow replaced rather than darkened: on white
/// that yellow is 1.3:1, and darkening it far enough to be seen walks it into
/// the orange, since for a dichromat the two differ only in lightness. A dark
/// red takes the slot instead — well clear of everything else, on the surface
/// and under simulation alike — and the set keeps black, which a white plate
/// makes the strongest colour of the eight.
const CVD_LIGHT_SERIES: &[Color32] = &[
    Color32::from_rgb(0xe6, 0x9f, 0x00), // orange
    Color32::from_rgb(0x56, 0xb4, 0xe9), // sky blue
    Color32::from_rgb(0x00, 0x9e, 0x73), // bluish green
    Color32::from_rgb(0x78, 0x26, 0x2f), // dark red, in place of the yellow
    Color32::from_rgb(0x00, 0x72, 0xb2), // blue
    Color32::from_rgb(0xd5, 0x5e, 0x00), // vermillion
    Color32::from_rgb(0xcc, 0x79, 0xa7), // reddish purple
    Color32::from_rgb(0x00, 0x00, 0x00), // black
];

const DARK_PALETTE: Palette = Palette {
    improve: Color32::from_rgb(0x5c, 0xb8, 0x5c),
    worse: Color32::from_rgb(0xd9, 0x53, 0x4f),
    warn: Color32::from_rgb(0xd9, 0x95, 0x00),
    ok: Color32::from_rgb(0x5c, 0xb8, 0x5c),
    busy: Color32::from_rgb(0xd9, 0x95, 0x00),
    error: Color32::from_rgb(0xd9, 0x53, 0x4f),
    series: DARK_SERIES,
    color_blind_series: CVD_DARK_SERIES,
};

const LIGHT_PALETTE: Palette = Palette {
    // The same roles, darkened so they read on a light background.
    improve: Color32::from_rgb(0x2f, 0x7d, 0x32),
    worse: Color32::from_rgb(0xc0, 0x28, 0x24),
    warn: Color32::from_rgb(0xa8, 0x6e, 0x00),
    ok: Color32::from_rgb(0x2f, 0x7d, 0x32),
    busy: Color32::from_rgb(0xa8, 0x6e, 0x00),
    error: Color32::from_rgb(0xc0, 0x28, 0x24),
    series: LIGHT_SERIES,
    color_blind_series: CVD_LIGHT_SERIES,
};

/// Index into [`THEMES`] of the theme in use. The look is one per process — the
/// overlay runs its own egui context and shares it — so it lives here rather
/// than being carried to every call site that wants a colour.
static ACTIVE: AtomicUsize = AtomicUsize::new(0);

/// Whether charts take their series from [`Palette::color_blind_series`]. Kept
/// beside [`ACTIVE`] and for the same reason: the overlay's own egui context
/// draws from these colours too.
static COLOR_BLIND: AtomicBool = AtomicBool::new(false);

/// Put `theme` on screen: egui's widget colours, our own colours, the text
/// sizes and the few style tweaks the app relies on.
pub fn apply(ctx: &Context, theme: Theme) {
    let entry = entry(theme);
    ACTIVE.store(index_of(theme), Ordering::Relaxed);

    let mut style = Style::clone(&ctx.global_style());
    style.visuals = (entry.visuals)();
    style.text_styles = TEXT_SIZES
        .iter()
        .map(|&(ref style, size, monospace)| {
            let family = if monospace {
                FontFamily::Monospace
            } else {
                FontFamily::Proportional
            };
            (style.clone(), FontId::new(size, family))
        })
        .collect();
    // Labels are read, not selected: dragging across a table would otherwise
    // start a text selection instead of doing nothing.
    style.interaction.selectable_labels = false;
    style.interaction.tooltip_delay = 0.0;
    // Floating scroll bars grow when the pointer comes near them and would then
    // be drawn on top of the content — the horizontal bar of a table covering
    // its last row. Reserve a strip as wide as the fully grown bar, so it sits
    // next to the content instead of over it. The strip is only taken while the
    // bar is actually shown.
    style.spacing.scroll.floating_allocated_width = style.spacing.scroll.bar_width;

    // The app picks its own theme, so both of egui's slots get the same style;
    // following the desktop's light/dark preference would override the choice.
    ctx.set_style_of(eframe::egui::Theme::Dark, style.clone());
    ctx.set_style_of(eframe::egui::Theme::Light, style);
}

/// The colours of the theme in use.
pub fn palette() -> &'static Palette {
    THEMES[ACTIVE.load(Ordering::Relaxed)].palette
}

/// Colour of the `index`-th series of a chart, counted in draw order.
///
/// Past the eight hues of the palette the order starts again: the number of
/// series here is up to the user — every ability they tick is one — so there is
/// no fixed set to design for. Every chart names its series in the legend and
/// on hover, which is what keeps them apart when a hue comes round twice.
pub fn series_color(index: usize) -> Color32 {
    let series = series();
    series[index % series.len()]
}

/// The series colours in force: the theme's own, or its colour-blind set when
/// [`set_color_blind_series`] has switched them on.
pub fn series() -> &'static [Color32] {
    let palette = palette();
    if COLOR_BLIND.load(Ordering::Relaxed) {
        palette.color_blind_series
    } else {
        palette.series
    }
}

/// Switch the charts between the theme's ordinary series colours and its
/// colour-blind set.
///
/// A setting rather than a theme of its own: which colours read apart is about
/// the eyes doing the reading, not about whether the app is dark or light, and
/// making it a theme would have meant one colour-blind copy of every theme
/// there is.
///
/// Only the chart series move. The deltas in the compare table and the status
/// marks keep their green and red, because those never stand on colour alone —
/// a delta carries its `+`/`-` sign and a status mark its word.
pub fn set_color_blind_series(on: bool) {
    COLOR_BLIND.store(on, Ordering::Relaxed);
}

fn index_of(theme: Theme) -> usize {
    THEMES.iter().position(|t| t.theme == theme).unwrap_or(0)
}

fn entry(theme: Theme) -> &'static ThemeEntry {
    &THEMES[index_of(theme)]
}

impl Theme {
    /// What the settings tab calls this theme.
    pub fn display(&self) -> &'static str {
        entry(*self).name
    }
}

/// egui's own dark theme, in the app's material. Colours untouched.
fn dark_visuals() -> Visuals {
    glassify(
        Visuals::dark(),
        Glass {
            rim: Color32::WHITE,
            strength: 1.0,
        },
    )
}

/// egui's own light theme, in the app's material. Colours untouched; the rim is
/// dark and eased off, because a white hairline on a white page is nothing.
fn light_visuals() -> Visuals {
    glassify(
        Visuals::light(),
        Glass {
            rim: Color32::from_rgb(0x20, 0x20, 0x28),
            strength: 0.55,
        },
    )
}

/// A dark theme with more contrast between the surfaces than egui's own, which
/// is what the app opens with.
fn light_dark_visuals() -> Visuals {
    let background = Rgba::from_rgb(0.08, 0.08, 0.08).into();
    let darker_background = Rgba::from_rgb(0.05, 0.05, 0.05).into();
    let brighter_background = Rgba::from_rgb(0.15, 0.15, 0.15).into();
    let mut theme = Visuals::dark();
    theme.code_bg_color = background;
    theme.error_fg_color = Rgba::from_rgb(0.8, 0.3, 0.3).into();
    theme.extreme_bg_color = darker_background;
    theme.faint_bg_color = brighter_background;
    theme.hyperlink_color = Rgba::from_rgb(0.2, 0.2, 0.9).into();
    theme.panel_fill = background;
    theme.warn_fg_color = Rgba::from_rgb(0.8, 0.7, 0.3).into();
    theme.selection = Selection {
        bg_fill: Rgba::from_rgb(0.2, 0.2, 0.7).into(),
        ..Default::default()
    };
    theme.popup_shadow = Shadow::NONE;

    theme.widgets.inactive.bg_fill = Rgba::from_rgb(0.2, 0.2, 0.2).into();
    theme.widgets.hovered.bg_fill = Rgba::from_rgb(0.25, 0.25, 0.25).into();
    theme.widgets.active.bg_fill = Rgba::from_rgb(0.3, 0.3, 0.3).into();

    theme.widgets.noninteractive.fg_stroke.color = Rgba::from_rgb(0.92, 0.92, 0.92).into();
    theme.widgets.inactive.fg_stroke.color = Rgba::from_rgb(0.92, 0.92, 0.92).into();

    theme.window_fill = background;
    theme.window_stroke.color = Rgba::from_rgb(0.9, 0.9, 0.9).into();
    theme.window_shadow = Shadow::NONE;
    // The colours above are the ones this theme has always had — the fills that
    // set a drop-down apart from the page included. `glassify` only rounds and
    // rims them.
    glassify(
        theme,
        Glass {
            rim: Color32::WHITE,
            strength: 1.0,
        },
    )
}

/// What tells one theme's glass apart from another's. Everything else about the
/// material — the radii, the shadows — is the same for every theme, and lives
/// in [`glassify`].
struct Glass {
    /// The hairline along the edge of a frame: what reads as the rim of a piece
    /// of glass catching the light.
    rim: Color32,
    /// Scales every rim's alpha. A dark surface takes a full-strength rim; on a
    /// light one the same alphas draw hard outlines, so they are eased off
    /// rather than re-tuned one by one.
    strength: f32,
}

impl Glass {
    /// The rim at `alpha`, after [`Glass::strength`].
    fn edge(&self, alpha: u8) -> Color32 {
        self.tint(self.rim, alpha)
    }

    fn tint(&self, color: Color32, alpha: u8) -> Color32 {
        let alpha = (alpha as f32 * self.strength).round().clamp(1.0, 255.0) as u8;
        Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
    }
}

/// Give a flat set of visuals the glass treatment: rounded corners, widget
/// fills that let the surface below show through, a lit rim on every frame, and
/// shadows switched back on so a popup reads as floating above the page rather
/// than cut into it.
///
/// Every theme in the registry goes through here, so the app is made of one
/// material throughout and the themes differ only in colour.
///
/// This is deliberately only the *shape* of things. Fills — `bg_fill`,
/// `weak_bg_fill`, `faint_bg_color` — are left exactly as the theme set them,
/// because that is what separates a drop-down or a table row from the page
/// behind it. Replacing them with a translucent pane looks like glass and reads
/// like fog: the field stops standing out from the background. Rounding,
/// rimming and shadowing cost nothing in contrast, so those are what a theme
/// gets here.
fn glassify(mut visuals: Visuals, glass: Glass) -> Visuals {
    visuals.window_corner_radius = CornerRadius::same(10);
    visuals.menu_corner_radius = CornerRadius::same(8);

    // A rim that firms up as the pointer approaches, so the response to it is
    // the edge lighting up rather than the fill washing out.
    let widgets = [
        (&mut visuals.widgets.noninteractive, 14u8),
        (&mut visuals.widgets.inactive, 26),
        (&mut visuals.widgets.hovered, 60),
        (&mut visuals.widgets.active, 90),
        (&mut visuals.widgets.open, 70),
    ];
    for (widget, rim) in widgets {
        widget.bg_stroke = Stroke::new(1.0, glass.edge(rim));
        // 4, not more: a checkbox is about 14 points across and shares this
        // radius, so anything rounder turns it into a radio button.
        widget.corner_radius = CornerRadius::same(4);
    }
    // The pressed state is the one moment the theme's own accent takes over the
    // rim, so a click lands somewhere visible instead of a shade lighter. The
    // accent is the theme's link colour — the one colour every theme already
    // defines as "bright enough to stand out against my background".
    let accent = visuals.hyperlink_color;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, glass.tint(accent, 200));

    visuals.window_stroke = Stroke::new(1.0, glass.edge(60));

    // Straight down and wide: a shadow that reads as height, not as an object
    // lying next to its own silhouette.
    visuals.window_shadow = Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(120),
    };
    visuals.popup_shadow = Shadow {
        offset: [0, 6],
        blur: 16,
        spread: 0,
        color: Color32::from_black_alpha(96),
    };
    visuals
}

/// Deep space instead of neutral grey: the surfaces carry a blue cast and the
/// accent is the cyan of a shield facing.
///
/// The fills are stepped to match the separation "Light Dark" has between a
/// field and the page behind it, so a drop-down still reads as a drop-down.
fn nebula_visuals() -> Visuals {
    let mut base = Visuals::dark();
    base.panel_fill = Color32::from_rgb(0x0b, 0x0e, 0x14);
    base.window_fill = Color32::from_rgb(0x0e, 0x12, 0x1a);
    base.extreme_bg_color = Color32::from_rgb(0x07, 0x09, 0x10);
    base.code_bg_color = Color32::from_rgb(0x11, 0x16, 0x20);
    base.faint_bg_color = Color32::from_rgb(0x18, 0x1f, 0x2c);
    base.warn_fg_color = Color32::from_rgb(0xf0, 0xa0, 0x30);
    base.error_fg_color = Color32::from_rgb(0xe6, 0x67, 0x67);
    base.hyperlink_color = Color32::from_rgb(0x3f, 0xc7, 0xe0);
    base.selection = Selection {
        bg_fill: Color32::from_rgb(0x1b, 0x5c, 0x74),
        stroke: Stroke::new(1.0, Color32::from_rgb(0x9f, 0xe4, 0xf2)),
    };

    base.widgets.noninteractive.bg_fill = Color32::from_rgb(0x16, 0x1d, 0x2a);
    base.widgets.noninteractive.weak_bg_fill = Color32::from_rgb(0x12, 0x18, 0x23);
    base.widgets.inactive.bg_fill = Color32::from_rgb(0x25, 0x30, 0x44);
    base.widgets.inactive.weak_bg_fill = Color32::from_rgb(0x1c, 0x25, 0x34);
    base.widgets.hovered.bg_fill = Color32::from_rgb(0x31, 0x3f, 0x58);
    base.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x27, 0x33, 0x49);
    base.widgets.active.bg_fill = Color32::from_rgb(0x3e, 0x50, 0x70);
    base.widgets.active.weak_bg_fill = Color32::from_rgb(0x35, 0x45, 0x60);
    base.widgets.open.bg_fill = Color32::from_rgb(0x31, 0x3f, 0x58);
    base.widgets.open.weak_bg_fill = Color32::from_rgb(0x27, 0x33, 0x49);

    base.widgets.noninteractive.fg_stroke.color = Color32::from_rgb(0xd8, 0xe2, 0xf0);
    base.widgets.inactive.fg_stroke.color = Color32::from_rgb(0xd8, 0xe2, 0xf0);
    glassify(
        base,
        Glass {
            rim: Color32::from_rgb(0x7f, 0xc8, 0xe8),
            strength: 1.0,
        },
    )
}

/// The same material for daylight: a cool white page with fields a clear step
/// darker than it, rather than the barely-there greys of egui's own light theme.
fn frost_light_visuals() -> Visuals {
    let mut base = Visuals::light();
    base.panel_fill = Color32::from_rgb(0xf2, 0xf5, 0xf9);
    base.window_fill = Color32::from_rgb(0xf7, 0xf9, 0xfc);
    base.extreme_bg_color = Color32::from_rgb(0xff, 0xff, 0xff);
    base.faint_bg_color = Color32::from_rgb(0xe4, 0xea, 0xf3);
    base.hyperlink_color = Color32::from_rgb(0x0a, 0x6f, 0xd0);
    base.selection = Selection {
        bg_fill: Color32::from_rgb(0xb7, 0xd6, 0xf5),
        stroke: Stroke::new(1.0, Color32::from_rgb(0x0a, 0x4a, 0x8a)),
    };

    base.widgets.noninteractive.bg_fill = Color32::from_rgb(0xe7, 0xec, 0xf3);
    base.widgets.noninteractive.weak_bg_fill = Color32::from_rgb(0xed, 0xf1, 0xf7);
    base.widgets.inactive.bg_fill = Color32::from_rgb(0xd8, 0xe1, 0xed);
    base.widgets.inactive.weak_bg_fill = Color32::from_rgb(0xe2, 0xe9, 0xf2);
    base.widgets.hovered.bg_fill = Color32::from_rgb(0xc6, 0xd4, 0xe6);
    base.widgets.hovered.weak_bg_fill = Color32::from_rgb(0xd0, 0xdc, 0xeb);
    base.widgets.active.bg_fill = Color32::from_rgb(0xb2, 0xc6, 0xde);
    base.widgets.active.weak_bg_fill = Color32::from_rgb(0xbd, 0xce, 0xe3);
    base.widgets.open.bg_fill = Color32::from_rgb(0xc6, 0xd4, 0xe6);
    base.widgets.open.weak_bg_fill = Color32::from_rgb(0xd0, 0xdc, 0xeb);
    glassify(
        base,
        Glass {
            rim: Color32::from_rgb(0x1e, 0x3a, 0x5a),
            strength: 0.55,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Which theme is active is one value for the whole process, so the tests
    /// that set it take turns instead of running over each other.
    static ACTIVE_THEME: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// The registry is what the settings tab lists, so a theme missing from it
    /// could be stored in the settings and never offered back.
    #[test]
    fn every_theme_is_in_the_registry() {
        for theme in [
            Theme::Dark,
            Theme::LightDark,
            Theme::Light,
            Theme::Nebula,
            Theme::FrostLight,
        ] {
            assert_eq!(
                theme,
                entry(theme).theme,
                "{theme:?} is missing from THEMES or sits at another index"
            );
        }
    }

    #[test]
    fn every_theme_has_a_name() {
        for entry in THEMES {
            assert!(!entry.name.is_empty());
        }
    }

    /// Two series of one chart must never come out the same colour before the
    /// palette has been used up.
    #[test]
    fn the_series_of_a_palette_are_distinct() {
        for entry in THEMES {
            for series in [entry.palette.series, entry.palette.color_blind_series] {
                for (i, a) in series.iter().enumerate() {
                    for b in series.iter().skip(i + 1) {
                        assert_ne!(a, b, "{} repeats a series colour", entry.name);
                    }
                }
            }
        }
    }

    /// A team is five players and a breakdown is a handful of abilities; the
    /// palette has to cover that much before it starts over.
    #[test]
    fn a_palette_covers_at_least_eight_series() {
        for entry in THEMES {
            assert!(entry.palette.series.len() >= 8, "{}", entry.name);
            assert!(
                entry.palette.color_blind_series.len() >= 8,
                "{} colour-blind set",
                entry.name
            );
        }
    }

    #[test]
    fn series_colours_start_over_after_the_last_one() {
        let _active = ACTIVE_THEME.lock().unwrap();
        apply_for_test(Theme::LightDark);
        let len = palette().series.len();
        assert_eq!(series_color(0), series_color(len));
        assert_ne!(series_color(0), series_color(1));
    }

    /// The setting has to reach the colours the charts actually draw with, and
    /// leave them as it found them when it is switched off again.
    #[test]
    fn the_setting_switches_which_series_the_charts_draw() {
        let _active = ACTIVE_THEME.lock().unwrap();
        apply_for_test(Theme::LightDark);
        let ordinary = series_color(0);

        set_color_blind_series(true);
        assert_eq!(palette().color_blind_series[0], series_color(0));
        assert_ne!(ordinary, series_color(0));

        set_color_blind_series(false);
        assert_eq!(ordinary, series_color(0));
    }

    /// The deltas and the status marks keep their colours: those never stand on
    /// colour alone — a delta carries its sign, a mark its word — and swapping
    /// them would be a change of look rather than a help to anybody.
    #[test]
    fn the_setting_leaves_everything_but_the_series_alone() {
        let _active = ACTIVE_THEME.lock().unwrap();
        apply_for_test(Theme::LightDark);
        let (improve, worse, warn) = (palette().improve, palette().worse, palette().warn);

        set_color_blind_series(true);
        let after = palette();
        set_color_blind_series(false);

        assert_eq!(improve, after.improve);
        assert_eq!(worse, after.worse);
        assert_eq!(warn, after.warn);
    }

    /// Every pair of the colour-blind set has to stay apart under the two
    /// common deficiencies — not just neighbouring pairs, which is all the
    /// ordinary palette promises and all it manages. A chart can hold any
    /// eight of them at once, and it is the far-apart pairs of the ordinary set
    /// (its blue against its violet, its aqua against its magenta) that collapse.
    ///
    /// The floor is in CIELAB ΔE. Ten is roughly where two colours stop reading
    /// as shades of one thing; the sets below clear fifteen.
    #[test]
    fn the_colour_blind_series_stay_apart_for_a_dichromat() {
        for entry in THEMES {
            for deficiency in [Deficiency::Protanopia, Deficiency::Deuteranopia] {
                let worst = worst_pair(entry.palette.color_blind_series, deficiency);
                assert!(
                    worst >= 15.0,
                    "{} under {deficiency:?}: two series only ΔE {worst:.1} apart",
                    entry.name
                );
            }
        }
    }

    /// The point of the set. Kept as a comparison rather than a second floor,
    /// so retuning either palette cannot quietly leave the colour-blind one no
    /// better than what it replaces.
    #[test]
    fn the_colour_blind_series_beat_the_ordinary_ones_for_a_dichromat() {
        for entry in THEMES {
            for deficiency in [Deficiency::Protanopia, Deficiency::Deuteranopia] {
                let ordinary = worst_pair(entry.palette.series, deficiency);
                let helped = worst_pair(entry.palette.color_blind_series, deficiency);
                assert!(
                    helped > ordinary,
                    "{} under {deficiency:?}: the colour-blind set is no further apart \
                     (ΔE {helped:.1}) than the ordinary one (ΔE {ordinary:.1})",
                    entry.name
                );
            }
        }
    }

    #[derive(Debug, Clone, Copy)]
    enum Deficiency {
        Protanopia,
        Deuteranopia,
    }

    /// How far apart the closest two colours of `series` are, seen with
    /// `deficiency`.
    fn worst_pair(series: &[Color32], deficiency: Deficiency) -> f64 {
        let mut worst = f64::INFINITY;
        for (i, a) in series.iter().enumerate() {
            for b in series.iter().skip(i + 1) {
                let distance = delta_e(
                    dichromat(*a, deficiency),
                    dichromat(*b, deficiency),
                );
                worst = worst.min(distance);
            }
        }
        worst
    }

    /// What a dichromat sees, after Viénot, Brettel & Mollon (1999): the colour
    /// is taken to LMS, the missing cone's response is replaced by what the
    /// other two imply, and the result is taken back to RGB.
    fn dichromat(color: Color32, deficiency: Deficiency) -> [f64; 3] {
        let [r, g, b] = linear(color);
        let l = 17.8824 * r + 43.5161 * g + 4.11935 * b;
        let m = 3.45565 * r + 27.1554 * g + 3.86714 * b;
        let s = 0.0299566 * r + 0.184309 * g + 1.46709 * b;
        let (l, m) = match deficiency {
            Deficiency::Protanopia => (2.02344 * m - 2.52581 * s, m),
            Deficiency::Deuteranopia => (l, 0.494207 * l + 1.24827 * s),
        };
        [
            0.080_944_447_9 * l - 0.130_504_409 * m + 0.116_721_066 * s,
            -0.010_248_533_5 * l + 0.054_019_326_6 * m - 0.113_614_708 * s,
            -0.000_365_296_938 * l - 0.004_121_614_69 * m + 0.693_511_405 * s,
        ]
        .map(|c| c.clamp(0.0, 1.0))
    }

    fn linear(color: Color32) -> [f64; 3] {
        [color.r(), color.g(), color.b()].map(|c| {
            let c = c as f64 / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        })
    }

    /// Distance in CIELAB (the 1976 form — plain enough to read, and this is a
    /// floor rather than a just-noticeable difference).
    fn delta_e(a: [f64; 3], b: [f64; 3]) -> f64 {
        let a = lab(a);
        let b = lab(b);
        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
    }

    fn lab([r, g, b]: [f64; 3]) -> [f64; 3] {
        let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
        let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
        let f = |t: f64| {
            if t > 216.0 / 24389.0 {
                t.cbrt()
            } else {
                (841.0 / 108.0) * t + 4.0 / 29.0
            }
        };
        let (fx, fy, fz) = (f(x), f(y), f(z));
        [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
    }

    /// The text sizes must cover every style egui asks for, or text falls back
    /// to a default size that no longer matches the rest.
    #[test]
    fn every_text_style_has_a_size() {
        let styles: Vec<_> = TEXT_SIZES.iter().map(|(s, _, _)| s.clone()).collect();
        for style in [
            TextStyle::Small,
            TextStyle::Body,
            TextStyle::Button,
            TextStyle::Monospace,
            TextStyle::Heading,
        ] {
            assert!(styles.contains(&style), "{style:?} has no size");
        }
    }

    /// Every theme is made of the same material: rounded frames, a rim, and
    /// shadows on. One that missed the treatment would look like a different
    /// app one tab over.
    #[test]
    fn every_theme_is_rounded_rimmed_and_lifted() {
        for entry in THEMES {
            let visuals = (entry.visuals)();
            let name = entry.name;
            assert_ne!(
                CornerRadius::ZERO,
                visuals.widgets.inactive.corner_radius,
                "{name} paints square widgets"
            );
            assert_ne!(
                CornerRadius::ZERO,
                visuals.window_corner_radius,
                "{name} paints square windows"
            );
            assert!(
                visuals.widgets.inactive.bg_stroke.color.a() > 0,
                "{name} draws no rim on a resting widget"
            );
            assert_ne!(Shadow::NONE, visuals.window_shadow, "{name} casts no shadow");
        }
    }

    /// A field — a drop-down, a text box, a table row — has to stand off the
    /// page behind it. This is what a translucent "pane" fill costs: it looks
    /// like glass and reads like fog, and the combats list stops looking like
    /// something to click.
    #[test]
    fn a_field_stands_out_from_the_page_in_every_theme() {
        for entry in THEMES {
            let visuals = (entry.visuals)();
            let page = luma(visuals.panel_fill);
            let field = luma(visuals.widgets.inactive.bg_fill);
            assert!(
                (page - field).abs() >= 15.0,
                "{}: a resting field is {:.0} from the page it sits on, which reads as one surface",
                entry.name,
                (page - field).abs()
            );
        }
    }

    /// The rim firms up as the pointer arrives, which is what replaced washing
    /// the fill out.
    #[test]
    fn the_rim_gets_stronger_with_interaction() {
        for entry in THEMES {
            let widgets = (entry.visuals)().widgets;
            assert!(
                widgets.inactive.bg_stroke.color.a() < widgets.hovered.bg_stroke.color.a(),
                "{} does not light up on hover",
                entry.name
            );
        }
    }

    /// The material must not touch the fills — those are the theme's own, and
    /// they are what carries the contrast.
    #[test]
    fn glassify_leaves_the_fills_the_theme_chose() {
        let before = Visuals::dark();
        let after = glassify(
            Visuals::dark(),
            Glass {
                rim: Color32::WHITE,
                strength: 1.0,
            },
        );
        assert_eq!(before.widgets.inactive.bg_fill, after.widgets.inactive.bg_fill);
        assert_eq!(
            before.widgets.hovered.weak_bg_fill,
            after.widgets.hovered.weak_bg_fill
        );
        assert_eq!(before.faint_bg_color, after.faint_bg_color);
        assert_eq!(before.panel_fill, after.panel_fill);
        assert_eq!(before.selection.bg_fill, after.selection.bg_fill);
    }

    /// A rim with no alpha left is no rim. The light themes ease every alpha
    /// down, so that is where rounding to nothing would show up first.
    #[test]
    fn eased_off_rims_never_vanish() {
        for visuals in [frost_light_visuals(), light_visuals()] {
            for widget in [
                &visuals.widgets.noninteractive,
                &visuals.widgets.inactive,
                &visuals.widgets.hovered,
                &visuals.widgets.active,
                &visuals.widgets.open,
            ] {
                assert!(widget.bg_stroke.color.a() > 0);
            }
        }
    }

    /// Perceived brightness on the 0..255 scale, for asking whether two
    /// surfaces read as one.
    fn luma(color: Color32) -> f32 {
        0.299 * color.r() as f32 + 0.587 * color.g() as f32 + 0.114 * color.b() as f32
    }

    fn apply_for_test(theme: Theme) {
        ACTIVE.store(index_of(theme), Ordering::Relaxed);
    }

    /// What `apply` puts on the context: our sizes, the tweaks the app relies
    /// on, and the same style under both of egui's slots — the app picks its
    /// own theme rather than following the desktop's light/dark setting.
    #[test]
    fn applying_a_theme_sets_the_style_the_app_relies_on() {
        let _active = ACTIVE_THEME.lock().unwrap();
        let ctx = Context::default();
        apply(&ctx, Theme::Light);

        let style = ctx.style_of(eframe::egui::Theme::Dark);
        assert_eq!(
            13.0,
            style.text_styles[&TextStyle::Body].size,
            "the text sizes come from TEXT_SIZES"
        );
        assert!(!style.interaction.selectable_labels);
        assert_eq!(
            style.spacing.scroll.bar_width, style.spacing.scroll.floating_allocated_width,
            "a floating scroll bar has to get a strip of its own"
        );
        assert_eq!(
            style.visuals.dark_mode,
            ctx.style_of(eframe::egui::Theme::Light).visuals.dark_mode,
            "both egui slots carry the theme the app picked"
        );
    }

    /// Picking a theme has to change the colours the app paints with, not only
    /// egui's widget colours.
    #[test]
    fn the_palette_follows_the_applied_theme() {
        let _active = ACTIVE_THEME.lock().unwrap();
        let ctx = Context::default();
        apply(&ctx, Theme::Light);
        let light = palette().improve;
        apply(&ctx, Theme::LightDark);
        assert_ne!(light, palette().improve);
    }
}

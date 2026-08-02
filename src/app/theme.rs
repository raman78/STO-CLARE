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

use std::sync::atomic::{AtomicUsize, Ordering};

use eframe::{
    egui::{Color32, Context, FontFamily, FontId, Style, TextStyle, Visuals, style::Selection},
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
        visuals: Visuals::dark,
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
        visuals: Visuals::light,
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

const DARK_PALETTE: Palette = Palette {
    improve: Color32::from_rgb(0x5c, 0xb8, 0x5c),
    worse: Color32::from_rgb(0xd9, 0x53, 0x4f),
    warn: Color32::from_rgb(0xd9, 0x95, 0x00),
    ok: Color32::from_rgb(0x5c, 0xb8, 0x5c),
    busy: Color32::from_rgb(0xd9, 0x95, 0x00),
    error: Color32::from_rgb(0xd9, 0x53, 0x4f),
    series: DARK_SERIES,
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
};

/// Index into [`THEMES`] of the theme in use. The look is one per process — the
/// overlay runs its own egui context and shares it — so it lives here rather
/// than being carried to every call site that wants a colour.
static ACTIVE: AtomicUsize = AtomicUsize::new(0);

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
    let series = palette().series;
    series[index % series.len()]
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
    theme
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
        for theme in [Theme::Dark, Theme::LightDark, Theme::Light] {
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
            let series = entry.palette.series;
            for (i, a) in series.iter().enumerate() {
                for b in series.iter().skip(i + 1) {
                    assert_ne!(a, b, "{} repeats a series colour", entry.name);
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

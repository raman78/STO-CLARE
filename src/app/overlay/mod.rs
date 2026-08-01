use std::sync::Arc;

use eframe::{egui::*, epaint::mutex::Mutex};

use crate::{
    analyzer::{Combat, Player},
    app::settings::Settings,
    custom_widgets::table::Table,
    helpers::number_formatting::NumberFormatter,
};
use crate::custom_widgets::popup_button::PopupButton;

use super::analysis_handling::{AnalysisHandler, AnalysisInfo};

// The Linux wlr-layer-shell backend (always-on-top surface + its own thread).
#[cfg(target_os = "linux")]
pub mod layer_shell;

pub struct Overlay(Arc<Mutex<OverlayInner>>);

struct OverlayInner {
    // Used by the eframe-viewport path; the layer-shell surface owns its own
    // geometry instead.
    position: Option<Pos2>,
    current_size: Vec2,
    data: DisplayData,
    show: bool,
    // Only the viewport path toggles this; the layer-shell overlay owns its own
    // move state.
    move_around: bool,
    columns: Vec<ColumnDescriptor>,
    analysis_handler: AnalysisHandler,
    state: State,
    settings: Settings,
    // In a Wayland session the overlay is a wlr-layer-shell surface (always-on-top
    // over full-screen games) instead of an eframe viewport; see layer_shell.
    #[cfg(target_os = "linux")]
    layer: Option<layer_shell::LayerOverlay>,
    // wgpu handles shared with eframe's main-window renderer, used to spawn the
    // layer-shell overlay. Injected once at startup by App::new (see set_gpu),
    // and only in a Wayland session — so their presence is what selects the
    // layer-shell backend (see `uses_layer_shell`).
    #[cfg(target_os = "linux")]
    overlay_gpu: Option<layer_shell::OverlayGpu>,
}

#[derive(Default)]
enum State {
    Update(Arc<Combat>),
    Idle(Arc<Combat>),
    #[default]
    Empty,
}

#[derive(Default)]
struct DisplayData {
    columns: Vec<ColumnDescriptor>,
    players: Vec<DisplayPlayer>,
}

struct DisplayPlayer {
    name: String,
    columns: Vec<ColumnValue>,
}

struct ColumnValue {
    value: f64,
    value_string: String,
}

fn val(value: f64, value_string: String) -> ColumnValue {
    ColumnValue {
        value,
        value_string,
    }
}

#[derive(Clone)]
struct ColumnDescriptor {
    name: &'static str,
    enabled: bool,
    select: fn(&Settings, &Player, &mut NumberFormatter) -> ColumnValue,
}

macro_rules! col {
    ($name:expr, $select:expr $(,)?) => {
        ColumnDescriptor {
            name: $name,
            enabled: false,
            select: $select,
        }
    };
    ($name:expr, $enabled:expr, $select:expr $(,)?) => {
        ColumnDescriptor {
            name: $name,
            enabled: $enabled,
            select: $select,
        }
    };
}

static COLUMNS: &[ColumnDescriptor] = &[
    col!("DPS", true, |s, p, f| {
        val(
            p.damage_out.damage_metrics.dps.all,
            f.format(
                p.damage_out.damage_metrics.dps.all,
                if s.general.more_decimals { 2 } else { 0 },
            ),
        )
    }),
    col!("Dmg Out", |s, p, f| {
        val(
            p.damage_out.damage_metrics.total_damage.all,
            f.format(
                p.damage_out.damage_metrics.total_damage.all,
                if s.general.more_decimals { 2 } else { 0 },
            ),
        )
    }),
    col!("Dmg Out %", |s, p, f| {
        val(
            p.damage_out.damage_percentage.all.unwrap_or(0.0),
            p.damage_out
                .damage_percentage
                .all
                .map(|p| f.format(p, if s.general.more_decimals { 3 } else { 1 }))
                .unwrap_or(String::new()),
        )
    }),
    col!("Max One-Hit", |s, p, f| {
        val(
            p.damage_out.max_one_hit.damage,
            f.format(
                p.damage_out.max_one_hit.damage,
                if s.general.more_decimals { 2 } else { 0 },
            ),
        )
    }),
    col!("Dmg In", |s, p, f| {
        val(
            p.damage_in.damage_metrics.total_damage.all,
            f.format(
                p.damage_in.damage_metrics.total_damage.all,
                if s.general.more_decimals { 2 } else { 0 },
            ),
        )
    }),
    col!("Dmg In %", |s, p, f| {
        val(
            p.damage_in.damage_percentage.all.unwrap_or(0.0),
            p.damage_in
                .damage_percentage
                .all
                .map(|p| f.format(p, if s.general.more_decimals { 3 } else { 1 }))
                .unwrap_or(String::new()),
        )
    }),
    col!("Hits Out", |_, p, _| {
        val(
            p.damage_out.damage_metrics.hits.all as _,
            p.damage_out.damage_metrics.hits.all.to_string(),
        )
    }),
    col!("Hits Out %", |s, p, f| {
        val(
            p.damage_out.hits_percentage.all.unwrap_or(0.0),
            p.damage_out
                .hits_percentage
                .all
                .map(|p| f.format(p, if s.general.more_decimals { 3 } else { 1 }))
                .unwrap_or(String::new()),
        )
    }),
    col!("Hits In", |_, p, _| {
        val(
            p.damage_out.damage_metrics.hits.all as _,
            p.damage_out.damage_metrics.hits.all.to_string(),
        )
    }),
    col!("Hits In %", |s, p, f| {
        val(
            p.damage_in.hits_percentage.all.unwrap_or(0.0),
            p.damage_in
                .hits_percentage
                .all
                .map(|p| f.format(p, if s.general.more_decimals { 3 } else { 1 }))
                .unwrap_or(String::new()),
        )
    }),
    col!("Kills", |_, p, _| {
        let count: u32 = p.damage_out.kills.values().copied().sum();
        val(count as _, count.to_string())
    }),
    col!("Deaths", |_, p, _| {
        let count: u32 = p.damage_in.kills.values().copied().sum();
        val(count as _, count.to_string())
    }),
];

impl Overlay {
    pub fn new(root_handler: &AnalysisHandler, settings: &Settings) -> Self {
        Self(Arc::new(Mutex::new(OverlayInner {
            move_around: true,
            columns: COLUMNS.iter().cloned().collect(),
            // Must start non-zero: Wayland rejects a 0x0 xdg_surface geometry,
            // which crashes wgpu ("Surface is not configured for presentation").
            // Matches the min_inner_size used when building the viewport below.
            current_size: vec2(240.0, 80.0),
            data: Default::default(),
            position: None,
            // Come back up the way the app was left. The handler below already
            // starts with auto-refresh on, which is what `toggle_show` would
            // have set for a visible overlay, so nothing else is needed here.
            show: settings.general.overlay_shown,
            analysis_handler: root_handler.get_handler(true, Self::viewport_id()),
            state: State::Empty,
            settings: settings.clone(),
            #[cfg(target_os = "linux")]
            layer: None,
            #[cfg(target_os = "linux")]
            overlay_gpu: None,
        })))
    }

    /// Whether the overlay is currently open, so it can be persisted on exit.
    pub fn is_shown(&self) -> bool {
        self.0.lock().show
    }

    /// Injects the shared wgpu handles the layer-shell overlay renders through.
    /// Called once at startup by `App::new`, and only when the app really is
    /// running on Wayland — so this is also what selects the layer-shell
    /// backend over the plain viewport one.
    #[cfg(target_os = "linux")]
    pub fn set_gpu(&self, gpu: layer_shell::OverlayGpu) {
        self.0.lock().overlay_gpu = Some(gpu);
    }

    pub fn show(self: &Self, ui: &mut Ui) {
        let mut inner = self.0.lock();

        if Button::new("Overlay")
            .selected(inner.show)
            .ui(ui)
            .on_hover_text("Enables an Overlay, that you can move in front of the game window. Note that the it will always show the newest combat.")
            .clicked()
        {
            inner.toggle_show();
        }

        // With the viewport backend the overlay is a plain window, so its column
        // config (⛭) and move toggle (✋) live in the main window. The
        // layer-shell overlay carries both on its own toolbar instead.
        if !inner.uses_layer_shell() {
            PopupButton::new("⛭").show(ui, |ui| {
                ui.label("Configure what columns are displayed in the Overlay");
                let mut config_changed = false;
                for column in inner.columns.iter_mut() {
                    if ui.checkbox(&mut column.enabled, column.name).clicked() {
                        config_changed = true;
                    }
                }
                if config_changed {
                    inner.force_update(ui.ctx());
                }
            });

            ui.add_enabled_ui(inner.show, |ui: &mut Ui| {
                if Button::new("✋")
                    .selected(inner.move_around)
                    .ui(ui)
                    .on_hover_text("Move the Overlay")
                    .clicked()
                {
                    inner.move_around = !inner.move_around;
                }
            });
        }

        inner.poll_update(ui.ctx());
        if !inner.show {
            return;
        }

        // Wayland: render the overlay on a wlr-layer-shell surface so it stays
        // above full-screen games (the winit always-on-top hint is ignored
        // there). We push the freshly computed rows to that surface's own
        // thread; there is no eframe viewport in this case.
        #[cfg(target_os = "linux")]
        if inner.uses_layer_shell() {
            // Apply column toggles raised by the overlay's own ⛭ popup.
            let events = inner
                .layer
                .as_ref()
                .map(|layer| layer.poll_events())
                .unwrap_or_default();
            let mut config_changed = false;
            for event in events {
                match event {
                    layer_shell::OverlayEvent::ToggleColumn(index) => {
                        if let Some(column) = inner.columns.get_mut(index) {
                            column.enabled = !column.enabled;
                            config_changed = true;
                        }
                    }
                }
            }
            if config_changed {
                inner.force_update(ui.ctx());
            }

            if inner.layer.is_none() {
                if let Some(gpu) = inner.overlay_gpu.clone() {
                    let position = inner
                        .settings
                        .general
                        .overlay_position
                        .map_or((0, 0), |[top, left]| (top, left));
                    inner.layer =
                        Some(layer_shell::LayerOverlay::spawn(gpu, position));
                }
            }
            inner.check_update(ui.ctx());
            let data = inner.to_overlay_data();
            let style = ui.style().clone();
            if let Some(layer) = &inner.layer {
                layer.update(data);
                layer.set_style(style);
            }
            // Keep the main app repainting so we keep feeding the overlay and
            // polling its toolbar events (column toggles) promptly.
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(200));
            return;
        }

        // Everywhere else — Windows, macOS and an X11 session on Linux — the
        // overlay is a plain always-on-top eframe viewport.
        {
            let mut builder = ViewportBuilder::default()
                .with_title("CLA Overlay")
                .with_app_id("sto-cla-overlay")
                .with_decorations(inner.move_around)
                .with_minimize_button(false)
                .with_maximize_button(false)
                .with_close_button(true)
                .with_resizable(false)
                .with_min_inner_size(vec2(240.0, 80.0))
                .with_inner_size(inner.current_size)
                .with_always_on_top()
                .with_taskbar(false)
                .with_mouse_passthrough(!inner.move_around);
            builder.position = inner.position;
            drop(inner);
            let inner = self.0.clone();
            ui.ctx()
                .show_viewport_deferred(Self::viewport_id(), builder, move |ui, _| {
                    inner.lock().show_overlay(ui);
                });
        }
    }

    pub fn viewport_id() -> ViewportId {
        ViewportId("overlay".into())
    }

    pub fn request_repaint(ctx: &Context) {
        ctx.request_repaint_of(Self::viewport_id());
    }

    pub fn settings_changed(&self, settings: &Settings) {
        self.0.lock().settings = settings.clone();
    }

    /// Current overlay position as the (top, left) anchor margin, for
    /// persisting it; `None` when the layer overlay isn't running.
    #[cfg(target_os = "linux")]
    pub fn position(&self) -> Option<[i32; 2]> {
        let inner = self.0.lock();
        inner.layer.as_ref().map(|layer| {
            let (top, left) = layer.position();
            [top, left]
        })
    }
}

impl OverlayInner {
    /// Whether the overlay runs on a wlr-layer-shell surface instead of an
    /// eframe viewport.
    ///
    /// This is decided at runtime rather than by `cfg`, because a Linux build
    /// has to serve both session types: layer-shell is a Wayland protocol and
    /// there is nothing to talk to in an X11 session, where the plain
    /// always-on-top viewport works anyway. `App::new` only hands over the
    /// shared wgpu handles when the app really came up on Wayland, so having
    /// them is the same thing as being able to use layer-shell.
    fn uses_layer_shell(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            self.overlay_gpu.is_some()
        }
        #[cfg(not(target_os = "linux"))]
        {
            false
        }
    }

    // The eframe-viewport render path, used everywhere except a Wayland
    // session, where the overlay is a layer-shell surface rendered on its own
    // thread instead (see layer_shell).
    fn show_overlay(&mut self, ui: &mut Ui) {
        self.check_update(ui.ctx());
        CentralPanel::default().show_inside(ui, |ui| {
            if ui
                .ctx()
                .input_for(Overlay::viewport_id(), |i| i.viewport().close_requested())
            {
                self.toggle_show();
            }
            self.position = ui.ctx().input_for(Overlay::viewport_id(), |i| {
                i.viewport().outer_rect.map(|r| r.left_top())
            });
            let required_size = Table::new(ui)
                .min_scroll_height(f32::MAX)
                .header(15.0, |h| {
                    h.cell(|ui| {
                        ui.label("Player");
                    });

                    for column in self.data.columns.iter() {
                        h.cell(|ui| {
                            ui.label(column.name);
                        });
                    }
                })
                .body(25.0, |t| {
                    for player in self.data.players.iter() {
                        t.row(|r| {
                            r.cell(|ui| {
                                ui.label(player.name.as_str());
                            });

                            for column in player.columns.iter() {
                                r.cell_with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.label(column.value_string.as_str());
                                });
                            }
                        });
                    }
                })
                .size();
            let required_size = required_size
                + ui.spacing().window_margin.left_top()
                + ui.spacing().window_margin.right_bottom()
                + ui.spacing().item_spacing;
            // Never request below the viewport's min size: otherwise the
            // requested size and the compositor-clamped actual size disagree,
            // which keeps re-issuing resize commands (fragile on Wayland).
            let required_size = required_size.ceil().max(vec2(240.0, 80.0));
            if self.current_size != required_size {
                ui.ctx().send_viewport_cmd_to(
                    Overlay::viewport_id(),
                    ViewportCommand::InnerSize(required_size),
                );
                self.current_size = required_size;
            }
        });
    }

    fn toggle_show(&mut self) {
        self.show = !self.show;
        self.analysis_handler.enable_auto_refresh(self.show);
        #[cfg(target_os = "linux")]
        if !self.show {
            self.layer = None; // dropping stops the layer-shell overlay thread
        }
    }

    #[cfg(target_os = "linux")]
    fn to_overlay_data(&self) -> layer_shell::OverlayData {
        layer_shell::OverlayData {
            columns: self.data.columns.iter().map(|c| c.name.to_string()).collect(),
            rows: self
                .data
                .players
                .iter()
                .map(|p| layer_shell::OverlayRow {
                    name: p.name.clone(),
                    values: p.columns.iter().map(|c| c.value_string.clone()).collect(),
                })
                .collect(),
            // Full column list with enabled flags drives the overlay's ⛭ popup;
            // its ToggleColumn(index) maps straight back into `self.columns`.
            all_columns: self
                .columns
                .iter()
                .map(|c| (c.name.to_string(), c.enabled))
                .collect(),
        }
    }

    fn check_update(&mut self, ctx: &Context) {
        self.poll_update(ctx);
        let combat = match &self.state {
            State::Update(c) => c.clone(),
            State::Idle(_) | State::Empty => return,
        };
        self.perform_update(ctx, &combat);
        self.state = State::Idle(combat.clone());
    }

    fn poll_update(&mut self, ctx: &Context) {
        let combat = match self.analysis_handler.check_for_info().last() {
            Some(AnalysisInfo::Refreshed {
                latest_combat,
                combats: _,
                difficulties: _,
                base_names: _,
                environments: _,
                file_size: _,
            }) => latest_combat,
            _ => return,
        };
        self.state = State::Update(combat);
        if self.show {
            ctx.request_repaint_of(Overlay::viewport_id());
        }
    }

    fn force_update(&mut self, ctx: &Context) {
        match &self.state {
            State::Update(c) | State::Idle(c) => self.perform_update(ctx, &c.clone()),
            State::Empty => (),
        }
    }

    fn perform_update(&mut self, ctx: &Context, combat: &Combat) {
        if self.show {
            ctx.request_repaint_of(Overlay::viewport_id());
        }

        let mut display_data = DisplayData::default();
        display_data.columns = self.columns.iter().filter(|c| c.enabled).cloned().collect();
        let mut formatter = NumberFormatter::new();
        for (&player_name, player) in combat.players.iter() {
            let mut display_player = DisplayPlayer {
                name: combat
                    .name_manager
                    .get_name(player_name)
                    .unwrap()
                    .to_string(),
                columns: Vec::new(),
            };
            for column in display_data.columns.iter() {
                display_player.columns.push((column.select)(
                    &self.settings,
                    player,
                    &mut formatter,
                ));
            }
            display_data.players.push(display_player);
        }

        if display_data.columns.len() > 0 {
            display_data
                .players
                .sort_by(|p1, p2| p1.sort_value().total_cmp(&p2.sort_value()).reverse());
        }
        self.data = display_data;
    }
}

impl DisplayPlayer {
    fn sort_value(&self) -> f64 {
        self.columns.first().map(|c| c.value).unwrap_or(0.0)
    }
}

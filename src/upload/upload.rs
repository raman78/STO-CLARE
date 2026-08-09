use std::{io::Write, thread::JoinHandle, time::Duration};

use eframe::egui::*;
use reqwest::{
    Url,
    blocking::{
        ClientBuilder,
        multipart::{Form, Part},
    },
};
use serde::Deserialize;

use crate::{
    analyzer::{Combat, settings::AnalysisSettings},
    app::theme,
    custom_widgets::table::Table,
    helpers::number_formatting::NumberFormatter,
};

use super::common::{RequestError, spawn_request};

#[derive(Default)]
pub struct Upload {
    state: UploadState,
}

const UPLOAD_TOOLTIP: &str = "Uploads the current combat to the records (powered by OSCR). Note that the uploaded values may vary compared to the values displayed here, since the calculations may be done differently.";

impl Upload {
    pub fn show(
        &mut self,
        ui: &mut Ui,
        combat: Option<&Combat>,
        settings: &AnalysisSettings,
        url: &str,
    ) {
        ui.add_enabled_ui(self.state.is_idle() && combat.is_some(), |ui| {
            if ui
                .button("Upload 🌎")
                .on_hover_text(UPLOAD_TOOLTIP)
                .clicked()
            {
                self.state = self.begin_upload(ui.ctx().clone(), combat.unwrap(), settings, url);
            };
        });
        match &mut self.state {
            UploadState::Idle => (),
            UploadState::Uploading(join_handle) => {
                if join_handle.as_ref().unwrap().is_finished() {
                    self.state = join_handle.take().unwrap().join().unwrap();
                    ui.ctx().request_repaint_of(ViewportId::ROOT);
                }

                Self::window(ui, true, |ui| {
                    ui.with_layout(Layout::top_down(Align::Center), |ui| {
                        ui.add_space(20.0);
                        ui.label("uploading...");
                        ui.add_space(40.0);
                        ui.label(WidgetText::from("⏳").color(theme::palette().busy));
                        ui.add_space(20.0);
                    });
                });
            }
            UploadState::UploadComplete {
                detail,
                combatlog,
                entries,
            } => {
                let result = entries;
                // Built from the configured site rather than a fixed address, so
                // a run uploaded to a test server links to that server.
                let run_link = combatlog
                    .and_then(|id| Url::parse(url).ok().map(|base| (id, base)))
                    .and_then(|(id, base)| base.join(&format!("/ui/combatlog/{id}/")).ok());
                if let Some(true) = Self::window(ui, false, |ui| {
                    ui.label(&*detail);
                    if let Some(link) = &run_link {
                        ui.hyperlink_to("Open this run on the ladder site 🌎", link.as_str());
                    }
                    ui.add_space(10.0);
                    // The server accepts the log but can return no ladder rows at
                    // all — e.g. a solo-only ladder entered with a group. Without
                    // this the window would just show an empty table.
                    if result.is_empty() {
                        ui.label(
                            "The combat was uploaded, but it did not produce any ladder \
                             entries.\n\nThis usually means the map and difficulty have no \
                             ladder for this period, or the ladder only accepts solo runs.",
                        );
                    }
                    Table::new(ui)
                        .header(15.0, |r| {
                            r.cell(|ui| {
                                ui.label("Name");
                            });
                            r.cell(|ui| {
                                ui.label("Updated");
                            });
                            r.cell(|ui| {
                                ui.label("Details");
                            });
                            r.cell(|ui| {
                                ui.label("Value");
                            });
                        })
                        .body(25.0, |b| {
                            for result in result.iter() {
                                b.row(|r| {
                                    r.cell(|ui| {
                                        ui.label(&result.name);
                                    });
                                    r.cell_with_layout(
                                        Layout::top_down(Align::Center)
                                            .with_cross_align(Align::Center),
                                        |ui| {
                                            let text = match result.updated {
                                                true => {
                                                    WidgetText::from("✔").color(theme::palette().ok)
                                                }
                                                false => WidgetText::from("✖")
                                                    .color(theme::palette().error),
                                            };
                                            ui.label(text);
                                        },
                                    );
                                    r.cell(|ui| {
                                        ui.label(&result.detail);
                                    });
                                    r.cell(|ui| {
                                        ui.label(&result.value);
                                    });
                                });
                            }
                        });
                    ui.add_space(40.0);
                    ui.button("Close").clicked()
                }) {
                    self.state = UploadState::Idle;
                }
            }
            UploadState::UploadError(error) => {
                if let Some(true) = Self::window(ui, false, |ui| {
                    ui.label(&*error);
                    ui.add_space(40.0);
                    ui.button("Close").clicked()
                }) {
                    self.state = UploadState::Idle;
                }
            }
        }
    }

    fn window<R>(ui: &Ui, constrain: bool, add_contents: impl FnOnce(&mut Ui) -> R) -> Option<R> {
        let mut window = Window::new("Upload")
            .collapsible(false)
            .auto_sized()
            .constrain(true);

        if constrain {
            window = window.max_size([360.0, 480.0]);
        }

        window.show(ui.ctx(), add_contents).and_then(|r| r.inner)
    }

    fn begin_upload(
        &self,
        ctx: Context,
        combat: &Combat,
        settings: &AnalysisSettings,
        url: &str,
    ) -> UploadState {
        let combat_name = combat.name();
        let combat_data = combat.read_log_combat_data(settings.combatlog_file());
        let combat_data = match combat_data {
            Some(d) => d,
            // Used to return `Idle`, which made the button do nothing at all
            // with no explanation. Happens when the combat's byte range is
            // unknown, or the log file has since been moved, deleted or
            // rewritten (e.g. by "Clear Log File" or log consolidation).
            None => {
                log::error!(
                    "upload: cannot read combat data for {combat_name:?} from {}",
                    settings.combatlog_file().display()
                );
                return UploadState::UploadError(format!(
                    "Could not read this combat from the log file:\n{}\n\n\
                     The log may have been moved, deleted or rewritten since it was \
                     analyzed. Reload the log and try again.",
                    settings.combatlog_file().display()
                ));
            }
        };
        let url = match Url::parse(url) {
            Ok(u) => u,
            Err(_) => {
                log::error!("upload: invalid upload URL {url:?}");
                return UploadState::UploadError(format!(
                    "The upload URL is invalid:\n{url}\n\n\
                     Check it in Settings (it should normally be \
                     https://oscr.stobuilds.com/)."
                ));
            }
        };
        log::info!(
            "upload: sending {:?} ({} bytes) to {url}",
            combat_name,
            combat_data.len()
        );
        let join_handle = spawn_request(move || Self::upload(ctx, url, combat_data, combat_name));
        UploadState::Uploading(Some(join_handle))
    }

    fn upload(ctx: Context, url: Url, combat_data: Vec<u8>, combat_name: String) -> UploadState {
        let state = match Self::do_upload(url, combat_data, combat_name) {
            Ok(UploadOutcome::Uploaded {
                detail,
                combatlog,
                entries,
            }) => {
                log::info!("upload: {detail} (stored as combat log {combatlog:?})");
                for entry in entries.iter() {
                    log::info!(
                        "upload: {} — updated={} — {}",
                        entry.name,
                        entry.updated,
                        entry.detail
                    );
                }
                if entries.is_empty() {
                    log::info!("upload: accepted, but the server returned no ladder results");
                }
                UploadState::UploadComplete {
                    detail,
                    combatlog,
                    entries,
                }
            }
            // A log the server took but could not read. It answers `200` for
            // this, so nothing below the reason it gives is worth guessing at.
            Ok(UploadOutcome::Rejected(detail)) => {
                log::error!("upload: rejected — {detail}");
                UploadState::UploadError(format!("The combat could not be uploaded:\n\n{detail}"))
            }
            Err(e) => {
                log::error!("upload: failed — {e}");
                UploadState::UploadError(format!(
                    "{}",
                    e.action_error("Failed to upload combat log.")
                ))
            }
        };
        ctx.request_repaint_after_for(Duration::from_millis(10), ViewportId::ROOT);
        state
    }

    fn do_upload(
        url: Url,
        combat_data: Vec<u8>,
        combat_name: String,
    ) -> Result<UploadOutcome, RequestError> {
        // Gzipped, which is what the endpoint expects — the server hands the
        // bytes straight to the OSCR parser, which recognises the compression
        // by its leading magic bytes rather than by the file name, so the name
        // the combat carries here does not have to end in `.gz`.
        let mut data = Vec::new();
        let mut encoder = flate2::GzBuilder::new().write(&mut data, flate2::Compression::best());
        encoder.write_all(combat_data.as_slice()).unwrap();
        encoder.finish().unwrap();
        // Bounded like the official client's (3 s to connect, 60 s for the
        // answer). Without them a connection that never answers leaves the
        // upload thread waiting for good and the window saying "uploading..."
        // with no way out.
        let client = ClientBuilder::new()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(RESPONSE_TIMEOUT)
            .build()
            .unwrap();
        let url = url.join(UPLOAD_PATH).unwrap();
        let form = Form::new().part("file", Part::bytes(data).file_name(combat_name));
        let response = client.post(url).multipart(form).send()?;
        if !response.status().is_success() {
            return Err(RequestError::from(response));
        }

        Ok(response.json::<UploadResponseV2>()?.into())
    }
}

/// The upload endpoint. The `v2` one, which is what the OSCR client itself
/// uses: it answers a log it could not read with a plain reason instead of a
/// server error, and it names the log it stored.
const UPLOAD_PATH: &str = "/combatlog/uploadv2/";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Default)]
enum UploadState {
    #[default]
    Idle,
    Uploading(Option<JoinHandle<Self>>),
    UploadComplete {
        /// The server's own word for what happened, shown as it came.
        detail: String,
        /// Which stored log the upload became, when the server said. It names a
        /// page on the ladder site, so it becomes a link to the run itself.
        combatlog: Option<i64>,
        entries: Vec<UploadResponse>,
    },
    UploadError(String),
}

impl UploadState {
    fn is_idle(&self) -> bool {
        matches!(self, UploadState::Idle)
    }
}

/// What the `v2` endpoint answers with. It replies `200` whether or not the log
/// could be read, so the status says nothing: `results` is what tells the two
/// apart. Missing on the failure path — and `detail` then carries the reason,
/// which is the whole point of this endpoint over the older one.
///
/// `results` present but empty is a third case, and a success: the log was
/// accepted and produced no ladder rows.
#[derive(Deserialize)]
struct UploadResponseV2 {
    #[serde(default)]
    results: Option<Vec<UploadResponseModel>>,
    /// Which stored log the upload became. Only reported so it reaches the log
    /// file; nothing is built on it yet.
    #[serde(default)]
    combatlog: Option<i64>,
    detail: String,
}

/// A read log, or the reason it could not be read.
enum UploadOutcome {
    Uploaded {
        detail: String,
        combatlog: Option<i64>,
        entries: Vec<UploadResponse>,
    },
    Rejected(String),
}

impl From<UploadResponseV2> for UploadOutcome {
    fn from(response: UploadResponseV2) -> Self {
        match response.results {
            Some(results) => Self::Uploaded {
                detail: response.detail,
                combatlog: response.combatlog,
                entries: results.into_iter().map(Into::into).collect(),
            },
            None => Self::Rejected(response.detail),
        }
    }
}

#[derive(Deserialize)]
struct UploadResponseModel {
    name: String,
    updated: bool,
    detail: String,
    value: f64,
}

#[derive(Deserialize)]
struct UploadResponse {
    name: String,
    updated: bool,
    detail: String,
    value: String,
}
impl From<UploadResponseModel> for UploadResponse {
    fn from(value: UploadResponseModel) -> Self {
        let mut formatter = NumberFormatter::new();
        Self {
            name: value.name,
            updated: value.updated,
            detail: value.detail,
            value: formatter.format(value.value, 2),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape the server answers a log it read: the rows, the message, and
    /// the id of what it stored.
    #[test]
    fn a_read_log_comes_back_with_its_ladder_rows() {
        let response: UploadResponseV2 = serde_json::from_str(
            r#"{"results":[{"name":"Infected Space Elite","updated":true,
                "detail":"New personal best","value":123456.75}],
                "combatlog":84973,"detail":"Combatlog uploaded successfully."}"#,
        )
        .unwrap();

        match UploadOutcome::from(response) {
            UploadOutcome::Uploaded {
                detail,
                combatlog,
                entries,
            } => {
                assert_eq!(detail, "Combatlog uploaded successfully.");
                assert_eq!(combatlog, Some(84973));
                assert_eq!(entries.len(), 1);
                assert!(entries[0].updated);
            }
            UploadOutcome::Rejected(detail) => panic!("read log reported as rejected: {detail}"),
        }
    }

    /// The reason this endpoint is worth using: a log the server could not read
    /// comes back as an ordinary answer carrying the reason, not as an error.
    /// Only `detail` is present, so the two optional fields have to tolerate
    /// being absent entirely.
    #[test]
    fn an_unreadable_log_comes_back_as_its_reason() {
        let response: UploadResponseV2 =
            serde_json::from_str(r#"{"detail":"Combat log is empty"}"#).unwrap();

        match UploadOutcome::from(response) {
            UploadOutcome::Rejected(detail) => assert_eq!(detail, "Combat log is empty"),
            UploadOutcome::Uploaded { .. } => panic!("unreadable log reported as uploaded"),
        }
    }

    /// Explicit nulls mean the same as the fields being missing — the server
    /// declares both `allow_null` and `required=False`.
    #[test]
    fn explicit_nulls_read_the_same_as_missing_fields() {
        let response: UploadResponseV2 =
            serde_json::from_str(r#"{"results":null,"combatlog":null,"detail":"nope"}"#).unwrap();

        assert!(matches!(
            UploadOutcome::from(response),
            UploadOutcome::Rejected(_)
        ));
    }

    /// A log that was read but matched no ladder is a success with nothing in
    /// it, and must not be mistaken for a rejection — the window says so in its
    /// own words.
    #[test]
    fn an_accepted_log_with_no_ladder_rows_is_still_an_upload() {
        let response: UploadResponseV2 = serde_json::from_str(
            r#"{"results":[],"combatlog":12,"detail":"Combatlog uploaded successfully."}"#,
        )
        .unwrap();

        match UploadOutcome::from(response) {
            UploadOutcome::Uploaded { entries, .. } => assert!(entries.is_empty()),
            UploadOutcome::Rejected(detail) => {
                panic!("accepted log reported as rejected: {detail}")
            }
        }
    }
}

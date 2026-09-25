use super::{artist_suggestions, display_name, MusicApp};
use super::egui;

impl MusicApp {
    pub(super) fn review_window(&mut self, ui: &egui::Ui) {
        if self.review.is_empty() { return; }
        let mut artists: Vec<String> = self.tracks.iter().map(|track| track.artist.trim().to_string())
            .filter(|artist| !artist.is_empty()).collect();
        artists.sort_by_key(|artist| artist.to_lowercase());
        artists.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
        let mut import = false;
        let mut cancel = false;
        egui::Window::new("Review imports").default_width(580.0).show(ui.ctx(), |ui| {
            ui.label("Edit missing metadata. A blank artist is allowed.");
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                for candidate in &mut self.review {
                    ui.group(|ui| {
                        ui.label(candidate.source.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default());
                        if candidate.missing_title || candidate.missing_artist {
                            let missing = match (candidate.missing_title, candidate.missing_artist) {
                                (true, true) => "Missing title and artist",
                                (true, false) => "Missing title",
                                (false, true) => "Missing artist",
                                _ => "",
                            };
                            ui.small(missing);
                        }
                        ui.horizontal(|ui| {
                            ui.label("Title");
                            ui.text_edit_singleline(&mut candidate.title);
                        });
                        ui.horizontal(|ui| {
                            ui.label("Artist");
                            ui.text_edit_singleline(&mut candidate.artist);
                        });
                        let suggestions = artist_suggestions(&artists, &candidate.artist);
                        if !suggestions.is_empty() {
                            ui.horizontal_wrapped(|ui| {
                                ui.small("Existing artists:");
                                for artist in suggestions {
                                    if ui.button(artist).clicked() {
                                        candidate.artist = artist.to_string();
                                    }
                                }
                            });
                        }
                        ui.horizontal(|ui| {
                            ui.label("Album");
                            ui.text_edit_singleline(&mut candidate.album);
                        });
                    });
                }
            });
            ui.horizontal(|ui| {
                import = ui.button("Import").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        if import { self.commit_import(); }
        if cancel {
            self.review.clear();
            self.status = "Import cancelled".to_string();
        }
    }

    pub(super) fn remove_track_window(&mut self, ui: &egui::Ui) {
        let Some(id) = self.confirm_remove_track.clone() else { return };
        let Some(track) = self.track(&id) else {
            self.confirm_remove_track = None;
            return;
        };
        let description = format!("{} — {}", track.title, display_name(&track.artist, "Unknown artist"));
        let mut remove = false;
        let mut cancel = false;
        egui::Window::new("Remove track").collapsible(false).show(ui.ctx(), |ui| {
            ui.label(format!("Remove {description} from the library?"));
            ui.label("This also removes it from playlists. The audio file stays on disk.");
            ui.horizontal(|ui| {
                remove = ui.button("Remove track").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        if remove {
            self.confirm_remove_track = None;
            self.remove_track(&id);
        } else if cancel {
            self.confirm_remove_track = None;
        }
    }
}

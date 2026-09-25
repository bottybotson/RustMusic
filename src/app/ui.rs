mod dialogs;
mod playback;
mod views;

use std::time::Duration;

use eframe::egui;

use super::{matches_track, MusicApp, View};
use super::queue::QueueSource;
use crate::library::Track;

fn display_name(value: &str, empty: &str) -> String {
    if value.trim().is_empty() { empty.to_string() } else { value.to_string() }
}

fn duration_text(milliseconds: i64) -> String {
    if milliseconds <= 0 {
        return "—".to_string();
    }
    let seconds = milliseconds / 1000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

#[derive(Clone, Copy)]
enum PlayerIcon { Play, Pause, Previous, Next, Stop, Shuffle, Repeat }

fn icon_button(icon: PlayerIcon, selected: bool) -> egui::Button<'static> {
    let (source, label) = match icon {
        PlayerIcon::Play => (egui::include_image!("../../assets/play.svg"), "Play"),
        PlayerIcon::Pause => (egui::include_image!("../../assets/pause.svg"), "Pause"),
        PlayerIcon::Previous => (egui::include_image!("../../assets/previous.svg"), "Previous"),
        PlayerIcon::Next => (egui::include_image!("../../assets/next.svg"), "Next"),
        PlayerIcon::Stop => (egui::include_image!("../../assets/stop.svg"), "Stop"),
        PlayerIcon::Shuffle => (egui::include_image!("../../assets/shuffle.svg"), "Shuffle"),
        PlayerIcon::Repeat => (egui::include_image!("../../assets/repeat.svg"), "Repeat playlist"),
    };
    egui::Button::new(
        egui::Image::new(source)
            .fit_to_exact_size(egui::vec2(20.0, 20.0))
            .alt_text(label),
    )
    .image_tint_follows_text_color(true)
    .selected(selected)
    .min_size(egui::vec2(38.0, 38.0))
}

fn reorder_button(up: bool) -> egui::Button<'static> {
    let source = if up {
        egui::include_image!("../../assets/up.svg")
    } else {
        egui::include_image!("../../assets/down.svg")
    };
    egui::Button::new(
        egui::Image::new(source)
            .fit_to_exact_size(egui::vec2(18.0, 18.0))
            .alt_text(if up { "Move up" } else { "Move down" }),
    )
    .image_tint_follows_text_color(true)
    .min_size(egui::vec2(38.0, 38.0))
}

fn artist_suggestions<'a>(artists: &'a [String], input: &str) -> Vec<&'a str> {
    let prefix = input.trim().to_lowercase();
    if prefix.is_empty() { return Vec::new(); }
    artists.iter()
        .filter(|artist| {
            let name = artist.trim().to_lowercase();
            name.starts_with(&prefix) && name != prefix
        })
        .take(6)
        .map(String::as_str)
        .collect()
}

impl eframe::App for MusicApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.receive_scan();
        if self.incoming.is_some() || self.queue.index.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(200));
        }
        if self.seek_preview.is_none() && self.queue.index.is_some()
            && self.audio.as_ref().is_some_and(|engine| engine.is_empty()) {
            self.next();
        }
        egui::Panel::bottom("now_playing").exact_size(self.footer_height)
            .show(ui, |ui| self.draw_player(ui));
        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.heading("Music Library");
                ui.separator();
                for (view, label) in [(View::Library, "Library"), (View::Artists, "Artists"),
                    (View::Playlists, "Playlists"), (View::Queue, "Queue")] {
                    if ui.selectable_label(self.view == view, label).clicked() { self.view = view; }
                }
                ui.separator();
                if ui.add_enabled(self.incoming.is_none() && self.review.is_empty(), egui::Button::new("Import files")).clicked() {
                    self.begin_import();
                }
                if ui.add_enabled(self.incoming.is_none() && self.review.is_empty(), egui::Button::new("Import manifest")).clicked() {
                    self.begin_manifest_import();
                }
                if ui.add_enabled(self.selected_track.is_some() &&
                    matches!(self.view, View::Library | View::Artists | View::Playlists),
                    egui::Button::new("Play selected")).clicked() {
                    self.play_selected();
                }
                ui.add_space(12.0);
                for (view, label) in [(View::Settings, "Settings"), (View::Curation, "Curation")] {
                    if ui.selectable_label(self.view == view, label).clicked() { self.view = view; }
                }
            });
            ui.separator();
            match self.view {
                View::Library => self.draw_library(ui),
                View::Artists => self.draw_artists(ui),
                View::Playlists => self.draw_playlists(ui),
                View::Queue => self.draw_queue(ui),
                View::Settings => self.draw_settings(ui),
                View::Curation => self.draw_curation(ui),
            }
            if !self.status.is_empty() {
                ui.separator();
                ui.label(&self.status);
            }
        });
        self.review_window(ui);
        self.remove_track_window(ui);
        if let Some((track_id, playlist_id)) = self.pending_playlist_add.take() {
            self.add_track_to_playlist(&playlist_id, &track_id);
        }
        if let Some((track_id, next)) = self.pending_queue_add.take() {
            self.add_to_queue(track_id, next);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{artist_suggestions, MusicApp, View};
    use crate::library::Library;
    use egui_kittest::{kittest::Queryable, Harness};

    #[test]
    fn suggests_existing_artists_by_prefix() {
        let artists = vec!["ABBA".to_string(), "Adele".to_string(), "Muse".to_string()];
        assert_eq!(artist_suggestions(&artists, " ad"), vec!["Adele"]);
        assert!(artist_suggestions(&artists, "adele").is_empty());
        assert!(artist_suggestions(&artists, "").is_empty());
    }

    #[test]
    fn creates_playlist_through_the_ui() {
        let path = std::env::temp_dir().join(format!(
            "music-library-ui-{}.sqlite3",
            uuid::Uuid::new_v4()
        ));
        let mut harness = Harness::<MusicApp>::builder()
            .with_size(eframe::egui::vec2(1100.0, 720.0))
            .build_eframe(|cc| MusicApp::new(Library::open(&path).unwrap(), &cc.egui_ctx));

        harness.get_by_label("Playlists").click();
        harness.step();
        assert!(harness.state().view == View::Playlists);

        harness.get_by_label("Create").click();
        harness.step();
        assert_eq!(harness.state().status, "Enter a playlist name");

        harness.state_mut().new_playlist_name = "Morning Mix".to_owned();
        harness.step();
        harness.get_by_label("Create").click();
        harness.step();

        let playlists = harness.state().library.playlists().unwrap();
        assert_eq!(playlists.len(), 1);
        assert_eq!(playlists[0].name, "Morning Mix");
        harness.step();
        harness.get_by_label("Morning Mix");

        drop(harness);
        std::fs::remove_file(path).unwrap();
    }
}

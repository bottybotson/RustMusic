use std::time::Duration;

use super::{display_name, duration_text, icon_button, matches_track, reorder_button, MusicApp, PlayerIcon};
use super::{egui, QueueSource, Track};

impl MusicApp {
    fn queue_source_name(&self) -> Option<String> {
        match &self.queue.source {
            QueueSource::Library => Some("Library".to_string()),
            QueueSource::Playlist { id, name } => Some(self.playlists.iter()
                .find(|playlist| &playlist.id == id).map(|playlist| playlist.name.clone())
                .unwrap_or_else(|| name.clone())),
            QueueSource::Album(name) => Some(display_name(name, "Unknown album")),
            QueueSource::Single if self.queue.ids.len() > 1 => Some("Custom queue".to_string()),
            QueueSource::Single => None,
        }
    }

    pub(super) fn draw_queue(&mut self, ui: &mut egui::Ui) {
        ui.heading("Queue");
        if let Some(source) = self.queue_source_name() {
            if self.queue.index.is_some() { ui.label(format!("Playing from {source}")); }
        }
        if let Some(index) = self.queue.index {
            if let Some(track) = self.queue.ids.get(index).and_then(|id| self.track(id)) {
                ui.strong(format!("Now playing: {} — {}", track.title,
                    display_name(&track.artist, "Unknown artist")));
            }
        } else {
            ui.label("Choose a playlist, album or song to start playback.");
        }
        ui.separator();
        ui.add(egui::TextEdit::singleline(&mut self.queue_search)
            .hint_text("Find a song to add to the queue"));
        if !self.queue_search.trim().is_empty() {
            let results: Vec<Track> = self.tracks.iter()
                .filter(|track| matches_track(track, &self.queue_search)).take(50).cloned().collect();
            egui::ScrollArea::vertical().id_salt("queue_search_results").max_height(150.0).show(ui, |ui| {
                for track in results {
                    ui.horizontal(|ui| {
                        if ui.button("Play next").clicked() { self.add_to_queue(track.id.clone(), true); }
                        if ui.button("Add to queue").clicked() { self.add_to_queue(track.id.clone(), false); }
                        ui.label(format!("{} — {}", track.title, display_name(&track.artist, "Unknown artist")));
                    });
                }
            });
        }
        ui.label("Up next · use the arrows to change playback order");
        let start = self.queue.upcoming_start();
        let upcoming: Vec<(usize, String)> = self.queue.ids.iter().enumerate().skip(start)
            .map(|(index, id)| (index, id.clone())).collect();
        let mut move_entry = None;
        let mut remove_entry = None;
        egui::ScrollArea::vertical().id_salt("queue_up_next").show(ui, |ui| {
            for (index, id) in upcoming {
                ui.horizontal(|ui| {
                    if ui.add_enabled(index > start, reorder_button(true)).on_hover_text("Earlier").clicked() {
                        move_entry = Some((index, index - 1));
                    }
                    if ui.add_enabled(index + 1 < self.queue.ids.len(), reorder_button(false))
                        .on_hover_text("Later").clicked() {
                        move_entry = Some((index, index + 1));
                    }
                    if ui.button("Remove").clicked() { remove_entry = Some(index); }
                    if let Some(track) = self.track(&id) {
                        ui.label(format!("{} — {}", track.title, display_name(&track.artist, "Unknown artist")));
                    } else {
                        ui.label("Unavailable song");
                    }
                });
            }
        });
        if let Some((from, to)) = move_entry { self.queue.move_upcoming(from, to); }
        if let Some(index) = remove_entry { self.queue.remove_upcoming(index); }
    }

    pub(super) fn draw_player(&mut self, ui: &mut egui::Ui) {
        let current = self.queue.index.and_then(|index| self.queue.ids.get(index))
            .and_then(|id| self.track(id)).cloned();
        if current.is_some() {
            if let Some(source) = self.queue_source_name() { ui.heading(source); }
        }
        let mut previous = false;
        let mut next = false;
        let mut stop = false;
        ui.horizontal(|ui| {
            if let Some(track) = &current {
                ui.strong(&track.title);
                ui.label(format!("— {}", display_name(&track.artist, "Unknown artist")));
            } else {
                ui.label("Nothing playing");
            }
        });
        ui.horizontal_wrapped(|ui| {
            previous = ui.add_enabled(current.is_some(), icon_button(PlayerIcon::Previous, false))
                .on_hover_text("Previous").clicked();
            if let Some(engine) = &self.audio {
                let icon = if engine.is_paused() { PlayerIcon::Play } else { PlayerIcon::Pause };
                if ui.add_enabled(current.is_some(), icon_button(icon, false))
                    .on_hover_text(if engine.is_paused() { "Resume" } else { "Pause" }).clicked() {
                    engine.pause_or_resume();
                }
            } else {
                ui.add_enabled(false, icon_button(PlayerIcon::Play, false)).on_hover_text("Play");
            }
            next = ui.add_enabled(current.is_some(), icon_button(PlayerIcon::Next, false))
                .on_hover_text("Next").clicked();
            stop = ui.add_enabled(current.is_some(), icon_button(PlayerIcon::Stop, false))
                .on_hover_text("Stop").clicked();
            if ui.add_enabled(current.is_some(), icon_button(PlayerIcon::Shuffle, self.queue.shuffle))
                .on_hover_text("Shuffle upcoming songs").clicked() {
                self.queue.toggle_shuffle();
            }
            let playlist_active = current.is_some() && matches!(&self.queue.source, QueueSource::Playlist { .. });
            if ui.add_enabled(playlist_active, icon_button(PlayerIcon::Repeat, self.queue.repeat_playlist))
                .on_hover_text("Repeat this playlist").clicked() {
                self.queue.toggle_repeat_playlist();
            }
            if ui.add_sized([150.0, 28.0], egui::Slider::new(&mut self.volume, 0.0..=1.0).text("Volume")).changed() {
                if let Some(engine) = &self.audio { engine.set_volume(self.volume); }
            }
        });
        if let (Some(track), Some(engine)) = (&current, &self.audio) {
            if track.duration_ms > 0 {
                let length = track.duration_ms as f32 / 1000.0;
                let mut position = self.seek_preview.unwrap_or_else(|| engine.position().as_secs_f32()).min(length);
                ui.horizontal(|ui| {
                    ui.label(duration_text((position * 1000.0) as i64));
                    let width = (ui.available_width() - 55.0).max(80.0);
                    let response = ui.add_sized([width, 24.0],
                        egui::Slider::new(&mut position, 0.0..=length).show_value(false));
                    if response.changed() { self.seek_preview = Some(position); }
                    if !response.is_pointer_button_down_on() {
                        if let Some(position) = self.seek_preview.take() {
                            if let Err(error) = engine.seek(Duration::from_secs_f32(position)) {
                            self.status = format!("Cannot seek: {error}");
                            }
                        }
                    }
                    ui.label(duration_text(track.duration_ms));
                });
            }
        }
        if previous { self.previous(); }
        if next { self.next(); }
        if stop { self.stop(); }
    }
}

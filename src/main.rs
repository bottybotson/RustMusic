mod audio;
mod importer;
mod library;

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use eframe::egui;
use importer::Candidate;
use library::{AddResult, Library, Playlist, PlaylistEntry, Track};

struct ScanResult {
    candidates: Vec<Candidate>,
    errors: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Library,
    Artists,
    Playlists,
    Settings,
}

#[derive(Default)]
struct Queue {
    ids: Vec<String>,
    index: Option<usize>,
}

struct MusicApp {
    library: Library,
    tracks: Vec<Track>,
    playlists: Vec<Playlist>,
    entries: Vec<PlaylistEntry>,
    incoming: Option<Receiver<ScanResult>>,
    review: Vec<Candidate>,
    view: View,
    selected_track: Option<String>,
    confirm_remove_track: Option<String>,
    selected_artist: Option<String>,
    selected_album: Option<String>,
    selected_playlist: Option<String>,
    add_target: Option<String>,
    new_playlist_name: String,
    rename_value: String,
    confirm_delete: bool,
    queue: Queue,
    audio: Option<audio::Audio>,
    volume: f32,
    ui_scale: f32,
    search: String,
    status: String,
}

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

impl MusicApp {
    fn new(library: Library, ctx: &egui::Context) -> Self {
        let tracks = library.tracks().unwrap_or_default();
        let playlists = library.playlists().unwrap_or_default();
        let add_target = playlists.first().map(|item| item.id.clone());
        let ui_scale = library.ui_scale().unwrap_or(1.2);
        ctx.set_zoom_factor(ui_scale);
        ctx.all_styles_mut(|style| {
            style.spacing.interact_size.y = 26.0;
            style.spacing.button_padding = egui::vec2(8.0, 5.0);
            style.spacing.item_spacing = egui::vec2(9.0, 7.0);
        });
        Self {
            library,
            tracks,
            playlists,
            entries: Vec::new(),
            incoming: None,
            review: Vec::new(),
            view: View::Library,
            selected_track: None,
            confirm_remove_track: None,
            selected_artist: None,
            selected_album: None,
            selected_playlist: None,
            add_target,
            new_playlist_name: String::new(),
            rename_value: String::new(),
            confirm_delete: false,
            queue: Queue::default(),
            audio: None,
            volume: 0.8,
            ui_scale,
            search: String::new(),
            status: String::new(),
        }
    }

    fn begin_import(&mut self) {
        if self.incoming.is_some() || !self.review.is_empty() {
            return;
        }
        let Some(paths) = rfd::FileDialog::new()
            .add_filter("Audio", &["mp3", "flac", "wav", "ogg", "oga"])
            .pick_files()
        else {
            return;
        };
        let count = paths.len();
        let (sender, receiver) = mpsc::channel();
        self.incoming = Some(receiver);
        self.status = format!("Scanning {count} files…");
        std::thread::spawn(move || {
            let mut candidates = Vec::new();
            let mut errors = Vec::new();
            for path in paths {
                match importer::scan(&path) {
                    Ok(candidate) => candidates.push(candidate),
                    Err(error) => errors.push(format!("{}: {error}", path.display())),
                }
            }
            let _ = sender.send(ScanResult { candidates, errors });
        });
    }

    fn receive_scan(&mut self) {
        let Some(receiver) = &self.incoming else { return };
        let Ok(result) = receiver.try_recv() else { return };
        self.incoming = None;
        self.review = result.candidates;
        if !result.errors.is_empty() {
            self.status = format!("{} unreadable files:\n{}", result.errors.len(), result.errors.join("\n"));
        } else if self.review.is_empty() {
            self.status = "No supported audio files found".to_string();
        } else {
            self.status = format!("Review {} tracks before import", self.review.len());
        }
    }

    fn commit_import(&mut self) {
        if self.review.iter().any(|item| item.title.trim().is_empty()) {
            self.status = "Enter a title for every track before importing".to_string();
            return;
        }
        let mut added = 0;
        let mut relinked = 0;
        let mut duplicates = 0;
        let mut errors = Vec::new();
        for candidate in self.review.drain(..) {
            match self.library.add(&candidate) {
                Ok(AddResult::Added) => added += 1,
                Ok(AddResult::Relinked) => relinked += 1,
                Ok(AddResult::Duplicate) => duplicates += 1,
                Err(error) => errors.push(error.to_string()),
            }
        }
        match self.library.tracks() {
            Ok(tracks) => self.tracks = tracks,
            Err(error) => errors.push(error.to_string()),
        }
        self.status = format!("Imported {added}, relinked {relinked}, skipped {duplicates} identical files");
        if !errors.is_empty() {
            self.status.push_str(&format!(". Errors: {}", errors.join("; ")));
        }
    }

    fn refresh_playlists(&mut self) {
        match self.library.playlists() {
            Ok(playlists) => {
                self.playlists = playlists;
                if self.add_target.as_ref().is_none_or(|id| !self.playlists.iter().any(|item| &item.id == id)) {
                    self.add_target = self.playlists.first().map(|item| item.id.clone());
                }
            }
            Err(error) => self.status = format!("Cannot read playlists: {error}"),
        }
    }

    fn choose_playlist(&mut self, id: String) {
        self.selected_playlist = Some(id.clone());
        self.rename_value = self.playlists.iter().find(|item| item.id == id).map(|item| item.name.clone()).unwrap_or_default();
        self.confirm_delete = false;
        self.reload_entries();
    }

    fn reload_entries(&mut self) {
        if let Some(id) = &self.selected_playlist {
            match self.library.entries(id) {
                Ok(entries) => self.entries = entries,
                Err(error) => self.status = format!("Cannot read playlist: {error}"),
            }
        } else {
            self.entries.clear();
        }
    }

    fn create_playlist(&mut self) {
        let name = self.new_playlist_name.trim();
        if name.is_empty() {
            self.status = "Enter a playlist name".to_string();
            return;
        }
        match self.library.create_playlist(name) {
            Ok(id) => {
                self.new_playlist_name.clear();
                self.refresh_playlists();
                self.choose_playlist(id);
                self.view = View::Playlists;
            }
            Err(error) => self.status = format!("Cannot create playlist: {error}"),
        }
    }

    fn add_selected_to_playlist(&mut self) {
        let Some(id) = self.add_target.clone() else { return };
        let Some(track) = self.tracks.iter().find(|track| Some(&track.id) == self.selected_track.as_ref()).cloned() else {
            self.status = "Select a track first".to_string();
            return;
        };
        match self.library.add_to_playlist(&id, &track) {
            Ok(()) => {
                self.status = format!("Added {} to playlist", track.title);
                if self.selected_playlist.as_ref() == Some(&id) {
                    self.reload_entries();
                }
            }
            Err(error) => self.status = format!("Cannot add track: {error}"),
        }
    }

    fn track(&self, id: &str) -> Option<&Track> {
        self.tracks.iter().find(|track| track.id == id)
    }

    fn remove_track(&mut self, id: &str) {
        let title = self.track(id).map(|track| track.title.clone()).unwrap_or_default();
        match self.library.remove_track(id) {
            Ok(true) => {
                let was_playing = self.queue.index.and_then(|index| self.queue.ids.get(index))
                    .is_some_and(|playing_id| playing_id == id);
                let playing_id = self.queue.index.and_then(|index| self.queue.ids.get(index)).cloned();
                self.queue.ids.retain(|queued_id| queued_id != id);
                self.queue.index = playing_id.and_then(|playing_id| {
                    self.queue.ids.iter().position(|queued_id| queued_id == &playing_id)
                });
                if was_playing {
                    if let Some(engine) = &self.audio { engine.stop(); }
                }
                self.selected_track = None;
                self.status = format!("Removed {title} from the library and playlists. Audio file kept.");
                match self.library.tracks() {
                    Ok(tracks) => self.tracks = tracks,
                    Err(error) => self.status = format!("Track removed, but cannot refresh library: {error}"),
                }
                self.reload_entries();
                if self.selected_artist.as_ref().is_some_and(|artist| !self.tracks.iter().any(|track| &track.artist == artist)) {
                    self.selected_artist = None;
                    self.selected_album = None;
                } else if self.selected_album.as_ref().is_some_and(|album| !self.tracks.iter().any(|track| &track.album == album && Some(&track.artist) == self.selected_artist.as_ref())) {
                    self.selected_album = None;
                }
            }
            Ok(false) => self.status = "Track no longer exists in the library".to_string(),
            Err(error) => self.status = format!("Cannot remove track: {error}"),
        }
    }

    fn start_queue(&mut self, ids: Vec<String>, start: usize) {
        if ids.is_empty() {
            self.status = "No tracks to play".to_string();
            return;
        }
        self.queue.ids = ids;
        self.queue.index = None;
        self.play_from(start, true);
    }

    fn play_from(&mut self, start: usize, forward: bool) {
        if self.audio.is_none() {
            match audio::Audio::new() {
                Ok(engine) => self.audio = Some(engine),
                Err(error) => {
                    self.status = format!("Audio output: {error}");
                    return;
                }
            }
        }
        let indexes: Box<dyn Iterator<Item = usize>> = if forward {
            Box::new(start..self.queue.ids.len())
        } else {
            Box::new((0..=start).rev())
        };
        for index in indexes {
            let Some(track) = self.track(&self.queue.ids[index]) else { continue };
            if !track.source.is_file() { continue; }
            let title = track.title.clone();
            let source = track.source.clone();
            let engine = self.audio.as_ref().expect("audio initialized");
            engine.set_volume(self.volume);
            match engine.load_and_play(&source) {
                Ok(()) => {
                    self.queue.index = Some(index);
                    self.status = format!("Playing {title}");
                    return;
                }
                Err(error) => self.status = format!("Skipping {title}: {error}"),
            }
        }
        if let Some(engine) = &self.audio { engine.stop(); }
        self.queue.index = None;
        self.status = "No more available tracks in the queue".to_string();
    }

    fn next(&mut self) {
        if let Some(index) = self.queue.index {
            self.play_from(index + 1, true);
        }
    }

    fn previous(&mut self) {
        let Some(index) = self.queue.index else { return };
        if let Some(engine) = &self.audio {
            if engine.position() > Duration::from_secs(3) {
                if let Err(error) = engine.seek(Duration::ZERO) {
                    self.status = format!("Cannot seek: {error}");
                }
                return;
            }
        }
        self.play_from(index.saturating_sub(1), false);
    }

    fn play_selected(&mut self) {
        let Some(selected) = self.selected_track.clone() else {
            self.status = "Select a track to play".to_string();
            return;
        };
        let ids: Vec<String> = match self.view {
            View::Library => self.filtered_library_ids(),
            View::Artists => self.album_ids(),
            View::Playlists => self.entries.iter().map(|item| item.track_id.clone()).collect(),
        };
        if let Some(start) = ids.iter().position(|id| id == &selected) {
            self.start_queue(ids, start);
        } else {
            self.start_queue(vec![selected], 0);
        }
    }

    fn filtered_library_ids(&self) -> Vec<String> {
        let search = self.search.to_lowercase();
        self.tracks.iter().filter(|track| {
            search.is_empty() || format!("{} {} {}", track.title, track.artist, track.album).to_lowercase().contains(&search)
        }).map(|track| track.id.clone()).collect()
    }

    fn artist_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.tracks.iter().map(|track| track.artist.clone()).collect::<BTreeSet<_>>().into_iter().collect();
        names.sort_by_key(|name| name.to_lowercase());
        names
    }

    fn album_names(&self) -> Vec<String> {
        let Some(artist) = &self.selected_artist else { return Vec::new() };
        let mut names: Vec<String> = self.tracks.iter().filter(|track| &track.artist == artist)
            .map(|track| track.album.clone()).collect::<BTreeSet<_>>().into_iter().collect();
        names.sort_by_key(|name| name.to_lowercase());
        names
    }

    fn album_ids(&self) -> Vec<String> {
        let (Some(artist), Some(album)) = (&self.selected_artist, &self.selected_album) else { return Vec::new() };
        let mut tracks: Vec<&Track> = self.tracks.iter().filter(|track| &track.artist == artist && &track.album == album).collect();
        tracks.sort_by_key(|track| track.title.to_lowercase());
        tracks.into_iter().map(|track| track.id.clone()).collect()
    }

    fn draw_add_to_playlist(&mut self, ui: &mut egui::Ui) {
        if self.playlists.is_empty() { return; }
        let current_name = self.add_target.as_ref().and_then(|id| self.playlists.iter().find(|item| &item.id == id))
            .map(|item| item.name.clone()).unwrap_or_else(|| "Choose playlist".to_string());
        ui.horizontal(|ui| {
            ui.label("Selected track →");
            egui::ComboBox::from_id_salt("add_to_playlist").selected_text(current_name).show_ui(ui, |ui| {
                for item in &self.playlists {
                    ui.selectable_value(&mut self.add_target, Some(item.id.clone()), &item.name);
                }
            });
            if ui.button("Add to playlist").clicked() {
                self.add_selected_to_playlist();
            }
        });
    }

    fn draw_library(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("All tracks");
            ui.label("Search");
            ui.text_edit_singleline(&mut self.search);
        });
        ui.small("Right-click a track title to remove it from the library.");
        self.draw_add_to_playlist(ui);
        let ids = self.filtered_library_ids();
        let mut play = None;
        egui::ScrollArea::both().show(ui, |ui| {
            egui::Grid::new("library_tracks").striped(true).num_columns(7).show(ui, |ui| {
                for heading in ["", "Title", "Artist", "Album", "Length", "Format", "File"] {
                    ui.strong(heading);
                }
                ui.end_row();
                for (index, id) in ids.iter().enumerate() {
                    let Some(track) = self.track(id).cloned() else { continue };
                    if ui.button("▶").clicked() { play = Some(index); }
                    let response = ui.selectable_label(self.selected_track.as_ref() == Some(id), &track.title);
                    if response.clicked() {
                        self.selected_track = Some(id.clone());
                    }
                    response.context_menu(|ui| {
                        if ui.button("Remove from library…").clicked() {
                            self.confirm_remove_track = Some(id.clone());
                            ui.close();
                        }
                    });
                    ui.label(display_name(&track.artist, "Unknown artist"));
                    ui.label(display_name(&track.album, "Unknown album"));
                    ui.label(duration_text(track.duration_ms));
                    ui.label(&track.format);
                    ui.label(if track.source.is_file() { "Available" } else { "Missing" });
                    ui.end_row();
                }
            });
        });
        if let Some(index) = play { self.start_queue(ids, index); }
    }

    fn draw_artists(&mut self, ui: &mut egui::Ui) {
        ui.heading("Artists");
        self.draw_add_to_playlist(ui);
        let artists = self.artist_names();
        let albums = self.album_names();
        let ids = self.album_ids();
        let mut artist_choice = None;
        let mut album_choice = None;
        let mut play = None;
        ui.columns(3, |columns| {
            columns[0].strong("Artist");
            egui::ScrollArea::vertical().id_salt("artist_list").show(&mut columns[0], |ui| {
                for artist in &artists {
                    let selected = self.selected_artist.as_ref() == Some(artist);
                    if ui.selectable_label(selected, display_name(artist, "Unknown artist")).clicked() {
                        artist_choice = Some(artist.clone());
                    }
                }
            });
            columns[1].strong("Album");
            egui::ScrollArea::vertical().id_salt("album_list").show(&mut columns[1], |ui| {
                for album in &albums {
                    let selected = self.selected_album.as_ref() == Some(album);
                    if ui.selectable_label(selected, display_name(album, "Unknown album")).clicked() {
                        album_choice = Some(album.clone());
                    }
                }
            });
            columns[2].strong("Tracks");
            if !ids.is_empty() && columns[2].button("Play album").clicked() { play = Some(0); }
            egui::ScrollArea::vertical().id_salt("album_tracks").show(&mut columns[2], |ui| {
                for (index, id) in ids.iter().enumerate() {
                    let Some(track) = self.track(id).cloned() else { continue };
                    ui.horizontal(|ui| {
                        if ui.button("▶").clicked() { play = Some(index); }
                        let response = ui.selectable_label(self.selected_track.as_ref() == Some(id), &track.title);
                        if response.clicked() {
                            self.selected_track = Some(id.clone());
                        }
                        response.context_menu(|ui| {
                            if ui.button("Remove from library…").clicked() {
                                self.confirm_remove_track = Some(id.clone());
                                ui.close();
                            }
                        });
                        ui.label(duration_text(track.duration_ms));
                        if !track.source.is_file() { ui.small("Missing"); }
                    });
                }
            });
        });
        if let Some(artist) = artist_choice {
            self.selected_artist = Some(artist);
            self.selected_album = None;
        }
        if let Some(album) = album_choice { self.selected_album = Some(album); }
        if let Some(index) = play { self.start_queue(ids, index); }
    }

    fn draw_playlists(&mut self, ui: &mut egui::Ui) {
        ui.heading("Playlists");
        let mut create = false;
        let mut choose = None;
        ui.columns(2, |columns| {
            columns[0].horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.new_playlist_name).hint_text("New playlist name"));
                create = ui.button("Create").clicked();
            });
            egui::ScrollArea::vertical().id_salt("playlists").show(&mut columns[0], |ui| {
                for item in &self.playlists {
                    if ui.selectable_label(self.selected_playlist.as_ref() == Some(&item.id), &item.name).clicked() {
                        choose = Some(item.id.clone());
                    }
                }
            });
            self.draw_playlist_detail(&mut columns[1]);
        });
        if create { self.create_playlist(); }
        if let Some(id) = choose { self.choose_playlist(id); }
    }

    fn draw_playlist_detail(&mut self, ui: &mut egui::Ui) {
        let Some(playlist_id) = self.selected_playlist.clone() else {
            ui.label("Choose a playlist or create one.");
            return;
        };
        let mut rename = false;
        let mut delete = false;
        let mut sort_title = false;
        let mut sort_artist = false;
        let mut play = None;
        let mut move_entry = None;
        let mut remove = None;
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.rename_value);
            rename = ui.button("Rename").clicked();
            if ui.button("Delete…").clicked() { self.confirm_delete = true; }
        });
        if self.confirm_delete {
            ui.horizontal(|ui| {
                ui.label("Delete this playlist?");
                delete = ui.button("Delete").clicked();
                if ui.button("Cancel").clicked() { self.confirm_delete = false; }
            });
        }
        ui.horizontal(|ui| {
            if !self.entries.is_empty() && ui.button("Play playlist").clicked() { play = Some(0); }
            sort_title = ui.button("Sort by title").clicked();
            sort_artist = ui.button("Sort by artist").clicked();
        });
        ui.label(format!("{} entries · use ↑ and ↓ to reorder", self.entries.len()));
        let entries = self.entries.clone();
        egui::ScrollArea::vertical().id_salt("playlist_entries").show(ui, |ui| {
            for (index, entry) in entries.iter().enumerate() {
                let track = self.track(&entry.track_id);
                let track_available = track.is_some();
                let availability = match track {
                    None => "Unavailable",
                    Some(track) if !track.source.is_file() => "File missing",
                    _ => "",
                };
                ui.horizontal(|ui| {
                    if ui.button("▶").clicked() { play = Some(index); }
                    if ui.add_enabled(index > 0, egui::Button::new("↑")).clicked() { move_entry = Some((index, index - 1)); }
                    if ui.add_enabled(index + 1 < entries.len(), egui::Button::new("↓")).clicked() { move_entry = Some((index, index + 1)); }
                    let title = format!("{} — {}", entry.title, display_name(&entry.artist, "Unknown artist"));
                    let response = ui.selectable_label(self.selected_track.as_ref() == Some(&entry.track_id), title);
                    if response.clicked() {
                        self.selected_track = Some(entry.track_id.clone());
                    }
                    response.context_menu(|ui| {
                        if ui.button("Remove from playlist").clicked() {
                            remove = Some(entry.id.clone());
                            ui.close();
                        }
                        if track_available && ui.button("Remove from library…").clicked() {
                            self.confirm_remove_track = Some(entry.track_id.clone());
                            ui.close();
                        }
                    });
                    if !availability.is_empty() { ui.small(availability); }
                });
            }
        });
        if rename {
            let name = self.rename_value.trim();
            if name.is_empty() {
                self.status = "Playlist name cannot be blank".to_string();
            } else {
                match self.library.rename_playlist(&playlist_id, name) {
                    Ok(()) => self.refresh_playlists(),
                    Err(error) => self.status = format!("Cannot rename playlist: {error}"),
                }
            }
        }
        if delete {
            match self.library.delete_playlist(&playlist_id) {
                Ok(()) => {
                    self.selected_playlist = None;
                    self.entries.clear();
                    self.confirm_delete = false;
                    self.refresh_playlists();
                }
                Err(error) => self.status = format!("Cannot delete playlist: {error}"),
            }
        } else if let Some(id) = remove {
            match self.library.remove_entry(&playlist_id, &id) {
                Ok(()) => self.reload_entries(),
                Err(error) => self.status = format!("Cannot remove entry: {error}"),
            }
        } else if let Some((from, to)) = move_entry {
            let mut ids: Vec<String> = entries.iter().map(|entry| entry.id.clone()).collect();
            ids.swap(from, to);
            self.save_order(&playlist_id, ids);
        } else if sort_title || sort_artist {
            let mut sorted = entries.clone();
            if sort_title {
                sorted.sort_by_key(|entry| (entry.title.to_lowercase(), entry.artist.to_lowercase()));
            } else {
                sorted.sort_by_key(|entry| (entry.artist.to_lowercase(), entry.title.to_lowercase()));
            }
            self.save_order(&playlist_id, sorted.into_iter().map(|entry| entry.id).collect());
        }
        if let Some(index) = play {
            self.start_queue(entries.into_iter().map(|entry| entry.track_id).collect(), index);
        }
    }

    fn save_order(&mut self, playlist_id: &str, ids: Vec<String>) {
        match self.library.reorder(playlist_id, &ids) {
            Ok(()) => self.reload_entries(),
            Err(error) => self.status = format!("Cannot reorder playlist: {error}"),
        }
    }

    fn draw_player(&mut self, ui: &mut egui::Ui) {
        let current = self.queue.index.and_then(|index| self.queue.ids.get(index))
            .and_then(|id| self.track(id)).cloned();
        let mut previous = false;
        let mut next = false;
        let mut stop = false;
        ui.horizontal_wrapped(|ui| {
            if let Some(track) = &current {
                ui.strong(&track.title);
                ui.label(format!("— {}", display_name(&track.artist, "Unknown artist")));
            } else {
                ui.label("Nothing playing");
            }
            ui.separator();
            previous = ui.add_enabled(current.is_some(), egui::Button::new("Previous")).clicked();
            if let Some(engine) = &self.audio {
                if ui.add_enabled(current.is_some(), egui::Button::new(if engine.is_paused() { "Resume" } else { "Pause" })).clicked() {
                    engine.pause_or_resume();
                }
            }
            next = ui.add_enabled(current.is_some(), egui::Button::new("Next")).clicked();
            stop = ui.add_enabled(current.is_some(), egui::Button::new("Stop")).clicked();
            if ui.add(egui::Slider::new(&mut self.volume, 0.0..=1.0).text("Volume")).changed() {
                if let Some(engine) = &self.audio { engine.set_volume(self.volume); }
            }
        });
        if let (Some(track), Some(engine)) = (&current, &self.audio) {
            if track.duration_ms > 0 {
                let length = track.duration_ms as f32 / 1000.0;
                let mut position = engine.position().as_secs_f32().min(length);
                ui.horizontal(|ui| {
                    ui.label(duration_text((position * 1000.0) as i64));
                    if ui.add(egui::Slider::new(&mut position, 0.0..=length).show_value(false)).changed() {
                        if let Err(error) = engine.seek(Duration::from_secs_f32(position)) {
                            self.status = format!("Cannot seek: {error}");
                        }
                    }
                    ui.label(duration_text(track.duration_ms));
                });
            }
        }
        if previous { self.previous(); }
        if next { self.next(); }
        if stop {
            if let Some(engine) = &self.audio { engine.stop(); }
            self.queue.index = None;
            self.status = "Stopped".to_string();
        }
    }

    fn draw_settings(&mut self, ui: &mut egui::Ui) {
        ui.heading("Settings");
        ui.label("Interface size");
        let mut scale = self.ui_scale;
        if ui.add(egui::Slider::new(&mut scale, 0.9..=2.0).step_by(0.05).suffix("×")).changed() {
            self.ui_scale = scale;
            ui.ctx().set_zoom_factor(scale);
            if let Err(error) = self.library.set_ui_scale(scale) {
                self.status = format!("Cannot save interface size: {error}");
            }
        }
        if ui.button("Reset to default").clicked() {
            self.ui_scale = 1.2;
            ui.ctx().set_zoom_factor(self.ui_scale);
            if let Err(error) = self.library.set_ui_scale(self.ui_scale) {
                self.status = format!("Cannot save interface size: {error}");
            }
        }
    }

    fn review_window(&mut self, ui: &egui::Ui) {
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

    fn remove_track_window(&mut self, ui: &egui::Ui) {
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

impl eframe::App for MusicApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.receive_scan();
        if self.incoming.is_some() || self.queue.index.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(200));
        }
        if self.queue.index.is_some() && self.audio.as_ref().is_some_and(|engine| engine.is_empty()) {
            self.next();
        }
        egui::Panel::bottom("now_playing").show(ui, |ui| self.draw_player(ui));
        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.heading("Music Library");
                ui.separator();
                for (view, label) in [(View::Library, "Library"), (View::Artists, "Artists"), (View::Playlists, "Playlists"), (View::Settings, "Settings")] {
                    if ui.selectable_label(self.view == view, label).clicked() { self.view = view; }
                }
                ui.separator();
                if ui.add_enabled(self.incoming.is_none() && self.review.is_empty(), egui::Button::new("Import files")).clicked() {
                    self.begin_import();
                }
                if ui.add_enabled(self.selected_track.is_some(), egui::Button::new("Play selected")).clicked() {
                    self.play_selected();
                }
            });
            ui.separator();
            match self.view {
                View::Library => self.draw_library(ui),
                View::Artists => self.draw_artists(ui),
                View::Playlists => self.draw_playlists(ui),
                View::Settings => self.draw_settings(ui),
            }
            if !self.status.is_empty() {
                ui.separator();
                ui.label(&self.status);
            }
        });
        self.review_window(ui);
        self.remove_track_window(ui);
    }
}

fn main() -> eframe::Result {
    let data_dir = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    let path = data_dir.join("MusicLibrary").join("library.sqlite3");
    let library = match Library::open(&path) {
        Ok(library) => library,
        Err(error) => {
            eprintln!("Cannot open library database at {}: {error}", path.display());
            std::process::exit(1);
        }
    };
    eframe::run_native(
        "Music Library",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1150.0, 720.0])
                .with_min_inner_size([850.0, 520.0]),
            ..Default::default()
        },
        Box::new(move |creation_context| Ok(Box::new(MusicApp::new(library, &creation_context.egui_ctx)))),
    )
}

#[cfg(test)]
mod tests {
    use super::artist_suggestions;

    #[test]
    fn suggests_existing_artists_by_prefix() {
        let artists = vec!["ABBA".to_string(), "Adele".to_string(), "Muse".to_string()];
        assert_eq!(artist_suggestions(&artists, " ad"), vec!["Adele"]);
        assert!(artist_suggestions(&artists, "adele").is_empty());
        assert!(artist_suggestions(&artists, "").is_empty());
    }
}

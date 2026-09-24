mod audio;
mod importer;
mod library;

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use eframe::egui;
use rand::seq::SliceRandom;
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
    Queue,
    Settings,
    Curation,
}

#[derive(Default)]
enum QueueSource {
    Library,
    Playlist { id: String, name: String },
    Album(String),
    #[default]
    Single,
}

#[derive(Default)]
struct Queue {
    ids: Vec<String>,
    index: Option<usize>,
    source: QueueSource,
    source_ids: Vec<String>,
    shuffle: bool,
    repeat_playlist: bool,
}

impl Queue {
    fn upcoming_start(&self) -> usize {
        self.index.map_or(0, |index| index + 1)
    }

    fn shuffle_upcoming(&mut self) {
        let start = self.upcoming_start();
        self.ids[start..].shuffle(&mut rand::rng());
    }

    fn insert(&mut self, id: String, next: bool) {
        let index = if next { self.upcoming_start() } else { self.ids.len() };
        self.ids.insert(index, id);
    }

    fn restart_playlist(&mut self) -> bool {
        if !self.repeat_playlist || !matches!(&self.source, QueueSource::Playlist { .. }) || self.source_ids.is_empty() {
            return false;
        }
        self.ids = self.source_ids.clone();
        self.index = None;
        if self.shuffle { self.shuffle_upcoming(); }
        true
    }
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
    playlist_search: String,
    curation_search: String,
    pending_playlist_add: Option<(String, String)>,
    pending_queue_add: Option<(String, bool)>,
    queue_search: String,
    new_playlist_name: String,
    rename_value: String,
    confirm_delete: bool,
    queue: Queue,
    audio: Option<audio::Audio>,
    volume: f32,
    ui_scale: f32,
    footer_height: f32,
    seek_preview: Option<f32>,
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

#[derive(Clone, Copy)]
enum PlayerIcon { Play, Pause, Previous, Next, Stop, Shuffle, Repeat }

fn icon_button(icon: PlayerIcon, selected: bool) -> egui::Button<'static> {
    let (source, label) = match icon {
        PlayerIcon::Play => (egui::include_image!("../assets/play.svg"), "Play"),
        PlayerIcon::Pause => (egui::include_image!("../assets/pause.svg"), "Pause"),
        PlayerIcon::Previous => (egui::include_image!("../assets/previous.svg"), "Previous"),
        PlayerIcon::Next => (egui::include_image!("../assets/next.svg"), "Next"),
        PlayerIcon::Stop => (egui::include_image!("../assets/stop.svg"), "Stop"),
        PlayerIcon::Shuffle => (egui::include_image!("../assets/shuffle.svg"), "Shuffle"),
        PlayerIcon::Repeat => (egui::include_image!("../assets/repeat.svg"), "Repeat playlist"),
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
        egui::include_image!("../assets/up.svg")
    } else {
        egui::include_image!("../assets/down.svg")
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

fn matches_track(track: &Track, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty() || format!("{} {} {}", track.title, track.artist, track.album)
        .to_lowercase().contains(&query)
}

impl MusicApp {
    fn new(library: Library, ctx: &egui::Context) -> Self {
        let tracks = library.tracks().unwrap_or_default();
        let playlists = library.playlists().unwrap_or_default();
        let ui_scale = library.ui_scale().unwrap_or(1.2);
        let footer_height = library.footer_height().unwrap_or(190.0);
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
            playlist_search: String::new(),
            curation_search: String::new(),
            pending_playlist_add: None,
            pending_queue_add: None,
            queue_search: String::new(),
            new_playlist_name: String::new(),
            rename_value: String::new(),
            confirm_delete: false,
            queue: Queue::default(),
            audio: None,
            volume: 0.8,
            ui_scale,
            footer_height,
            seek_preview: None,
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

    fn begin_manifest_import(&mut self) {
        if self.incoming.is_some() || !self.review.is_empty() {
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .add_filter("JSON manifest", &["json"])
            .pick_file()
        else {
            return;
        };
        let (sender, receiver) = mpsc::channel();
        self.incoming = Some(receiver);
        self.status = "Scanning manifest…".to_string();
        std::thread::spawn(move || {
            let result = match importer::scan_manifest(&path) {
                Ok((candidates, errors)) => ScanResult { candidates, errors },
                Err(error) => ScanResult {
                    candidates: Vec::new(),
                    errors: vec![format!("{}: {error}", path.display())],
                },
            };
            let _ = sender.send(result);
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
            Ok(playlists) => self.playlists = playlists,
            Err(error) => self.status = format!("Cannot read playlists: {error}"),
        }
    }

    fn choose_playlist(&mut self, id: String) {
        self.selected_playlist = Some(id.clone());
        self.rename_value = self.playlists.iter().find(|item| item.id == id).map(|item| item.name.clone()).unwrap_or_default();
        self.playlist_search.clear();
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

    fn add_track_to_playlist(&mut self, playlist_id: &str, track_id: &str) {
        let Some(track) = self.track(track_id).cloned() else {
            self.status = "Track no longer exists in the library".to_string();
            return;
        };
        match self.library.add_to_playlist(playlist_id, &track) {
            Ok(()) => {
                self.status = format!("Added {} to playlist", track.title);
                if self.selected_playlist.as_deref() == Some(playlist_id) {
                    self.reload_entries();
                }
                self.refresh_playlists();
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
                let old_index = self.queue.index;
                let was_playing = old_index.and_then(|index| self.queue.ids.get(index))
                    .is_some_and(|playing_id| playing_id == id);
                let removed_before = old_index.map_or(0, |index| self.queue.ids[..index].iter()
                    .filter(|queued_id| queued_id.as_str() == id).count());
                self.queue.ids.retain(|queued_id| queued_id != id);
                self.queue.source_ids.retain(|queued_id| queued_id != id);
                self.queue.index = if was_playing { None } else { old_index.map(|index| index - removed_before) };
                if was_playing {
                    if let Some(engine) = &self.audio { engine.stop(); }
                    self.seek_preview = None;
                }
                self.selected_track = None;
                self.status = format!("Removed {title} from the library and playlists. Audio file kept.");
                match self.library.tracks() {
                    Ok(tracks) => self.tracks = tracks,
                    Err(error) => self.status = format!("Track removed, but cannot refresh library: {error}"),
                }
                self.refresh_playlists();
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

    fn start_queue(&mut self, ids: Vec<String>, start: usize, source: QueueSource) {
        if ids.is_empty() {
            self.status = "No tracks to play".to_string();
            return;
        }
        self.queue.source_ids = ids.clone();
        self.queue.ids = ids;
        self.queue.index = None;
        if !matches!(&source, QueueSource::Playlist { .. }) { self.queue.repeat_playlist = false; }
        self.queue.source = source;
        if self.queue.shuffle && start + 1 < self.queue.ids.len() {
            self.queue.ids[start + 1..].shuffle(&mut rand::rng());
        }
        self.play_from(start, true);
    }

    fn play_from(&mut self, start: usize, forward: bool) {
        self.seek_preview = None;
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
            if self.queue.index.is_none() && self.queue.restart_playlist() {
                self.play_from(0, true);
            }
            if self.queue.index.is_none() {
                self.queue.ids.clear();
                self.queue.source_ids.clear();
            }
        }
    }

    fn play_playlist(&mut self, id: &str) {
        let source = self.playlist_source(id);
        match self.library.entries(id) {
            Ok(entries) => self.start_queue(entries.into_iter().map(|entry| entry.track_id).collect(),
                0, source),
            Err(error) => self.status = format!("Cannot play playlist: {error}"),
        }
    }

    fn playlist_source(&self, id: &str) -> QueueSource {
        let name = self.playlists.iter().find(|playlist| playlist.id == id)
            .map(|playlist| playlist.name.clone()).unwrap_or_else(|| "Playlist".to_string());
        QueueSource::Playlist { id: id.to_string(), name }
    }

    fn add_to_queue(&mut self, id: String, next: bool) {
        if self.track(&id).is_none() { return; }
        if self.queue.index.is_none() {
            self.start_queue(vec![id], 0, QueueSource::Single);
        } else {
            self.queue.insert(id, next);
            self.status = if next { "Playing next" } else { "Added to queue" }.to_string();
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
        let (ids, source): (Vec<String>, QueueSource) = match self.view {
            View::Library => (self.filtered_library_ids(), QueueSource::Library),
            View::Artists => (self.album_ids(), self.selected_album.clone().map(QueueSource::Album).unwrap_or_default()),
            View::Playlists => (self.entries.iter().map(|item| item.track_id.clone()).collect(),
                self.selected_playlist.as_ref().map(|id| self.playlist_source(id)).unwrap_or_default()),
            View::Queue | View::Settings | View::Curation => (Vec::new(), QueueSource::Single),
        };
        if let Some(start) = ids.iter().position(|id| id == &selected) {
            self.start_queue(ids, start, source);
        } else {
            self.start_queue(vec![selected], 0, QueueSource::Single);
        }
    }

    fn filtered_library_ids(&self) -> Vec<String> {
        self.tracks.iter().filter(|track| matches_track(track, &self.search))
            .map(|track| track.id.clone()).collect()
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

    fn draw_track_menu(&mut self, ui: &mut egui::Ui, track_id: &str) {
        if ui.button("Play next").clicked() {
            self.pending_queue_add = Some((track_id.to_string(), true));
            ui.close();
        }
        if ui.button("Add to queue").clicked() {
            self.pending_queue_add = Some((track_id.to_string(), false));
            ui.close();
        }
        if self.playlists.is_empty() { return; }
        ui.separator();
        let playlists = self.playlists.clone();
        ui.menu_button("Add to playlist", |ui| {
            for playlist in playlists {
                if ui.button(&playlist.name).clicked() {
                    self.pending_playlist_add = Some((track_id.to_string(), playlist.id));
                    ui.close();
                }
            }
        });
    }

    fn draw_library(&mut self, ui: &mut egui::Ui) {
        let ids = self.filtered_library_ids();
        let mut play = None;
        ui.horizontal(|ui| {
            ui.heading("All tracks");
            if ui.add_enabled(!ids.is_empty(), icon_button(PlayerIcon::Play, false))
                .on_hover_text("Play visible library").clicked() { play = Some(0); }
            ui.label("Search");
            ui.text_edit_singleline(&mut self.search);
        });
        ui.small("Double-click a song to play from here. Right-click for queue and playlist actions.");
        egui::ScrollArea::both().show(ui, |ui| {
            egui::Grid::new("library_tracks").striped(true).num_columns(6).show(ui, |ui| {
                for heading in ["Title", "Artist", "Album", "Length", "Format", "File"] {
                    ui.strong(heading);
                }
                ui.end_row();
                for (index, id) in ids.iter().enumerate() {
                    let Some(track) = self.track(id).cloned() else { continue };
                    let response = ui.selectable_label(self.selected_track.as_ref() == Some(id), &track.title);
                    if response.clicked() {
                        self.selected_track = Some(id.clone());
                    }
                    if response.double_clicked() { play = Some(index); }
                    response.context_menu(|ui| {
                        self.draw_track_menu(ui, id);
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
        if let Some(index) = play { self.start_queue(ids, index, QueueSource::Library); }
    }

    fn draw_artists(&mut self, ui: &mut egui::Ui) {
        ui.heading("Artists");
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
            if columns[2].add_enabled(!ids.is_empty(), icon_button(PlayerIcon::Play, false))
                .on_hover_text("Play album").clicked() { play = Some(0); }
            egui::ScrollArea::vertical().id_salt("album_tracks").show(&mut columns[2], |ui| {
                for (index, id) in ids.iter().enumerate() {
                    let Some(track) = self.track(id).cloned() else { continue };
                    ui.horizontal(|ui| {
                        let response = ui.selectable_label(self.selected_track.as_ref() == Some(id), &track.title);
                        if response.clicked() {
                            self.selected_track = Some(id.clone());
                        }
                        if response.double_clicked() { play = Some(index); }
                        response.context_menu(|ui| {
                            self.draw_track_menu(ui, id);
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
        if let Some(index) = play {
            let source = self.selected_album.clone().map(QueueSource::Album).unwrap_or_default();
            self.start_queue(ids, index, source);
        }
    }

    fn draw_playlists(&mut self, ui: &mut egui::Ui) {
        ui.heading("Playlists");
        let mut create = false;
        let mut choose = None;
        let mut play_playlist = None;
        ui.columns(2, |columns| {
            columns[0].horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.new_playlist_name).hint_text("New playlist name"));
                create = ui.button("Create").clicked();
            });
            egui::ScrollArea::vertical().id_salt("playlists").show(&mut columns[0], |ui| {
                let playlists = self.playlists.clone();
                for item in playlists {
                    ui.horizontal(|ui| {
                        if ui.add(icon_button(PlayerIcon::Play, false))
                            .on_hover_text(format!("Play {}", item.name)).clicked() {
                            play_playlist = Some(item.id.clone());
                        }
                        if ui.selectable_label(self.selected_playlist.as_ref() == Some(&item.id), &item.name).clicked() {
                            choose = Some(item.id.clone());
                        }
                    });
                }
            });
            self.draw_playlist_detail(&mut columns[1]);
        });
        if create { self.create_playlist(); }
        if let Some(id) = choose { self.choose_playlist(id); }
        if let Some(id) = play_playlist { self.play_playlist(&id); }
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
            if ui.add_enabled(!self.entries.is_empty(), icon_button(PlayerIcon::Play, false))
                .on_hover_text("Play playlist").clicked() { play = Some(0); }
            sort_title = ui.button("Sort by title").clicked();
            sort_artist = ui.button("Sort by artist").clicked();
        });
        ui.add(egui::TextEdit::singleline(&mut self.playlist_search).hint_text("Search songs to add: title, artist or album"));
        if !self.playlist_search.trim().is_empty() {
            let results: Vec<Track> = self.tracks.iter()
                .filter(|track| matches_track(track, &self.playlist_search))
                .take(51).cloned().collect();
            if results.is_empty() { ui.label("No matching songs"); }
            egui::ScrollArea::vertical().id_salt("playlist_search_results").max_height(190.0).show(ui, |ui| {
                for track in results.iter().take(50) {
                    ui.horizontal(|ui| {
                        if ui.button("Add").clicked() {
                            self.pending_playlist_add = Some((track.id.clone(), playlist_id.clone()));
                        }
                        ui.label(format!("{} — {} · {}", track.title,
                            display_name(&track.artist, "Unknown artist"),
                            display_name(&track.album, "Unknown album")));
                    });
                }
            });
            if results.len() > 50 { ui.small("Showing the first 50 results. Refine your search."); }
        }
        ui.label(format!("{} entries · use the arrow buttons to reorder", self.entries.len()));
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
                    if ui.add_enabled(index > 0, reorder_button(true)).on_hover_text("Move up").clicked() { move_entry = Some((index, index - 1)); }
                    if ui.add_enabled(index + 1 < entries.len(), reorder_button(false)).on_hover_text("Move down").clicked() { move_entry = Some((index, index + 1)); }
                    let title = format!("{} — {}", entry.title, display_name(&entry.artist, "Unknown artist"));
                    let response = ui.selectable_label(self.selected_track.as_ref() == Some(&entry.track_id), title);
                    if response.clicked() {
                        self.selected_track = Some(entry.track_id.clone());
                    }
                    if response.double_clicked() { play = Some(index); }
                    response.context_menu(|ui| {
                        if ui.button("Remove from playlist").clicked() {
                            remove = Some(entry.id.clone());
                            ui.close();
                        }
                        if track_available { self.draw_track_menu(ui, &entry.track_id); }
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
                Ok(()) => { self.reload_entries(); self.refresh_playlists(); }
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
            let source = self.playlist_source(&playlist_id);
            self.start_queue(entries.into_iter().map(|entry| entry.track_id).collect(), index,
                source);
        }
    }

    fn save_order(&mut self, playlist_id: &str, ids: Vec<String>) {
        match self.library.reorder(playlist_id, &ids) {
            Ok(()) => { self.reload_entries(); self.refresh_playlists(); }
            Err(error) => self.status = format!("Cannot reorder playlist: {error}"),
        }
    }

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

    fn draw_queue(&mut self, ui: &mut egui::Ui) {
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
        if let Some((from, to)) = move_entry { self.queue.ids.swap(from, to); }
        if let Some(index) = remove_entry { self.queue.ids.remove(index); }
    }

    fn draw_player(&mut self, ui: &mut egui::Ui) {
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
                self.queue.shuffle = !self.queue.shuffle;
                if self.queue.shuffle { self.queue.shuffle_upcoming(); }
            }
            let playlist_active = current.is_some() && matches!(&self.queue.source, QueueSource::Playlist { .. });
            if ui.add_enabled(playlist_active, icon_button(PlayerIcon::Repeat, self.queue.repeat_playlist))
                .on_hover_text("Repeat this playlist").clicked() {
                self.queue.repeat_playlist = !self.queue.repeat_playlist;
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
        if stop {
            if let Some(engine) = &self.audio { engine.stop(); }
            self.queue.ids.clear();
            self.queue.source_ids.clear();
            self.queue.index = None;
            self.queue.repeat_playlist = false;
            self.seek_preview = None;
            self.status = "Stopped".to_string();
        }
    }

    fn draw_settings(&mut self, ui: &mut egui::Ui) {
        ui.heading("Settings");
        ui.label("Interface size");
        let mut scale = self.ui_scale;
        if ui.add(egui::Slider::new(&mut scale, 0.9..=2.0).step_by(0.05).suffix("x")).changed() {
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
        ui.separator();
        ui.label("Player footer height");
        let mut height = self.footer_height;
        if ui.add(egui::Slider::new(&mut height, 150.0..=360.0).step_by(5.0).suffix(" pt")).changed() {
            self.footer_height = height;
            if let Err(error) = self.library.set_footer_height(height) {
                self.status = format!("Cannot save player footer height: {error}");
            }
        }
        if ui.button("Reset footer height").clicked() {
            self.footer_height = 190.0;
            if let Err(error) = self.library.set_footer_height(self.footer_height) {
                self.status = format!("Cannot save player footer height: {error}");
            }
        }
    }

    fn draw_curation(&mut self, ui: &mut egui::Ui) {
        ui.heading("Curation");
        ui.label("Remove entries from the library. Audio files stay on disk.");
        ui.add(egui::TextEdit::singleline(&mut self.curation_search).hint_text("Find a track by title, artist or album"));
        let tracks: Vec<Track> = self.tracks.iter()
            .filter(|track| matches_track(track, &self.curation_search)).cloned().collect();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for track in tracks {
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("{} — {} · {}", track.title,
                        display_name(&track.artist, "Unknown artist"),
                        display_name(&track.album, "Unknown album")))
                        .on_hover_text(track.source.display().to_string());
                    if ui.button("Remove…").clicked() {
                        self.confirm_remove_track = Some(track.id.clone());
                    }
                });
            }
        });
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
        Box::new(move |creation_context| {
            egui_extras::install_image_loaders(&creation_context.egui_ctx);
            Ok(Box::new(MusicApp::new(library, &creation_context.egui_ctx)))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::{artist_suggestions, matches_track, Queue, QueueSource, Track};
    use std::path::PathBuf;

    #[test]
    fn suggests_existing_artists_by_prefix() {
        let artists = vec!["ABBA".to_string(), "Adele".to_string(), "Muse".to_string()];
        assert_eq!(artist_suggestions(&artists, " ad"), vec!["Adele"]);
        assert!(artist_suggestions(&artists, "adele").is_empty());
        assert!(artist_suggestions(&artists, "").is_empty());
    }

    #[test]
    fn finds_songs_by_title_artist_or_album() {
        let track = Track {
            id: "one".to_string(), source: PathBuf::new(),
            title: "First Song".to_string(), artist: "Example Artist".to_string(),
            album: "Blue Album".to_string(), duration_ms: 1000, format: "MP3".to_string(),
        };
        for query in ["first", "ARTIST", "blue"] {
            assert!(matches_track(&track, query));
        }
        assert!(!matches_track(&track, "second"));
    }

    #[test]
    fn shuffling_keeps_the_current_song_and_history() {
        let mut queue = Queue {
            ids: ["first", "second", "third", "fourth", "fifth"].map(str::to_string).to_vec(),
            index: Some(1),
            ..Default::default()
        };
        queue.insert("added".to_string(), true);
        assert_eq!(queue.ids[..3].iter().map(String::as_str).collect::<Vec<_>>(),
            ["first", "second", "added"]);
        queue.shuffle_upcoming();
        assert_eq!(queue.ids[..2].iter().map(String::as_str).collect::<Vec<_>>(),
            ["first", "second"]);
        let mut remaining = queue.ids[2..].to_vec();
        remaining.sort();
        assert_eq!(remaining, ["added", "fifth", "fourth", "third"].map(str::to_string));
    }

    #[test]
    fn repeating_a_playlist_restarts_its_songs_without_one_off_additions() {
        let source = vec!["one".to_string(), "two".to_string()];
        let mut queue = Queue {
            ids: source.clone(),
            index: Some(1),
            source: QueueSource::Playlist { id: "playlist-id".to_string(), name: "List".to_string() },
            source_ids: source.clone(),
            repeat_playlist: true,
            ..Default::default()
        };
        queue.insert("extra".to_string(), false);
        assert_eq!(queue.ids, ["one", "two", "extra"].map(str::to_string));
        assert!(queue.restart_playlist());
        assert_eq!(queue.ids, source);
        assert_eq!(queue.index, None);
    }
}

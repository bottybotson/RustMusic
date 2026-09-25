mod queue;
mod ui;

use std::collections::BTreeSet;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use eframe::egui;

use crate::audio;
use crate::importer::{self, Candidate};
use crate::library::{AddResult, Library, Playlist, PlaylistEntry, Track};
use queue::{Queue, QueueSource};

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

pub(crate) struct MusicApp {
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

fn matches_track(track: &Track, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty() || format!("{} {} {}", track.title, track.artist, track.album)
        .to_lowercase().contains(&query)
}

impl MusicApp {
    pub(crate) fn new(library: Library, ctx: &egui::Context) -> Self {
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
                if self.queue.remove_track(id) {
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
        self.queue.start(ids, start, source);
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
                self.queue.clear();
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

    fn save_order(&mut self, playlist_id: &str, ids: Vec<String>) {
        match self.library.reorder(playlist_id, &ids) {
            Ok(()) => { self.reload_entries(); self.refresh_playlists(); }
            Err(error) => self.status = format!("Cannot reorder playlist: {error}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::matches_track;
    use crate::library::Track;
    use std::path::PathBuf;

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
}

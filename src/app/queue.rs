use rand::seq::SliceRandom;

#[derive(Default)]
pub(super) enum QueueSource {
    Library,
    Playlist { id: String, name: String },
    Album(String),
    #[default]
    Single,
}

#[derive(Default)]
pub(super) struct Queue {
    pub(super) ids: Vec<String>,
    pub(super) index: Option<usize>,
    pub(super) source: QueueSource,
    pub(super) source_ids: Vec<String>,
    pub(super) shuffle: bool,
    pub(super) repeat_playlist: bool,
}

impl Queue {
    pub(super) fn start(&mut self, ids: Vec<String>, start: usize, source: QueueSource) {
        self.source_ids = ids.clone();
        self.ids = ids;
        self.index = None;
        if !matches!(&source, QueueSource::Playlist { .. }) {
            self.repeat_playlist = false;
        }
        self.source = source;
        if self.shuffle && start + 1 < self.ids.len() {
            self.ids[start + 1..].shuffle(&mut rand::rng());
        }
    }

    pub(super) fn upcoming_start(&self) -> usize {
        self.index.map_or(0, |index| index + 1)
    }

    pub(super) fn remove_track(&mut self, id: &str) -> bool {
        let old_index = self.index;
        let was_playing = old_index.and_then(|index| self.ids.get(index))
            .is_some_and(|playing_id| playing_id == id);
        let removed_before = old_index.map_or(0, |index| self.ids[..index].iter()
            .filter(|queued_id| queued_id.as_str() == id).count());
        self.ids.retain(|queued_id| queued_id != id);
        self.source_ids.retain(|queued_id| queued_id != id);
        self.index = if was_playing { None } else { old_index.map(|index| index - removed_before) };
        was_playing
    }

    pub(super) fn clear(&mut self) {
        self.ids.clear();
        self.source_ids.clear();
        self.index = None;
        self.repeat_playlist = false;
        self.source = QueueSource::Single;
    }

    pub(super) fn shuffle_upcoming(&mut self) {
        let start = self.upcoming_start();
        self.ids[start..].shuffle(&mut rand::rng());
    }

    pub(super) fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;
        if self.shuffle {
            self.shuffle_upcoming();
        }
    }

    pub(super) fn toggle_repeat_playlist(&mut self) {
        if matches!(&self.source, QueueSource::Playlist { .. }) {
            self.repeat_playlist = !self.repeat_playlist;
        }
    }

    pub(super) fn move_upcoming(&mut self, from: usize, to: usize) {
        if from >= self.upcoming_start() && to >= self.upcoming_start()
            && from < self.ids.len() && to < self.ids.len() {
            self.ids.swap(from, to);
        }
    }

    pub(super) fn remove_upcoming(&mut self, index: usize) {
        if index >= self.upcoming_start() && index < self.ids.len() {
            self.ids.remove(index);
        }
    }

    pub(super) fn insert(&mut self, id: String, next: bool) {
        let index = if next { self.upcoming_start() } else { self.ids.len() };
        self.ids.insert(index, id);
    }

    pub(super) fn restart_playlist(&mut self) -> bool {
        if !self.repeat_playlist || !matches!(&self.source, QueueSource::Playlist { .. }) || self.source_ids.is_empty() {
            return false;
        }
        self.ids = self.source_ids.clone();
        self.index = None;
        if self.shuffle { self.shuffle_upcoming(); }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{Queue, QueueSource};

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

    #[test]
    fn removing_repeated_track_preserves_current_position() {
        let mut queue = Queue {
            ids: ["one", "gone", "two", "gone", "three"].map(str::to_string).to_vec(),
            source_ids: ["one", "gone", "two"].map(str::to_string).to_vec(),
            index: Some(4),
            ..Default::default()
        };
        assert!(!queue.remove_track("gone"));
        assert_eq!(queue.ids, ["one", "two", "three"].map(str::to_string));
        assert_eq!(queue.source_ids, ["one", "two"].map(str::to_string));
        assert_eq!(queue.index, Some(2));
        assert!(queue.remove_track("three"));
        assert_eq!(queue.index, None);
    }

    #[test]
    fn starting_an_album_disables_playlist_repeat() {
        let mut queue = Queue { repeat_playlist: true, ..Default::default() };
        queue.start(vec!["one".to_string()], 0, QueueSource::Album("Album".to_string()));
        assert!(!queue.repeat_playlist);
        queue.toggle_repeat_playlist();
        assert!(!queue.repeat_playlist);
        assert_eq!(queue.source_ids, queue.ids);
        queue.clear();
        assert!(queue.ids.is_empty());
        assert!(matches!(queue.source, QueueSource::Single));
    }

    #[test]
    fn editing_upcoming_cannot_change_history_or_current_track() {
        let mut queue = Queue {
            ids: ["past", "current", "next"].map(str::to_string).to_vec(),
            index: Some(1),
            ..Default::default()
        };
        queue.move_upcoming(1, 2);
        queue.remove_upcoming(1);
        assert_eq!(queue.ids, ["past", "current", "next"].map(str::to_string));
        queue.remove_upcoming(2);
        assert_eq!(queue.ids, ["past", "current"].map(str::to_string));
    }
}

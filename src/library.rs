use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use crate::importer::Candidate;

#[derive(Clone)]
pub struct Track {
    pub id: String,
    pub source: PathBuf,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: i64,
    pub format: String,
}

#[derive(Clone)]
pub struct Playlist {
    pub id: String,
    pub name: String,
}

#[derive(Clone)]
pub struct PlaylistEntry {
    pub id: String,
    pub track_id: String,
    pub title: String,
    pub artist: String,
}

pub struct Library {
    connection: Connection,
}

pub enum AddResult {
    Added,
    Relinked,
    Duplicate,
}

fn now_millis() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as i64
}

impl Library {
    pub fn open(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(path)?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS settings (
                 key TEXT PRIMARY KEY,
                 value TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS tracks (
                 id TEXT PRIMARY KEY,
                 content_hash TEXT NOT NULL UNIQUE,
                 source TEXT NOT NULL,
                 title TEXT NOT NULL,
                 artist TEXT NOT NULL,
                 album TEXT NOT NULL,
                 duration_ms INTEGER NOT NULL,
                 format TEXT NOT NULL,
                 revision INTEGER NOT NULL DEFAULT 1,
                 added_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );
             CREATE TABLE IF NOT EXISTS playlists (
                 id TEXT PRIMARY KEY,
                 name TEXT NOT NULL,
                 revision INTEGER NOT NULL DEFAULT 1,
                 modified_at INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE IF NOT EXISTS playlist_entries (
                 id TEXT PRIMARY KEY,
                 playlist_id TEXT NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
                 track_id TEXT NOT NULL,
                 fallback_title TEXT NOT NULL,
                 fallback_artist TEXT NOT NULL,
                 position INTEGER NOT NULL
             );",
        )?;
        let columns: Vec<String> = connection.prepare("PRAGMA table_info(playlists)")?
            .query_map([], |row| row.get(1))?
            .collect::<rusqlite::Result<_>>()?;
        if !columns.iter().any(|column| column == "modified_at") {
            connection.execute("ALTER TABLE playlists ADD COLUMN modified_at INTEGER NOT NULL DEFAULT 0", [])?;
        }
        connection.execute(
            "INSERT OR IGNORE INTO settings (key, value) VALUES ('library_id', ?1)",
            [Uuid::new_v4().to_string()],
        )?;
        Ok(Self { connection })
    }

    pub fn tracks(&self) -> rusqlite::Result<Vec<Track>> {
        let mut statement = self.connection.prepare(
            "SELECT id, source, title, artist, album, duration_ms, format
             FROM tracks ORDER BY title COLLATE NOCASE, artist COLLATE NOCASE",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(Track {
                id: row.get(0)?,
                source: PathBuf::from(row.get::<_, String>(1)?),
                title: row.get(2)?,
                artist: row.get(3)?,
                album: row.get(4)?,
                duration_ms: row.get(5)?,
                format: row.get(6)?,
            })
        })?;
        rows.collect()
    }

    pub fn ui_scale(&self) -> rusqlite::Result<f32> {
        let value: Option<String> = self.connection.query_row(
            "SELECT value FROM settings WHERE key = 'ui_scale'",
            [],
            |row| row.get(0),
        ).optional()?;
        Ok(value.and_then(|value| value.parse::<f32>().ok())
            .filter(|value| value.is_finite())
            .unwrap_or(1.2)
            .clamp(0.9, 2.0))
    }

    pub fn set_ui_scale(&self, scale: f32) -> rusqlite::Result<()> {
        self.connection.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('ui_scale', ?1)",
            [scale.to_string()],
        )?;
        Ok(())
    }

    pub fn footer_height(&self) -> rusqlite::Result<f32> {
        let value: Option<String> = self.connection.query_row(
            "SELECT value FROM settings WHERE key = 'footer_height'",
            [],
            |row| row.get(0),
        ).optional()?;
        Ok(value.and_then(|value| value.parse::<f32>().ok())
            .filter(|value| value.is_finite())
            .unwrap_or(160.0)
            .clamp(105.0, 320.0))
    }

    pub fn set_footer_height(&self, height: f32) -> rusqlite::Result<()> {
        self.connection.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('footer_height', ?1)",
            [height.to_string()],
        )?;
        Ok(())
    }

    pub fn add(&self, candidate: &Candidate) -> rusqlite::Result<AddResult> {
        let source = candidate.source.to_string_lossy().to_string();
        let inserted = self.connection.execute(
            "INSERT OR IGNORE INTO tracks
             (id, content_hash, source, title, artist, album, duration_ms, format)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                Uuid::new_v4().to_string(),
                candidate.content_hash,
                source,
                candidate.title.trim(),
                candidate.artist.trim(),
                candidate.album.trim(),
                candidate.duration_ms,
                candidate.format,
            ],
        )?;
        if inserted > 0 {
            return Ok(AddResult::Added);
        }

        let previous: String = self.connection.query_row(
            "SELECT source FROM tracks WHERE content_hash = ?1",
            [&candidate.content_hash],
            |row| row.get(0),
        )?;
        if !Path::new(&previous).is_file() {
            self.connection.execute(
                "UPDATE tracks SET source = ?1 WHERE content_hash = ?2",
                params![source, candidate.content_hash],
            )?;
            return Ok(AddResult::Relinked);
        }
        Ok(AddResult::Duplicate)
    }

    pub fn remove_track(&self, track_id: &str) -> rusqlite::Result<bool> {
        let transaction = self.connection.unchecked_transaction()?;
        let removed = transaction.execute("DELETE FROM tracks WHERE id = ?1", [track_id])?;
        if removed == 0 {
            return Ok(false);
        }
        transaction.execute(
            "UPDATE playlists SET revision = revision + 1,
             modified_at = MAX(?2, COALESCE((SELECT MAX(modified_at) + 1 FROM playlists), 0))
             WHERE id IN (SELECT playlist_id FROM playlist_entries WHERE track_id = ?1)",
            params![track_id, now_millis()],
        )?;
        transaction.execute("DELETE FROM playlist_entries WHERE track_id = ?1", [track_id])?;
        transaction.commit()?;
        Ok(true)
    }

    pub fn playlists(&self) -> rusqlite::Result<Vec<Playlist>> {
        let mut statement = self.connection.prepare(
            "SELECT id, name FROM playlists ORDER BY modified_at DESC, name COLLATE NOCASE, id",
        )?;
        let rows = statement.query_map([], |row| Ok(Playlist { id: row.get(0)?, name: row.get(1)? }))?;
        rows.collect()
    }

    pub fn entries(&self, playlist_id: &str) -> rusqlite::Result<Vec<PlaylistEntry>> {
        let mut statement = self.connection.prepare(
            "SELECT e.id, e.track_id, COALESCE(t.title, e.fallback_title),
                    COALESCE(t.artist, e.fallback_artist)
             FROM playlist_entries e LEFT JOIN tracks t ON t.id = e.track_id
             WHERE e.playlist_id = ?1 ORDER BY e.position, e.id",
        )?;
        let rows = statement.query_map([playlist_id], |row| {
            Ok(PlaylistEntry {
                id: row.get(0)?,
                track_id: row.get(1)?,
                title: row.get(2)?,
                artist: row.get(3)?,
            })
        })?;
        rows.collect()
    }

    pub fn create_playlist(&self, name: &str) -> rusqlite::Result<String> {
        let id = Uuid::new_v4().to_string();
        self.connection.execute(
            "INSERT INTO playlists (id, name, modified_at)
             VALUES (?1, ?2, MAX(?3, COALESCE((SELECT MAX(modified_at) + 1 FROM playlists), 0)))",
            params![&id, name, now_millis()],
        )?;
        Ok(id)
    }

    pub fn rename_playlist(&self, id: &str, name: &str) -> rusqlite::Result<()> {
        self.connection.execute(
            "UPDATE playlists SET name = ?1, revision = revision + 1,
             modified_at = MAX(?3, COALESCE((SELECT MAX(modified_at) + 1 FROM playlists), 0))
             WHERE id = ?2",
            params![name, id, now_millis()],
        )?;
        Ok(())
    }

    pub fn delete_playlist(&self, id: &str) -> rusqlite::Result<()> {
        self.connection.execute("DELETE FROM playlists WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn add_to_playlist(&self, playlist_id: &str, track: &Track) -> rusqlite::Result<()> {
        let tx = self.connection.unchecked_transaction()?;
        let position: i64 = tx.query_row(
            "SELECT COALESCE(MAX(position) + 1, 0) FROM playlist_entries WHERE playlist_id = ?1",
            [playlist_id],
            |row| row.get(0),
        )?;
        tx.execute(
            "INSERT INTO playlist_entries
             (id, playlist_id, track_id, fallback_title, fallback_artist, position)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![Uuid::new_v4().to_string(), playlist_id, track.id, track.title, track.artist, position],
        )?;
        tx.execute(
            "UPDATE playlists SET revision = revision + 1,
             modified_at = MAX(?2, COALESCE((SELECT MAX(modified_at) + 1 FROM playlists), 0))
             WHERE id = ?1",
            params![playlist_id, now_millis()],
        )?;
        tx.commit()
    }

    pub fn remove_entry(&self, playlist_id: &str, entry_id: &str) -> rusqlite::Result<()> {
        let tx = self.connection.unchecked_transaction()?;
        tx.execute("DELETE FROM playlist_entries WHERE id = ?1 AND playlist_id = ?2", params![entry_id, playlist_id])?;
        tx.execute(
            "UPDATE playlists SET revision = revision + 1,
             modified_at = MAX(?2, COALESCE((SELECT MAX(modified_at) + 1 FROM playlists), 0))
             WHERE id = ?1",
            params![playlist_id, now_millis()],
        )?;
        tx.commit()
    }

    pub fn reorder(&self, playlist_id: &str, ordered_ids: &[String]) -> rusqlite::Result<()> {
        let tx = self.connection.unchecked_transaction()?;
        for (position, id) in ordered_ids.iter().enumerate() {
            tx.execute(
                "UPDATE playlist_entries SET position = ?1 WHERE id = ?2 AND playlist_id = ?3",
                params![position as i64, id, playlist_id],
            )?;
        }
        tx.execute(
            "UPDATE playlists SET revision = revision + 1,
             modified_at = MAX(?2, COALESCE((SELECT MAX(modified_at) + 1 FROM playlists), 0))
             WHERE id = ?1",
            params![playlist_id, now_millis()],
        )?;
        tx.commit()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_playlists_and_orders_recent_changes_first() {
        let path = std::env::temp_dir().join(format!("music-library-playlists-{}.sqlite3", Uuid::new_v4()));
        let legacy = Connection::open(&path).unwrap();
        legacy.execute_batch(
            "CREATE TABLE playlists (id TEXT PRIMARY KEY, name TEXT NOT NULL, revision INTEGER NOT NULL DEFAULT 1);
             INSERT INTO playlists (id, name) VALUES ('old-id', 'Old');",
        ).unwrap();
        drop(legacy);

        let library = Library::open(&path).unwrap();
        let recent_id = library.create_playlist("Recent").unwrap();
        assert_eq!(library.playlists().unwrap()[0].id, recent_id);
        library.rename_playlist("old-id", "Updated").unwrap();
        assert_eq!(library.playlists().unwrap()[0].id, "old-id");

        let candidate = Candidate {
            source: PathBuf::from("missing-file.mp3"), content_hash: "order-hash".to_string(),
            title: "Song".to_string(), artist: "Artist".to_string(),
            album: "Album".to_string(), duration_ms: 1000, format: "MP3",
            missing_title: false, missing_artist: false,
        };
        library.add(&candidate).unwrap();
        let track = library.tracks().unwrap().remove(0);
        library.add_to_playlist(&recent_id, &track).unwrap();
        assert_eq!(library.playlists().unwrap()[0].id, recent_id);
        drop(library);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn ui_scale_persists_between_launches() {
        let path = std::env::temp_dir().join(format!("music-library-scale-{}.sqlite3", Uuid::new_v4()));
        let library = Library::open(&path).unwrap();
        assert_eq!(library.ui_scale().unwrap(), 1.2);
        library.set_ui_scale(1.5).unwrap();
        drop(library);
        let reopened = Library::open(&path).unwrap();
        assert_eq!(reopened.ui_scale().unwrap(), 1.5);
        drop(reopened);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn footer_height_persists_between_launches() {
        let path = std::env::temp_dir().join(format!("music-library-footer-{}.sqlite3", Uuid::new_v4()));
        let library = Library::open(&path).unwrap();
        assert_eq!(library.footer_height().unwrap(), 160.0);
        library.set_footer_height(210.0).unwrap();
        drop(library);
        let reopened = Library::open(&path).unwrap();
        assert_eq!(reopened.footer_height().unwrap(), 210.0);
        drop(reopened);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn removing_track_clears_its_playlist_entries_without_deleting_audio() {
        let folder = std::env::temp_dir().join(format!("music-library-remove-{}", Uuid::new_v4()));
        fs::create_dir_all(&folder).unwrap();
        let path = folder.join("library.sqlite3");
        let source = folder.join("sample.mp3");
        fs::write(&source, b"audio file stays").unwrap();
        let library = Library::open(&path).unwrap();
        let candidate = Candidate {
            source: source.clone(), content_hash: "hash-one".to_string(),
            title: "One".to_string(), artist: "Artist".to_string(),
            album: "Album".to_string(), duration_ms: 1000, format: "MP3",
            missing_title: false, missing_artist: false,
        };
        library.add(&candidate).unwrap();
        let track = library.tracks().unwrap().remove(0);
        let playlist_id = library.create_playlist("List").unwrap();
        library.add_to_playlist(&playlist_id, &track).unwrap();
        library.add_to_playlist(&playlist_id, &track).unwrap();

        assert!(library.remove_track(&track.id).unwrap());
        assert!(!library.remove_track(&track.id).unwrap());
        assert!(library.tracks().unwrap().is_empty());
        assert!(library.entries(&playlist_id).unwrap().is_empty());
        assert!(source.is_file());
        drop(library);
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn migrates_library_and_preserves_unavailable_playlist_entries() {
        let path = std::env::temp_dir().join(format!("music-library-{}.sqlite3", Uuid::new_v4()));
        {
            let legacy = Connection::open(&path).unwrap();
            legacy.execute_batch(
                "CREATE TABLE tracks (
                    id TEXT PRIMARY KEY, content_hash TEXT NOT NULL UNIQUE,
                    source TEXT NOT NULL, title TEXT NOT NULL, artist TEXT NOT NULL,
                    album TEXT NOT NULL, duration_ms INTEGER NOT NULL,
                    format TEXT NOT NULL, revision INTEGER NOT NULL DEFAULT 1,
                    added_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );",
            ).unwrap();
        }

        let library = Library::open(&path).unwrap();
        let candidate = Candidate {
            source: PathBuf::from("missing-file.mp3"),
            content_hash: "sample-hash".to_string(),
            title: "One".to_string(),
            artist: "Artist".to_string(),
            album: "Album".to_string(),
            duration_ms: 1000,
            format: "MP3",
            missing_title: false,
            missing_artist: false,
        };
        assert!(matches!(library.add(&candidate).unwrap(), AddResult::Added));
        let track = library.tracks().unwrap().remove(0);
        let playlist_id = library.create_playlist("Favourites").unwrap();
        library.add_to_playlist(&playlist_id, &track).unwrap();
        library.add_to_playlist(&playlist_id, &track).unwrap();
        let before = library.entries(&playlist_id).unwrap();
        assert_ne!(before[0].id, before[1].id);
        library.reorder(&playlist_id, &[before[1].id.clone(), before[0].id.clone()]).unwrap();
        assert_eq!(library.entries(&playlist_id).unwrap()[0].id, before[1].id);
        library.connection.execute("DELETE FROM tracks WHERE id = ?1", [&track.id]).unwrap();
        drop(library);

        let reopened = Library::open(&path).unwrap();
        let entries = reopened.entries(&playlist_id).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].track_id, track.id);
        assert_eq!(entries[0].title, "One");
        reopened.delete_playlist(&playlist_id).unwrap();
        assert!(reopened.entries(&playlist_id).unwrap().is_empty());
        drop(reopened);
        fs::remove_file(path).unwrap();
    }
}

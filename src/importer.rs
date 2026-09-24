use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use lofty::file::{AudioFile, FileType, TaggedFileExt};
use lofty::probe::Probe;
use lofty::tag::Accessor;
use sha2::{Digest, Sha256};

pub struct Candidate {
    pub source: PathBuf,
    pub content_hash: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: i64,
    pub format: &'static str,
    pub missing_title: bool,
    pub missing_artist: bool,
}

fn supported_format(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "mp3" => Some("MP3"),
        "flac" => Some("FLAC"),
        "wav" => Some("WAV"),
        "ogg" | "oga" => Some("Ogg/Vorbis"),
        _ => None,
    }
}

pub fn scan(path: &Path) -> Result<Candidate, String> {
    let format = supported_format(path).ok_or("Unsupported file extension")?;
    let source = path.canonicalize().map_err(|error| error.to_string())?;
    let tagged = Probe::open(&source)
        .map_err(|error| error.to_string())?
        .guess_file_type()
        .map_err(|error| error.to_string())?
        .read()
        .map_err(|error| error.to_string())?;
    if !matches!(
        (format, tagged.file_type()),
        ("MP3", FileType::Mpeg)
            | ("FLAC", FileType::Flac)
            | ("WAV", FileType::Wav)
            | ("Ogg/Vorbis", FileType::Vorbis)
    ) {
        return Err("Audio format does not match the file extension".to_string());
    }
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
    let title = tag.and_then(|tag| tag.title()).map(|v| v.into_owned()).unwrap_or_default();
    let artist = tag.and_then(|tag| tag.artist()).map(|v| v.into_owned()).unwrap_or_default();
    let album = tag.and_then(|tag| tag.album()).map(|v| v.into_owned()).unwrap_or_default();
    let duration_ms = i64::try_from(tagged.properties().duration().as_millis()).unwrap_or(i64::MAX);

    let mut hasher = Sha256::new();
    let mut file = BufReader::new(File::open(&source).map_err(|error| error.to_string())?);
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }

    let missing_title = title.trim().is_empty();
    let missing_artist = artist.trim().is_empty();
    Ok(Candidate {
        source: source.clone(),
        content_hash: format!("{:x}", hasher.finalize()),
        title: if missing_title {
            source.file_stem().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
        } else {
            title
        },
        artist,
        album,
        duration_ms,
        format,
        missing_title,
        missing_artist,
    })
}

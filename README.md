# Music Library

Rust desktop player for Windows and Linux. It imports MP3, FLAC, WAV and Ogg/Vorbis files, lets you edit missing metadata, browse tracks by artist and album, manage playlists, and play a queue with a persistent now-playing bar.

## Code layout

- `src/main.rs` opens the database and starts the desktop app.
- `src/app.rs` owns app state and coordinates imports, playlists, and playback.
- `src/app/queue.rs` keeps queue ordering, source, shuffle, and repeat rules together.
- `src/app/ui.rs` dispatches views; `src/app/ui/` contains library and playlist views, playback controls, and dialogs.
- `src/library.rs`, `src/importer.rs`, and `src/audio.rs` handle SQLite storage, audio-file scanning, and output respectively.

The UI calls app actions, which use the library and audio modules. No UI framework types are needed in the queue, library, importer, or audio modules.

Run `cargo test` to check the app, including the headless playlist UI test built with `egui_kittest`. GitHub Actions skips the UI test on pull requests and runs the full suite after a merge updates `main`.

## Build on Linux Mint 22.3

These instructions target the current main Linux Mint release (Cinnamon, MATE or Xfce), not LMDE. You need an internet connection for the first build because Cargo downloads Rust packages.

1. Install the native build tools and libraries. Open a terminal and run:

   ```sh
   sudo apt update
   sudo apt install build-essential pkg-config libasound2-dev \
     libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
     libxkbcommon-dev libssl-dev curl ca-certificates unzip zenity
   ```

2. Install the current stable Rust toolchain using the [official rustup installer](https://rustup.rs/). Choose the default installation when prompted:

   ```sh
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   . "$HOME/.cargo/env"
   rustc --version
   cargo --version
   ```

3. Clone the repository and enter the project directory:

   ```sh
   git clone https://github.com/bottybotson/RustMusic.git
   cd RustMusic
   ```

4. Check the build, then compile and start the player:

   ```sh
   cargo check
   cargo run --release
   ```

   The first build may take several minutes. Later, from the same project directory, you can start the compiled program directly with:

   ```sh
   ./target/release/music-library
   ```

In the window, choose **Import files**, select audio files, edit any missing metadata, then choose **Import**. Typing an artist name offers matching artists already in your library. The **Library**, **Artists**, **Playlists**, **Settings**, and **Curation** tabs provide the views and controls.

## Import MP3s with a JSON manifest

Put a `music-library-import.json` file beside your MP3s (subdirectories are supported). Choose **Import manifest**, select the JSON file, review the titles, artists and albums, then choose **Import**. Paths are relative to the manifest and must stay inside its directory. The app reads metadata from MP3 tags when a field is omitted. It keeps the audio files in place, so keep the folder after importing.

```json
{
  "version": 1,
  "tracks": [
    { "file": "my-video.mp3", "title": "My Song", "artist": "My Artist", "album": "My Videos" }
  ]
}
```

For your own private YouTube videos, one batch workflow is to use [yt-dlp](https://github.com/yt-dlp/yt-dlp) and ffmpeg externally to save audio and video metadata together. From the desired output directory, run (replace `firefox` with your signed-in browser and the URL with your video or playlist URL):

```sh
yt-dlp --cookies-from-browser firefox -x --audio-format mp3 --write-info-json -o '%(id)s.%(ext)s' 'VIDEO_OR_PLAYLIST_URL'
python3 /path/to/RustMusic/tools/manifest_from_ytdlp.py .
```

The script pairs each `<video-id>.info.json` with `<video-id>.mp3`, writes `music-library-import.json`, and skips sidecars without an MP3. Edit the manifest to correct titles, artists or albums before importing. Keep sidecars and browser credentials private; the app itself neither contacts YouTube nor reads the sidecars. On Windows Command Prompt, use `"%(id)s.%(ext)s"` for the output template and `python` for the script.

- In **Library**, search or select a track. Use the play button beside **All tracks** to play the visible list, or double-click a song to start from it. Right-click a track title for **Play next**, **Add to queue**, or **Add to playlist**. The most recently changed playlists appear first in the playlist submenu.
- To remove a track from the library, open **Curation**, find the track, choose **Remove…**, and confirm. Its entries in playlists and the playback queue are removed too. The audio file is left on disk; importing the file again adds it back.
- In **Artists**, choose an artist and album to see their tracks. Use the play button above the album's tracks to play them in title order, or double-click a song to start from it.
- In **Playlists**, create or select a playlist, search for songs by title, artist or album, and click **Add** beside a result. A play button beside each playlist starts it directly. You can also right-click tracks in Library, Artists or a playlist to add them to another playlist. A playlist can be renamed, deleted, sorted by title or artist, and manually reordered with the arrow buttons. Right-click an entry to remove it from that playlist. A track can appear more than once. Double-click a song to start from that point in the playlist.
- In **Queue**, search for songs to play next or append, see what is playing, reorder upcoming songs with the arrows, or remove them. Queue edits affect the current listening session and leave the saved playlist alone.
- The bottom bar prominently shows the source of playback (Library, playlist or album) above the current track. A single track played on its own has no source heading; adding songs to it makes a **Custom queue**. Adding a song to a playlist queue keeps the playlist name. Matching icon buttons control previous, pause/resume, next, stop, shuffle and playlist repeat. Shuffle randomizes the upcoming songs without moving the current song or history. Playlist repeat restarts the source playlist after the remaining queue plays; one-off songs added to the queue play once. Dragging the position slider previews the time and seeks when released. Missing files and unresolved playlist entries stay visible; playback skips them.
- In **Settings**, adjust the interface size (0.9x to 2x) and the player footer height, or reset either to its default. Both choices persist across launches.

## If something fails

- `cargo: command not found`: Run `. "$HOME/.cargo/env"` in the current terminal, or open a new terminal.
- `alsa-sys` or `alsa.pc` errors during compilation: Check that `libasound2-dev` and `pkg-config` were installed in step 1.
- The import file picker does not open: Install a compatible desktop portal backend with `sudo apt install xdg-desktop-portal-gtk` and log out and back in. The app uses the XDG Desktop Portal file picker on Linux.
- A build error in this project's Rust source: Save the full `cargo check` output. The repository's Linux workflow also runs `cargo test` on each push.

The database is normally at `~/.local/share/MusicLibrary/library.sqlite3` on Mint, or under `$XDG_DATA_HOME/MusicLibrary` if you have set that variable. Audio files stay in their original locations. If one moves, it is shown as missing until you import an identical copy.

Snapshot export/import, Android and EQ are not implemented. Playback currently uses Rodio; the stored track IDs, file hashes, revisions and library ID allow later transfer features.

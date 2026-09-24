# Music Library

Rust desktop player for Windows and Linux. It imports MP3, FLAC, WAV and Ogg/Vorbis files, lets you edit missing metadata, browse tracks by artist and album, manage playlists, and play a queue with a persistent now-playing bar.

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

- In **Library**, search or select a track. Use **Play selected** or its play icon button to play through the visible list in order. Right-click a track title for **Add to playlist**, then choose a playlist from the submenu. The most recently changed playlists appear first.
- To remove a track from the library, open **Curation**, find the track, choose **Remove…**, and confirm. Its entries in playlists and the playback queue are removed too. The audio file is left on disk; importing the file again adds it back.
- In **Artists**, choose an artist and album to see their tracks. Use **Play album** or a track's play icon button to play that album in title order.
- In **Playlists**, create or select a playlist, search for songs by title, artist or album, and click **Add** beside a result. You can also right-click tracks in Library, Artists or a playlist to add them to another playlist. A playlist can be renamed, deleted, sorted by title or artist, and manually reordered with the up and down arrow buttons. Right-click an entry to remove it from that playlist. A track can appear more than once. Use **Play playlist** to play its entries in order.
- The bottom bar prominently shows the source of playback (Library, playlist or album) above the current track; a single track played on its own has no source heading. It provides previous, pause/resume, next, stop, position seeking, and volume. Dragging the position slider previews the time and seeks when released. Missing files and unresolved playlist entries stay visible; playback skips them.
- In **Settings**, adjust the interface size (0.9x to 2x) and the player footer height, or reset either to its default. Both choices persist across launches.

## If something fails

- `cargo: command not found`: Run `. "$HOME/.cargo/env"` in the current terminal, or open a new terminal.
- `alsa-sys` or `alsa.pc` errors during compilation: Check that `libasound2-dev` and `pkg-config` were installed in step 1.
- The import file picker does not open: Install a compatible desktop portal backend with `sudo apt install xdg-desktop-portal-gtk` and log out and back in. The app uses the XDG Desktop Portal file picker on Linux.
- A build error in this project's Rust source: Save the full `cargo check` output. The repository's Linux workflow also runs `cargo test` on each push.

The database is normally at `~/.local/share/MusicLibrary/library.sqlite3` on Mint, or under `$XDG_DATA_HOME/MusicLibrary` if you have set that variable. Audio files stay in their original locations. If one moves, it is shown as missing until you import an identical copy.

Snapshot export/import, Android, shuffle/repeat and EQ are not implemented. Playback currently uses Rodio; the stored track IDs, file hashes, revisions and library ID allow later transfer features.

use super::{display_name, duration_text, icon_button, matches_track, reorder_button, MusicApp, PlayerIcon};
use super::{egui, QueueSource, Track};

impl MusicApp {
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

    pub(super) fn draw_library(&mut self, ui: &mut egui::Ui) {
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

    pub(super) fn draw_artists(&mut self, ui: &mut egui::Ui) {
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

    pub(super) fn draw_playlists(&mut self, ui: &mut egui::Ui) {
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

    pub(super) fn draw_settings(&mut self, ui: &mut egui::Ui) {
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

    pub(super) fn draw_curation(&mut self, ui: &mut egui::Ui) {
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


}

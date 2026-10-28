//! UI-thread state: models, callbacks, and applying core events.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use client_core::library::SortOrder;
use client_core::{
    AudioDevice, Command, Commander, Event, Layout, ListQuery, ListSort, LoginFailure,
    QueuePosition, Repeat, Rgba, RowKind, Settings, SignOutReason, SleepTimer, TrackSource, View,
    ViewHeader, ViewRow, format,
};
use slint::{
    ComponentHandle, Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, Timer, VecModel,
};

use crate::artwork::{ArtworkCache, HEADER_PX};
use crate::rows::{Pane, Rows};
use crate::{AppWindow, LyricLine, QueueRow, Toolbar};

/// Sort labels of the sort menu.
fn sort_label(sort: ListSort) -> &'static str {
    match sort {
        ListSort::Added => "最近添加",
        ListSort::Title => "名称",
        ListSort::RecentlyPlayed => "最近播放",
        ListSort::ReleaseDate => "发行日期",
        ListSort::Year => "发行年份",
        ListSort::Album => "专辑",
        ListSort::Plays => "播放次数",
        ListSort::Duration => "时长",
        ListSort::Name => "名称",
        ListSort::AlbumCount => "专辑数",
        ListSort::TrackCount => "歌曲数",
        ListSort::Random => "随机",
    }
}

/// The list behind the current view (its sort and direction).
struct ListState {
    view: View,
    /// Sort keys on this server, in menu order.
    sorts: Vec<ListSort>,
    sort: ListSort,
    desc: bool,
}

impl Default for ListState {
    fn default() -> Self {
        Self {
            view: View::Albums,
            sorts: Vec::new(),
            sort: ListSort::Added,
            desc: true,
        }
    }
}

impl ListState {
    /// Adopts the header of a loaded page. The server reports the sort and
    /// direction it actually used, so both are taken from the header: the UI
    /// never re-derives them (a list without a sort menu still remembers the
    /// direction it asked for).
    fn reset(&mut self, header: &ViewHeader) {
        self.view = header.view.clone();
        self.sorts.clone_from(&header.toolbar.sort_options);
        // A sort that is not offered on this server (a browse-only key on a
        // legacy server) falls back to the first option.
        self.sort = if self.sorts.contains(&header.toolbar.sort) {
            header.toolbar.sort
        } else if header.toolbar.sort_options.is_empty() {
            self.sort
        } else {
            header.toolbar.sort_options[0]
        };
        self.desc = header.toolbar.desc;
    }

    /// The query for the current sort / direction, keeping the filters.
    /// Playlists have no favorites, so the switch is dropped for them.
    fn query(&self, filters: &Filters) -> ListQuery {
        let kind = self
            .view
            .list_kind()
            .unwrap_or(client_core::ListKind::Albums);
        ListQuery {
            kind,
            sort: self.sort,
            order: if self.desc {
                SortOrder::Desc
            } else {
                SortOrder::Asc
            },
            search: filters.search.clone(),
            year_from: filters.year_from,
            year_to: filters.year_to,
            favorite: filters.favorite && !matches!(kind, client_core::ListKind::Playlists),
            ..ListQuery::default()
        }
    }
}

/// What the search and year fields ask for.
#[derive(Default, Clone)]
struct Filters {
    search: String,
    year_from: Option<i32>,
    year_to: Option<i32>,
    favorite: bool,
}

impl Filters {
    fn reset(&mut self, toolbar: &client_core::ViewToolbar) {
        self.search.clone_from(&toolbar.search);
        self.year_from = toolbar.year_from;
        self.year_to = toolbar.year_to;
        self.favorite = toolbar.favorite;
    }
}

/// The first sort of a list starts in its natural direction (recent / counts
/// first, names ascending).
fn default_desc(sorts: &[ListSort]) -> bool {
    matches!(
        sorts.first(),
        Some(
            ListSort::Added
                | ListSort::Plays
                | ListSort::RecentlyPlayed
                | ListSort::AlbumCount
                | ListSort::TrackCount
        )
    )
}

struct Ui {
    window: slint::Weak<AppWindow>,
    commander: Commander,
    /// All decoded covers (the rows hold it too; here for the player bar).
    artwork: Rc<ArtworkCache>,
    /// The rows behind both models (built on demand, so only visible rows
    /// request artwork).
    pane: Pane,
    lyrics: Rc<VecModel<LyricLine>>,
    queue: Rc<VecModel<QueueRow>>,
    /// Artwork path of the playing track (for its cover).
    now_artwork: RefCell<String>,
    notice_timer: Timer,
    /// Engine names behind the device combo box; index 0 (`None`) is the
    /// system default.
    device_names: RefCell<Vec<Option<String>>>,
    selected_device: RefCell<Option<String>>,
    /// The view shown (sort, direction, kind).
    list: RefCell<ListState>,
    grid: RefCell<bool>,
    /// The header artwork (album / artist / playlist cover).
    header_artwork: RefCell<String>,
    filters: RefCell<Filters>,
    playlists: RefCell<Vec<(i64, String)>>,
}

impl Ui {
    fn device_index(&self) -> i32 {
        let selected = self.selected_device.borrow();
        self.device_names
            .borrow()
            .iter()
            .position(|n| *n == *selected)
            .and_then(|i| i32::try_from(i).ok())
            .unwrap_or(0)
    }

    /// Fills the device combo box; a saved device that is not connected
    /// stays listed so the selection is not silently lost.
    fn set_devices(&self, w: &AppWindow, devices: Vec<AudioDevice>) {
        let mut names = vec![None];
        let mut labels: Vec<SharedString> = vec!["系统默认".into()];
        for d in devices.into_iter().filter(|d| d.name != "auto") {
            let label = if d.description.is_empty() {
                d.name.clone()
            } else {
                d.description
            };
            labels.push(label.into());
            names.push(Some(d.name));
        }
        if let Some(selected) = self.selected_device.borrow().clone()
            && !names.contains(&Some(selected.clone()))
        {
            labels.push(format!("{selected}（未连接）").into());
            names.push(Some(selected));
        }
        *self.device_names.borrow_mut() = names;
        w.set_audio_devices(ModelRc::new(VecModel::from(labels)));
        w.set_audio_device_index(self.device_index());
    }

    /// Highlights the sidebar item of `view`.
    fn set_navigation(&self, w: &AppWindow, view: &View) {
        let name = match view {
            View::Albums => "albums",
            View::Tracks => "tracks",
            View::Artists => "artists",
            View::Playlists => "playlists",
            View::FavoriteAlbums => "favorite-albums",
            View::FavoriteTracks => "favorite-tracks",
            _ => "",
        };
        w.set_current_view(name.into());
        w.set_nav_albums(name == "albums");
        w.set_nav_tracks(name == "tracks");
        w.set_nav_artists(name == "artists");
        w.set_nav_playlists(name == "playlists");
        w.set_nav_favorite_albums(name == "favorite-albums");
        w.set_nav_favorite_tracks(name == "favorite-tracks");
    }

    /// Applies a header (title, layout, toolbar) to the window.
    fn set_header(&self, w: &AppWindow, header: &ViewHeader) {
        self.list.borrow_mut().reset(header);
        let grid = header.layout == Layout::Grid;
        *self.grid.borrow_mut() = grid;
        w.set_grid(grid);
        // The inactive model is not drawn; refresh the one that is.
        self.pane.reset(grid);
        w.set_view_title(header.title.clone().into());
        w.set_view_subtitle(header.subtitle.clone().into());
        w.set_view_details(header.details.join(" · ").into());
        w.set_has_view_cover(!header.artwork.is_empty());
        *self.header_artwork.borrow_mut() = header.artwork.clone();
        let artwork = self.pane.rows.artwork();
        let cover = artwork
            .image(&header.artwork, HEADER_PX)
            .unwrap_or_default();
        w.set_view_cover(cover);
        if !header.artwork.is_empty()
            && artwork.image(&header.artwork, HEADER_PX).is_none()
            && artwork.claim(&header.artwork, HEADER_PX)
        {
            self.commander.send(Command::LoadImage {
                key: client_core::artwork_keys::image_key(&header.artwork, HEADER_PX),
                path: header.artwork.clone(),
                px: HEADER_PX,
            });
        }
        w.set_view_loaded(i32::try_from(header.loaded).unwrap_or(i32::MAX));
        w.set_view_total(i32::try_from(header.total).unwrap_or(i32::MAX));
        w.set_can_play(header.can_play);
        w.set_editable(header.editable);
        // The random-play button belongs to the track lists of the library.
        w.set_shuffle_all_visible(matches!(header.view, View::Tracks | View::FavoriteTracks));
        w.set_editing_playlist(match header.view {
            View::Playlist(id) => int(id),
            _ => 0,
        });
        w.set_rename_playlist_name(header.title.clone().into());
        let state = self.list.borrow();
        let labels: Vec<SharedString> = state
            .sorts
            .iter()
            .map(|s| SharedString::from(sort_label(*s)))
            .collect();
        let sort_index = state
            .sorts
            .iter()
            .position(|s| *s == state.sort)
            .and_then(|i| i32::try_from(i).ok())
            .unwrap_or(0);
        w.set_toolbar(Toolbar {
            sorts: ModelRc::new(VecModel::from(labels)),
            sort_index,
            desc: state.desc,
            search: header.toolbar.search.clone().into(),
            year_from: header.toolbar.year_from.unwrap_or(0),
            year_to: header.toolbar.year_to.unwrap_or(0),
            favorite: i32::from(header.toolbar.favorite),
            filterable: header.toolbar.filterable,
        });
        self.set_navigation(w, &header.view);
    }

    fn query(&self) -> ListQuery {
        self.list.borrow().query(&self.filters.borrow())
    }

    fn set_view(&self, w: &AppWindow, view: &View) {
        self.set_navigation(w, view);
    }
}

fn int(value: i64) -> i32 {
    i32::try_from(value).unwrap_or(if value < 0 { i32::MIN } else { i32::MAX })
}

thread_local! {
    static UI: RefCell<Option<Rc<Ui>>> = const { RefCell::new(None) };
}

fn with_ui(f: impl FnOnce(&Ui, &AppWindow)) {
    let ui = UI.with(|cell| cell.borrow().clone());
    if let Some(ui) = ui
        && let Some(window) = ui.window.upgrade()
    {
        f(&ui, &window);
    }
}

/// Wires callbacks and models; `settings` are the saved ones.
pub fn install(window: &AppWindow, commander: Commander, settings: &Settings) {
    #[allow(clippy::cast_possible_truncation)]
    window.set_volume(settings.volume as f32);
    window.set_show_lyrics(settings.show_lyrics);
    window.set_exclusive(settings.exclusive);
    window.set_close_to_tray(settings.close_to_tray);
    window.set_repeat(repeat_index(settings.repeat));
    window.set_shuffle(settings.shuffle);

    let lyrics = Rc::new(VecModel::<LyricLine>::default());
    let queue = Rc::new(VecModel::<QueueRow>::default());
    window.set_lyrics(ModelRc::from(lyrics.clone()));
    window.set_queue(ModelRc::from(queue.clone()));

    let c = commander.clone();
    let weak = window.as_weak();
    window.on_login(move |server, api_token, media_token| {
        if let Some(w) = weak.upgrade() {
            w.set_login_busy(true);
            w.set_login_error(SharedString::new());
        }
        c.send(Command::Login {
            server_url: server.trim().to_owned(),
            api_token: api_token.trim().to_owned(),
            media_token: media_token.trim().to_owned(),
        });
    });
    let c = commander.clone();
    window.on_logout(move || c.send(Command::Logout));

    let c = commander.clone();
    let weak = window.as_weak();
    window.on_navigate(move |name| {
        if let Some(w) = weak.upgrade() {
            w.set_show_settings(false);
        }
        c.send(Command::Navigate(view_from_name(&name)));
    });
    let c = commander.clone();
    let weak = window.as_weak();
    window.on_search(move |text| {
        let text = text.trim().to_owned();
        if text.is_empty() {
            return;
        }
        if let Some(w) = weak.upgrade() {
            w.set_show_settings(false);
        }
        c.send(Command::Search(text));
    });
    let weak = window.as_weak();
    window.on_layout_changed(move |columns, card_size| {
        let scale = weak.upgrade().map_or(1.0, |w| w.window().scale_factor());
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let px = (card_size * scale).ceil().max(0.0) as u32;
        with_ui(|ui, _| {
            let columns = usize::try_from(columns).unwrap_or(1).max(1);
            let resized = ui.pane.rows.artwork().set_cover_px(px);
            let reflowed = ui.pane.rows.columns.get() != columns;
            ui.pane.rows.columns.set(columns);
            if resized || reflowed {
                ui.pane.reset(*ui.grid.borrow());
            }
        });
    });
    // Clicks, favorites and queueing resolve through the row itself: a mixed
    // page (search, artist) has album / artist / track rows side by side, and
    // only a track row has an index into the view's track list.
    let c = commander.clone();
    window.on_row_clicked(move |index| {
        if let Some(row) = row_at(index) {
            c.send(row.open_command(usize::try_from(index).unwrap_or(0)));
        }
    });
    let c = commander.clone();
    window.on_row_play(move |index| {
        if let Some(row) = row_at(index) {
            c.send(row.play_command(usize::try_from(index).unwrap_or(0)));
        }
    });
    let c = commander.clone();
    window.on_row_enqueue(move |index, next| {
        if let Some(row) = row_at(index) {
            c.send(Command::Enqueue {
                source: row.enqueue_source(),
                at: position(next),
            });
        }
    });
    let c = commander.clone();
    window.on_row_favorite(move |index, favorite| {
        let Some(row) = row_at(index) else { return };
        // Playlists cannot be favorited: the menu item is only shown for the
        // other row kinds.
        let Some(kind) = row.kind.favorite_kind() else {
            return;
        };
        c.send(Command::SetFavorite {
            kind,
            id: row.id,
            favorite,
        });
    });
    let c = commander.clone();
    window.on_row_remove(move |index| {
        let Some(row) = row_at(index) else { return };
        with_ui(|ui, _| {
            if let View::Playlist(id) = ui.list.borrow().view.clone() {
                c.send(Command::RemoveFromPlaylist {
                    playlist_id: id,
                    track_ids: vec![row.id],
                });
            }
        });
    });
    let c = commander.clone();
    window.on_row_add_to_playlist(move |index, playlist| {
        let Some(row) = row_at(index) else { return };
        // An album card adds the whole album, not a track whose id happens to
        // equal the album id.
        let (track_ids, album_id) = match row.kind {
            RowKind::Track => (vec![row.id], None),
            RowKind::Album => (Vec::new(), Some(row.id)),
            // Artists and playlists are not queueable units here.
            RowKind::Artist | RowKind::Playlist => return,
        };
        with_ui(|ui, _| {
            let Some((playlist_id, _)) = ui
                .playlists
                .borrow()
                .get(usize::try_from(playlist).unwrap_or(usize::MAX))
                .cloned()
            else {
                return;
            };
            c.send(Command::AddToPlaylist {
                playlist_id,
                track_ids,
                album_id,
            });
        });
    });
    let c = commander.clone();
    window.on_row_moved(move |from, to| {
        with_ui(|ui, _| {
            let (Ok(from), Ok(to)) = (usize::try_from(from), usize::try_from(to)) else {
                return;
            };
            let length = ui.pane.rows.data.borrow().len();
            if from >= length || from == to {
                return;
            }
            if let View::Playlist(id) = ui.list.borrow().view.clone() {
                c.send(Command::MovePlaylistItem {
                    playlist_id: id,
                    from,
                    to: to.min(length - 1),
                });
            }
        });
    });
    let c2 = commander.clone();
    window.on_rename_playlist(move |id, name| {
        let name = name.trim().to_owned();
        if name.is_empty() || id <= 0 {
            return;
        }
        // The description is kept by the core (it holds the open playlist).
        c2.send(Command::RenamePlaylist {
            id: i64::from(id),
            name,
            description: String::new(),
        });
    });
    let c = commander.clone();
    window.on_delete_playlist(move |id| {
        if id > 0 {
            c.send(Command::DeletePlaylist(i64::from(id)));
        }
    });
    let c = commander.clone();
    let weak = window.as_weak();
    window.on_upload_artwork(move |id| {
        if id <= 0 {
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .add_filter("图片", &["jpg", "jpeg", "png", "webp"])
            .pick_file()
        else {
            return;
        };
        if let Some(w) = weak.upgrade() {
            w.set_notice("正在上传封面…".into());
        }
        c.send(Command::UploadPlaylistArtwork {
            playlist_id: i64::from(id),
            path,
        });
    });
    let c = commander.clone();
    window.on_delete_artwork(move |id| {
        if id > 0 {
            c.send(Command::DeletePlaylistArtwork(i64::from(id)));
        }
    });
    let c = commander.clone();
    window.on_shuffle_all(move || c.send(Command::ShuffleAll));
    let c = commander.clone();
    window.on_play_all(move || c.send(Command::PlayView));
    let c = commander.clone();
    window.on_enqueue_all(move |next| {
        c.send(Command::Enqueue {
            source: TrackSource::View,
            at: position(next),
        });
    });
    let c = commander.clone();
    let weak = window.as_weak();
    window.on_sort_picked(move |index| {
        with_ui(|ui, w| {
            let Some(sort) = ui
                .list
                .borrow()
                .sorts
                .get(usize::try_from(index).unwrap_or(usize::MAX))
                .copied()
            else {
                return;
            };
            ui.list.borrow_mut().sort = sort;
            ui.list.borrow_mut().desc = default_desc(&[sort]);
            w.set_toolbar(Toolbar {
                sort_index: index,
                desc: ui.list.borrow().desc,
                ..w.get_toolbar()
            });
            let _ = &weak;
            c.send(Command::ListQuery { query: ui.query() });
        });
    });
    let c = commander.clone();
    window.on_direction_toggled(move || {
        with_ui(|ui, w| {
            let desc = !ui.list.borrow().desc;
            ui.list.borrow_mut().desc = desc;
            w.set_toolbar(Toolbar {
                desc,
                ..w.get_toolbar()
            });
            c.send(Command::ListQuery { query: ui.query() });
        });
    });
    let c = commander.clone();
    let weak = window.as_weak();
    window.on_search_submitted(move |text| {
        let text = text.trim().to_owned();
        if text.is_empty() {
            return;
        }
        with_ui(|ui, _| ui.filters.borrow_mut().search.clone_from(&text));
        if let Some(w) = weak.upgrade() {
            w.set_search_text(text.clone().into());
        }
        c.send(Command::Search(text));
    });
    let c = commander.clone();
    window.on_year_range(move |from, to| {
        with_ui(|ui, _| {
            let mut filters = ui.filters.borrow_mut();
            filters.year_from = (from > 0).then_some(from);
            filters.year_to = (to > 0).then_some(to);
            drop(filters);
            if ui.list.borrow().view.is_list() {
                c.send(Command::ListQuery { query: ui.query() });
            }
        });
    });
    let c = commander.clone();
    let weak = window.as_weak();
    window.on_favorite_toggled(move |on| {
        if let Some(w) = weak.upgrade() {
            with_ui(|ui, _| {
                ui.filters.borrow_mut().favorite = on;
                w.set_toolbar(Toolbar {
                    favorite: i32::from(on),
                    ..w.get_toolbar()
                });
                if ui.list.borrow().view.is_list() {
                    c.send(Command::ListQuery { query: ui.query() });
                }
            });
        }
    });
    let c = commander.clone();
    let weak = window.as_weak();
    window.on_show_similar(move |index| {
        with_ui(|ui, _| {
            if let Some(row) = ui
                .pane
                .rows
                .data
                .borrow()
                .get(usize::try_from(index).unwrap_or(usize::MAX))
            {
                let _ = &weak;
                c.send(Command::Navigate(View::Similar(row.id)));
            }
        });
    });
    let c = commander.clone();
    window.on_load_more(move || c.send(Command::LoadMore));
    let c = commander.clone();
    window.on_add_view_to_playlist(move |playlist_id| {
        with_ui(|ui, _| {
            let track_ids: Vec<i64> = ui
                .pane
                .rows
                .data
                .borrow()
                .iter()
                .filter(|r| r.play.is_some())
                .map(|r| r.id)
                .collect();
            if !track_ids.is_empty() {
                c.send(Command::AddToPlaylist {
                    playlist_id: i64::from(playlist_id),
                    track_ids,
                    album_id: None,
                });
            }
        });
    });
    let c = commander.clone();
    window.on_playlists_wanted(move || c.send(Command::RefreshPlaylists));
    let c = commander.clone();
    window.on_refresh_playlists(move || c.send(Command::RefreshPlaylists));
    let c = commander.clone();
    window.on_create_playlist(move |name| {
        let name = name.trim().to_owned();
        if !name.is_empty() {
            c.send(Command::CreatePlaylist {
                name,
                description: String::new(),
            });
        }
    });

    let c = commander.clone();
    window.on_show_lyrics_changed(move |show| c.send(Command::SetShowLyrics(show)));
    let c = commander.clone();
    window.on_close_to_tray_toggled(move |on| c.send(Command::SetCloseToTray(on)));
    let c = commander.clone();
    window.on_settings_opened(move || c.send(Command::RefreshAudioDevices));
    let c = commander.clone();
    let weak = window.as_weak();
    window.on_audio_device_selected(move |index| {
        let name = with_device_name(index);
        match name {
            Some(name) => c.send(Command::SetAudioDevice(name)),
            None => {
                if let Some(w) = weak.upgrade() {
                    w.set_output_busy(false);
                }
            }
        }
    });
    let c = commander.clone();
    window.on_exclusive_toggled(move |on| c.send(Command::SetExclusive(on)));

    let c = commander.clone();
    window.on_play_queue_item(move |uid| {
        if let Ok(uid) = u64::try_from(uid) {
            c.send(Command::PlayQueueItem(uid));
        }
    });
    let c = commander.clone();
    window.on_remove_queue_item(move |uid| {
        if let Ok(uid) = u64::try_from(uid) {
            c.send(Command::RemoveFromQueue(uid));
        }
    });
    let c = commander.clone();
    window.on_set_sleep(move |mode, minutes| {
        let timer = match mode {
            1 => Some(SleepTimer::EndOfTrack),
            2 => Some(SleepTimer::EndOfAlbum),
            3 => u32::try_from(minutes).ok().map(SleepTimer::Minutes),
            _ => None,
        };
        c.send(Command::SetSleepTimer(timer));
    });
    let c = commander.clone();
    window.on_clear_queue(move || c.send(Command::ClearQueue));
    let c = commander.clone();
    let weak = window.as_weak();
    window.on_toggle_shuffle(move || {
        if let Some(w) = weak.upgrade() {
            c.send(Command::SetShuffle(!w.get_shuffle()));
        }
    });
    let c = commander.clone();
    let weak = window.as_weak();
    window.on_cycle_repeat(move || {
        if let Some(w) = weak.upgrade() {
            c.send(Command::SetRepeat(
                repeat_from_index(w.get_repeat()).cycle(),
            ));
        }
    });
    let c = commander.clone();
    window.on_toggle_pause(move || c.send(Command::TogglePause));
    let c = commander.clone();
    window.on_next(move || c.send(Command::Next));
    let c = commander.clone();
    window.on_previous(move || c.send(Command::Previous));
    let c = commander.clone();
    window.on_seek(move |ms| {
        c.send(Command::Seek {
            position_ms: ms.into(),
        })
    });
    let c = commander.clone();
    window.on_volume_changed(move |v| c.send(Command::SetVolume(f64::from(v))));

    let artwork = Rc::new(ArtworkCache::default());
    let rows = Rows::new(artwork.clone(), commander.clone());
    let pane = Pane::new(rows);
    let (grid_model, list_model) = pane.models();
    window.set_rows(grid_model);
    window.set_lines(list_model);

    let ui = Rc::new(Ui {
        window: window.as_weak(),
        commander,
        artwork,
        pane,
        lyrics,
        queue,
        now_artwork: RefCell::default(),
        notice_timer: Timer::default(),
        device_names: RefCell::new(vec![None]),
        selected_device: RefCell::new(settings.audio_device.clone()),
        list: RefCell::default(),
        grid: RefCell::new(true),
        header_artwork: RefCell::default(),
        filters: RefCell::default(),
        playlists: RefCell::default(),
    });
    UI.with(|cell| *cell.borrow_mut() = Some(ui));
}

fn with_device_name(index: i32) -> Option<Option<String>> {
    UI.with(|cell| {
        let ui = cell.borrow();
        let ui = ui.as_ref()?;
        let names = ui.device_names.borrow();
        Some(
            usize::try_from(index)
                .ok()
                .and_then(|i| names.get(i).cloned())
                .flatten(),
        )
    })
}

/// The `ViewRow` behind a row index of the pane.
fn row_at(index: i32) -> Option<ViewRow> {
    let index = usize::try_from(index).ok()?;
    UI.with(|cell| {
        let ui = cell.borrow();
        ui.as_ref()?.pane.rows.data.borrow().get(index).cloned()
    })
}

fn view_from_name(name: &str) -> View {
    match name {
        "tracks" => View::Tracks,
        "artists" => View::Artists,
        "playlists" => View::Playlists,
        "favorite-albums" => View::FavoriteAlbums,
        "favorite-tracks" => View::FavoriteTracks,
        _ => View::Albums,
    }
}

fn position(next: bool) -> QueuePosition {
    if next {
        QueuePosition::Next
    } else {
        QueuePosition::Last
    }
}

fn repeat_index(repeat: Repeat) -> i32 {
    match repeat {
        Repeat::Off => 0,
        Repeat::All => 1,
        Repeat::One => 2,
    }
}

fn repeat_from_index(index: i32) -> Repeat {
    match index {
        1 => Repeat::All,
        2 => Repeat::One,
        _ => Repeat::Off,
    }
}

fn row_index(index: Option<usize>) -> i32 {
    index.and_then(|i| i32::try_from(i).ok()).unwrap_or(-1)
}

fn to_image(rgba: &Rgba) -> Image {
    let buffer =
        SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&rgba.pixels, rgba.width, rgba.height);
    Image::from_rgba8(buffer)
}

fn login_message(failure: &LoginFailure) -> String {
    match failure {
        LoginFailure::CredentialsRequired => "API Token 和媒体凭据都必须填写".into(),
        LoginFailure::TokensMustDiffer => "API Token 和媒体凭据不能相同".into(),
        LoginFailure::ApiTokenInvalid => "API Token 无效".into(),
        LoginFailure::UnsupportedRevision(r) => {
            format!("服务器 API 版本（revision {r}）与客户端不兼容，需要 revision 3")
        }
        LoginFailure::MediaTokenInvalid => "媒体凭据无效".into(),
        LoginFailure::InvalidUrl => "服务器地址无效".into(),
        LoginFailure::Other(e) => format!("连接失败：{e}"),
    }
}

/// Applies one core event on the UI thread.
pub fn apply(event: Event) {
    with_ui(|ui, w| match event {
        Event::SavedLogin(saved) => {
            if !saved.server_url.is_empty() {
                w.set_login_server(saved.server_url.into());
            }
            w.set_login_api_token_saved(saved.has_api_token);
            w.set_login_media_token_saved(saved.has_media_token);
            if saved.signing_in {
                w.set_login_busy(true);
                w.set_login_error(SharedString::new());
            }
        }
        Event::SignedOut(reason) => {
            w.set_logged_in(false);
            w.set_login_busy(false);
            w.set_login_api_token(SharedString::new());
            w.set_login_media_token(SharedString::new());
            w.set_login_error(match reason {
                SignOutReason::User => SharedString::new(),
                SignOutReason::Expired => "API Token 已失效，请重新输入".into(),
            });
            *ui.now_artwork.borrow_mut() = String::new();
            ui.pane.rows.clear();
            ui.pane.rows.artwork().reset();
        }
        Event::LoginFailed(failure) => {
            w.set_login_busy(false);
            w.set_login_error(login_message(&failure).into());
        }
        Event::LoggedIn { server_url } => {
            w.set_login_busy(false);
            w.set_logged_in(true);
            w.set_show_settings(false);
            w.set_server_label(server_url.into());
        }
        Event::ViewReset { view, header } => {
            // No filters or rows survive a navigation; the placeholder header
            // already carries the query the new view loads with, so a reload
            // triggered by the user's own sort / filter keeps them.
            ui.filters.borrow_mut().reset(&header.toolbar);
            w.set_load_failed(false);
            *ui.grid.borrow_mut() = header.layout == Layout::Grid;
            w.set_grid(header.layout == Layout::Grid);
            ui.pane.rows.clear();
            ui.pane.reset(*ui.grid.borrow());
            ui.pane.rows.artwork().reset();
            *ui.header_artwork.borrow_mut() = header.artwork.clone();
            ui.set_header(w, &header);
            ui.set_view(w, &view);
        }
        Event::ViewHeader(header) => {
            ui.filters.borrow_mut().reset(&header.toolbar);
            w.set_load_failed(false);
            ui.set_header(w, &header);
        }
        Event::ViewRows {
            view,
            rows,
            total,
            loaded,
        } => {
            if ui.list.borrow().view != view {
                return;
            }
            // A retry that worked hides the retry button again.
            w.set_load_failed(false);
            let grid = *ui.grid.borrow();
            let before = ui.pane.rows.data.borrow().len();
            let added = ui.pane.rows.append(rows);
            ui.pane.added(grid, before, added);
            w.set_view_loaded(i32::try_from(loaded).unwrap_or(i32::MAX));
            w.set_view_total(i32::try_from(total).unwrap_or(i32::MAX));
        }
        Event::ViewTracks(_) => {}
        Event::LoadFailed {
            view,
            empty,
            message,
        } => {
            if ui.list.borrow().view != view {
                return;
            }
            // An empty page has nothing to scroll, so the pane offers a retry
            // (`load-more` from the button instead of from a scroll).
            w.set_load_failed(empty);
            let _ = message;
        }
        Event::Playlists(items) => {
            let names: Vec<SharedString> = items
                .iter()
                .map(|p| SharedString::from(p.name.as_str()))
                .collect();
            *ui.playlists.borrow_mut() = items.iter().map(|p| (p.id, p.name.clone())).collect();
            w.set_playlist_names(ModelRc::new(VecModel::from(names)));
        }
        Event::Favorite { kind, id, favorite } => {
            let mut changed = Vec::new();
            for (index, row) in ui.pane.rows.data.borrow_mut().iter_mut().enumerate() {
                // The kind of the row decides: a mixed page (search, artist)
                // shows albums and tracks side by side in one list.
                if row.id == id && row.kind.favorite_kind() == Some(kind) {
                    row.favorite = favorite;
                    changed.push(index);
                }
            }
            ui.pane.changed(*ui.grid.borrow(), &changed);
        }
        Event::Image { key, image } => {
            let image = to_image(&image);
            ui.pane.rows.artwork().store(&key, image.clone());
            let now_artwork = ui.now_artwork.borrow().clone();
            let header_artwork = ui.header_artwork.borrow().clone();
            if !now_artwork.is_empty()
                && key == client_core::artwork_keys::image_key(&now_artwork, HEADER_PX)
            {
                w.set_np_cover(image.clone());
            }
            if !header_artwork.is_empty()
                && key == client_core::artwork_keys::image_key(&header_artwork, HEADER_PX)
            {
                w.set_view_cover(image);
                return;
            }
            // Only the rows showing this artwork are rebuilt: a repaint of
            // every loaded row would ask for the covers the cache evicted.
            if let Some((px, path)) = key.split_once('|') {
                let _ = px;
                let indexes = ui.pane.rows.indexes_of(path);
                ui.pane.changed(*ui.grid.borrow(), &indexes);
            }
        }
        Event::ImageFailed { key } => {
            // Release the claim so the next repaint of that row asks again.
            ui.artwork.release(&key);
        }
        Event::NowPlaying(None) => {
            w.set_has_track(false);
            w.set_playing_id(-1);
            ui.now_artwork.borrow_mut().clear();
        }
        Event::NowPlaying(Some(now)) => {
            w.set_has_track(true);
            w.set_playing_id(int(now.track_id));
            w.set_np_title(now.title.into());
            w.set_np_artist(now.artist.into());
            w.set_np_album(now.album.into());
            w.set_np_quality(now.quality.unwrap_or_default().into());
            *ui.now_artwork.borrow_mut() = now.artwork.clone();
            match ui.artwork.image(&now.artwork, HEADER_PX) {
                Some(cover) => w.set_np_cover(cover),
                None => {
                    if ui.artwork.claim(&now.artwork, HEADER_PX) {
                        ui.commander.send(Command::LoadImage {
                            key: client_core::artwork_keys::image_key(&now.artwork, HEADER_PX),
                            path: now.artwork.clone(),
                            px: HEADER_PX,
                        });
                    }
                    w.set_np_cover(Image::default());
                }
            }
        }
        Event::Position {
            position_ms,
            duration_ms,
        } => {
            w.set_position_ms(int(position_ms));
            w.set_duration_ms(int(duration_ms));
            w.set_position_text(format::duration(position_ms).into());
            w.set_duration_text(format::duration(duration_ms).into());
        }
        Event::Paused(paused) => w.set_paused(paused),
        Event::Buffering(buffering) => w.set_buffering(buffering),
        #[allow(clippy::cast_possible_truncation)]
        Event::Volume(volume) => w.set_volume(volume as f32),
        Event::Lyrics {
            lines,
            synced,
            source,
        } => {
            ui.lyrics.set_vec(
                lines
                    .into_iter()
                    .map(|text| LyricLine { text: text.into() })
                    .collect::<Vec<_>>(),
            );
            w.set_lyrics_synced(synced);
            w.set_lyrics_active(-1);
            w.set_lyrics_source(source.unwrap_or_default().into());
        }
        Event::Queue { items, current } => {
            ui.queue.set_vec(
                items
                    .into_iter()
                    .map(|e| QueueRow {
                        uid: i32::try_from(e.uid).unwrap_or(i32::MAX),
                        title: e.title.into(),
                        artist: e.artist.into(),
                        duration: e.duration.into(),
                    })
                    .collect::<Vec<_>>(),
            );
            w.set_queue_current(row_index(current));
        }
        Event::QueuePosition(current) => w.set_queue_current(row_index(current)),
        Event::Repeat(repeat) => w.set_repeat(repeat_index(repeat)),
        Event::Shuffle(shuffle) => w.set_shuffle(shuffle),
        Event::SleepTimer(status) => {
            w.set_sleep_active(status.is_some());
            let label = match status {
                None => String::new(),
                Some(s) => match (s.timer, s.remaining_ms) {
                    (SleepTimer::EndOfTrack, _) => "本曲".into(),
                    (SleepTimer::EndOfAlbum, _) => "本专辑".into(),
                    (SleepTimer::Minutes(_), remaining) => {
                        format::duration(remaining.unwrap_or(0).max(0))
                    }
                },
            };
            w.set_sleep_label(label.into());
        }
        Event::AudioDevices(devices) => ui.set_devices(w, devices),
        Event::AudioDevice(device) => {
            *ui.selected_device.borrow_mut() = device;
            w.set_audio_device_index(ui.device_index());
            w.set_output_busy(false);
        }
        Event::Exclusive(enabled) => {
            w.set_exclusive(enabled);
            w.set_output_busy(false);
        }
        Event::LyricsActive(active) => {
            w.set_lyrics_active(active.and_then(|i| i32::try_from(i).ok()).unwrap_or(-1));
        }
        Event::Notice(text) => {
            tracing::warn!("{text}");
            w.set_notice(text.into());
            let weak = ui.window.clone();
            ui.notice_timer.start(
                slint::TimerMode::SingleShot,
                Duration::from_secs(5),
                move || {
                    if let Some(w) = weak.upgrade() {
                        w.set_notice(SharedString::new());
                    }
                },
            );
        }
    });
}

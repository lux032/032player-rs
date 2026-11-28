//! Wiring Slint models, initial settings, and window callbacks.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use client_core::{
    ArtworkFit, Command, Commander, DetailView, ExclusiveMode, FavoriteKind, RadioStatus, Repeat,
    RightPanel, Settings, SleepTimer, ThemeMode, TrackSource,
};
use slint::{ComponentHandle, ModelRc, SharedString, Timer, VecModel};

use super::m032::{
    self, GRID_CHRONICLE, GRID_HOME_ALBUMS, GRID_HOME_PLAYLISTS, GRID_HOME_WORKS, GRID_MEMBERS,
    GRID_SERIES, GRID_SERIES_ALBUMS, GRID_WORK_ALBUMS, GRID_WORKS,
};
use super::{
    ALBUM_GRIDS, Dialog, GRID_ALBUMS, GRID_ARTISTS, GRID_FAV_ALBUMS, GRID_FAV_ARTISTS,
    GRID_PLAYLISTS, GRID_SEARCH_ALBUMS, GRID_SEARCH_ARTISTS, GRID_SEARCH_COMPOSERS, GRIDS,
    PAGE_ALBUMS, RELEASE_GRIDS, Route, UI, Ui, cast, exclusive_mode_index, grid_index,
    repeat_from_index, repeat_index, search, shortcuts, track_list, with_ui,
};
use crate::cards::CardGridModel;
use crate::{AppWindow, DialogKind, GridId, Home, Lib032, LyricsSearch, RadioState, TrackRow};

/// Wires callbacks and models; `settings` are the saved ones.
#[allow(clippy::too_many_lines)]
pub fn install(window: &AppWindow, commander: Commander, settings: &Settings) {
    shortcuts::install(window, settings);
    cast::install(window, &commander, settings);
    #[allow(clippy::cast_possible_truncation)]
    window.set_volume(settings.volume as f32);
    let panel = settings.right_panel;
    window.set_panel_open(panel.open);
    window.set_panel_queue_tab(panel.queue_tab);
    window.set_panel_queue(panel.queue);
    window.set_panel_lyrics(panel.lyrics);
    window.set_panel_lyrics_first(panel.lyrics_first);
    window.set_panel_split(panel.split);
    window.set_panel_width(panel.width);
    window.set_sidebar_width(settings.sidebar_width);
    window.set_app_version(env!("CARGO_PKG_VERSION").into());
    window.on_open_logs(|| {
        let dir = crate::log_dir();
        let _ = std::fs::create_dir_all(&dir);
        os::browser::open_path(&dir);
    });
    window.set_exclusive(settings.exclusive);
    window.set_exclusive_mode(exclusive_mode_index(settings.exclusive_mode()));
    window.set_preferred_exclusive_mode(exclusive_mode_index(settings.preferred_exclusive_mode()));
    window.set_asio_available(false);
    window.set_asio_configured(false);
    window.set_asio_fallback(false);
    window.set_wasapi_device_label("系统默认".into());
    window.set_asio_driver_label(SharedString::new());
    window.set_asio_error(false);
    window.set_close_to_tray(settings.close_to_tray);
    window.set_fade_on_pause(settings.fade_on_pause);
    Ui::show_loudness_settings(window, &settings.loudness);
    window.set_theme_mode(match settings.theme {
        ThemeMode::System => 0,
        ThemeMode::Dark => 1,
        ThemeMode::Light => 2,
    });
    window.set_repeat(repeat_index(settings.repeat));
    window.set_shuffle(settings.shuffle);
    let grids: Vec<Rc<CardGridModel>> = (0..GRIDS)
        .map(|_| CardGridModel::new(commander.clone()))
        .collect();
    // Album and playlist grids preserve poster/landscape aspect ratios.
    for grid in ALBUM_GRIDS {
        grids[grid].set_fit(ArtworkFit::Contain);
    }
    for grid in [GRID_PLAYLISTS, GRID_HOME_PLAYLISTS, GRID_SERIES_ALBUMS] {
        grids[grid].set_fit(ArtworkFit::Contain);
    }
    // Non-virtualized `CardRows` inside a Flickable load covers only for rows in view.
    for grid in
        RELEASE_GRIDS
            .into_iter()
            .chain([GRID_WORK_ALBUMS, GRID_MEMBERS, GRID_SERIES_ALBUMS])
    {
        grids[grid].set_lazy(true);
    }
    let new_tracks = || Rc::new(VecModel::<TrackRow>::default());
    let ui = Rc::new(Ui {
        window: window.as_weak(),
        commander: commander.clone(),
        grids,
        tracks: new_tracks(),
        fav_tracks: new_tracks(),
        detail_tracks: new_tracks(),
        search_tracks: new_tracks(),
        search_preview_tracks: new_tracks(),
        search: RefCell::default(),
        played: new_tracks(),
        pane_tracks: new_tracks(),
        artist_favs: new_tracks(),
        artist_credits: new_tracks(),
        artist_dir_role: RefCell::new(None),
        full_cover_key: RefCell::default(),
        home_mixes: Rc::new(VecModel::default()),
        home_recent: Rc::new(VecModel::default()),
        home_resume: Rc::new(VecModel::default()),
        home_songs: Rc::new(VecModel::default()),
        mix_others: Rc::new(VecModel::default()),
        lyrics_results: Rc::new(VecModel::default()),
        m032: RefCell::default(),
        lyrics: Rc::new(VecModel::default()),
        queue: Rc::new(VecModel::default()),
        playlist_choices: Rc::new(VecModel::default()),
        now: RefCell::default(),
        now_cover_key: RefCell::default(),
        now_large_key: RefCell::default(),
        detail_cover_key: RefCell::default(),
        detail: RefCell::default(),
        route: Cell::new(Route::Page(PAGE_ALBUMS)),
        history: RefCell::default(),
        scroll_timer: Timer::default(),
        scroll_restore: Cell::new((0.0, 0.0)),
        scroll_restore_ticks: Cell::new(0),
        scroll_seen: Cell::new((None, None)),
        dialog: Cell::new(None),
        album_artist: RefCell::default(),
        track_artist: RefCell::default(),
        sent: RefCell::default(),
        browse: Cell::new(true),
        transcode: Cell::new(settings.transcode),
        transcode_choices: RefCell::new(vec![None]),
        notice_timer: Timer::default(),
        restore_hint_timer: Timer::default(),
        scale_timer: Timer::default(),
        queue_all: RefCell::default(),
        queue_covers: Rc::default(),
        queue_paths: Rc::default(),
        queue_search: RefCell::default(),
        last_scale: Cell::new(window.window().scale_factor()),
        window_state: Cell::new(settings.window),
        update_url: RefCell::default(),
        device_names: RefCell::new(vec![None]),
        device_labels: RefCell::new(vec!["系统默认".into()]),
        selected_device: RefCell::new(settings.audio_device.clone()),
        asio_driver_clsids: RefCell::new(Vec::new()),
        asio_channel_pairs: RefCell::new(Vec::new()),
        cast: RefCell::default(),
        radio: RefCell::new(RadioStatus {
            auto_play_enabled: settings.auto_play_similar,
            ..RadioStatus::default()
        }),
    });
    window.set_album_rows(ModelRc::from(ui.grids[GRID_ALBUMS].clone()));
    window.set_artist_rows(ModelRc::from(ui.grids[GRID_ARTISTS].clone()));
    window.set_fav_album_rows(ModelRc::from(ui.grids[GRID_FAV_ALBUMS].clone()));
    window.set_fav_artist_rows(ModelRc::from(ui.grids[GRID_FAV_ARTISTS].clone()));
    window.set_search_artist_rows(ModelRc::from(ui.grids[GRID_SEARCH_ARTISTS].clone()));
    window.set_search_album_rows(ModelRc::from(ui.grids[GRID_SEARCH_ALBUMS].clone()));
    window.set_search_composer_rows(ModelRc::from(ui.grids[GRID_SEARCH_COMPOSERS].clone()));
    window.set_playlist_rows(ModelRc::from(ui.grids[GRID_PLAYLISTS].clone()));
    window.set_tracks(ModelRc::from(ui.tracks.clone()));
    window.set_fav_tracks(ModelRc::from(ui.fav_tracks.clone()));
    window.set_detail_tracks(ModelRc::from(ui.detail_tracks.clone()));
    window.set_search_tracks(ModelRc::from(ui.search_tracks.clone()));
    window.set_queue(ModelRc::from(ui.queue.clone()));
    window.set_artist_favs(ModelRc::from(ui.artist_favs.clone()));
    window.set_artist_credits(ModelRc::from(ui.artist_credits.clone()));
    window.on_artist_credit_role_changed(|role| {
        with_ui(|ui, _| {
            if let Some(DetailView::Artist(a)) = ui.detail.borrow().as_ref() {
                ui.send(Command::FilterArtistCredits {
                    artist_id: a.id,
                    role: role.to_string(),
                });
            }
        });
    });
    window.on_artist_dir_role_changed(|role| {
        with_ui(|ui, w| {
            let parsed = match role.as_str() {
                "credit" => Some(client_core::ArtistRole::Credit),
                "lyricist" => Some(client_core::ArtistRole::Lyricist),
                "composer" => Some(client_core::ArtistRole::Composer),
                "arranger" => Some(client_core::ArtistRole::Arranger),
                "producer" => Some(client_core::ArtistRole::Producer),
                _ => None,
            };
            *ui.artist_dir_role.borrow_mut() = parsed;
            ui.sent.borrow_mut().remove(&7);
            w.set_content_scroll_y(0.0);
            ui.query(w, 7, w.get_q_all_artists());
        });
    });
    window.on_full_changed(|| with_ui(|ui, w| ui.full_changed(w)));
    window.on_open_update(|| {
        with_ui(|ui, _| {
            let url = ui.update_url.borrow().clone();
            if !url.is_empty() {
                os::browser::open_url(&url);
            }
        });
    });
    let c = commander.clone();
    window.on_now_info(move || c.send(Command::NowTrackInfo));
    {
        let home = window.global::<Home>();
        home.set_pane_tracks(ModelRc::from(ui.pane_tracks.clone()));
        home.set_mixes(ModelRc::from(ui.home_mixes.clone()));
        home.set_recent(ModelRc::from(ui.home_recent.clone()));
        home.set_resume(ModelRc::from(ui.home_resume.clone()));
        home.set_songs(ModelRc::from(ui.home_songs.clone()));
        home.set_works(ModelRc::from(ui.grids[GRID_HOME_WORKS].clone()));
        home.set_recent_albums(ModelRc::from(ui.grids[GRID_HOME_ALBUMS].clone()));
        home.set_chronicle(ModelRc::from(ui.grids[GRID_CHRONICLE].clone()));
        home.set_playlists(ModelRc::from(ui.grids[GRID_HOME_PLAYLISTS].clone()));
        let lib = window.global::<Lib032>();
        lib.set_works(ModelRc::from(ui.grids[GRID_WORKS].clone()));
        lib.set_series(ModelRc::from(ui.grids[GRID_SERIES].clone()));
        lib.set_history(ModelRc::from(ui.played.clone()));
        lib.set_work_albums(ModelRc::from(ui.grids[GRID_WORK_ALBUMS].clone()));
        lib.set_series_members(ModelRc::from(ui.grids[GRID_MEMBERS].clone()));
        lib.set_series_albums(ModelRc::from(ui.grids[GRID_SERIES_ALBUMS].clone()));
        lib.set_mix_others(ModelRc::from(ui.mix_others.clone()));
        window
            .global::<LyricsSearch>()
            .set_results(ModelRc::from(ui.lyrics_results.clone()));
    }
    Ui::show_lyrics_settings(window, &settings.lyrics);
    window.set_lyrics(ModelRc::from(ui.lyrics.clone()));
    window.set_playlist_choices(ModelRc::from(ui.playlist_choices.clone()));
    ui.scale_timer
        .start(slint::TimerMode::Repeated, Duration::from_secs(1), || {
            with_ui(|ui, w| {
                ui.check_scale(w);
                ui.record_window(w);
            });
        });
    UI.with(|cell| *cell.borrow_mut() = Some(ui));

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

    // Navigation and library.
    window.on_navigate(|page, id| {
        with_ui(|ui, w| {
            let id = i64::from(id);
            let route = match page {
                11 => Route::Artist(id),
                12 => Route::Playlist(id),
                10 => Route::Album(id),
                13 => Route::Work(id),
                14 => {
                    ui.open_series(w, id);
                    return;
                }
                15 => Route::DailyMix(id),
                22 if !w.global::<Lib032>().get_series_supported() => {
                    // Without the series API the list lives on the web.
                    ui.open_web("/admin/series");
                    return;
                }
                page => Route::Page(page),
            };
            ui.navigate(w, route);
        });
    });
    window.on_go_back(|| with_ui(|ui, w| ui.go_back(w)));
    window.on_artist_chosen(|id| {
        with_ui(|ui, w| {
            w.set_dialog(DialogKind::None);
            if id > 0 {
                ui.navigate(w, Route::Artist(id.into()));
            }
        })
    });
    window.on_query_changed(|target, q| with_ui(|ui, w| ui.query(w, target, q)));
    window.on_clear_filter(|target| with_ui(|ui, w| ui.filter_by_artist(w, target == 0, None)));
    window.on_card_layout_changed(|grid, columns, card_size| {
        with_ui(|ui, w| {
            // Posters are 2:3.
            let poster = matches!(
                grid,
                GridId::Works | GridId::Series | GridId::HomeWorks | GridId::Members
            );
            let logical = (card_size, card_size * if poster { 1.5 } else { 1.0 });
            ui.grids[grid_index(grid)].set_layout(
                usize::try_from(columns).unwrap_or(1),
                logical,
                ui.scale(w),
            );
        });
    });
    window.on_card_row_shown(|grid, row| {
        with_ui(|ui, _| {
            if let Ok(row) = usize::try_from(row) {
                ui.grids[grid_index(grid)].show_row(row);
            }
        });
    });
    window.on_card_limit_changed(|grid, limit| {
        with_ui(|ui, _| {
            ui.grids[grid_index(grid)].set_limit(usize::try_from(limit).unwrap_or(0));
        });
    });
    window.on_card_action(|grid, action, id, arg| {
        with_ui(|ui, w| {
            ui.card_action(w, grid_index(grid), &action, id.into(), arg.into());
        });
    });
    window.on_track_action(|list, action, index, arg| {
        with_ui(|ui, w| {
            if let Ok(index) = usize::try_from(index) {
                ui.track_action(w, track_list(list), &action, index, arg.into());
            }
        });
    });
    window
        .on_detail_action(|action, arg| with_ui(|ui, w| ui.detail_action(w, &action, arg.into())));
    window.on_dialog_confirm(|name, description| {
        with_ui(|ui, _| ui.dialog_confirm(name.trim(), description.trim()));
    });
    window.on_new_playlist(|| {
        with_ui(|ui, w| ui.open_dialog(w, Dialog::New(None), "", ""));
    });
    let c = commander.clone();
    window.on_shuffle_all(move || c.send(Command::ShuffleAll));
    let c = commander.clone();
    window.on_play_list(move |list, shuffle| {
        let source = TrackSource::List(track_list(list));
        c.send(if shuffle {
            Command::ShufflePlay(source)
        } else {
            Command::Play(source)
        });
    });
    search::install_callbacks(window);
    window.on_toggle_now_favorite(|| {
        with_ui(|ui, _| {
            if let Some(now) = ui.now.borrow().as_ref() {
                ui.send(Command::SetFavorite {
                    kind: FavoriteKind::Track,
                    id: now.track_id,
                    favorite: !now.favorite,
                });
            }
        });
    });
    window.on_open_now_album(|| {
        with_ui(|ui, w| {
            let album = ui.now.borrow().as_ref().map(|n| n.album_id);
            if let Some(id) = album.filter(|id| *id > 0) {
                ui.navigate(w, Route::Album(id));
            }
        });
    });
    window.on_open_now_artist(|id| {
        with_ui(|ui, w| {
            if id > 0 {
                ui.navigate(w, Route::Artist(id.into()));
            }
        });
    });

    // 032 pages.
    {
        let home = window.global::<Home>();
        home.on_tab_selected(|tab| with_ui(|ui, w| ui.select_tab(w, tab)));
        home.on_action(|action, arg| with_ui(|ui, w| ui.home_action(w, &action, arg.into())));
        let lib = window.global::<Lib032>();
        lib.on_query_works(|search, kind, sort| {
            with_ui(|ui, w| {
                w.global::<Lib032>().set_works_loading(true);
                ui.send(Command::QueryWorks(m032::works_query(&search, kind, sort)));
            });
        });
        lib.on_query_series(|search| {
            with_ui(|ui, w| {
                w.global::<Lib032>().set_series_loading(true);
                ui.send(Command::QuerySeries(search.to_string()));
            });
        });
        lib.on_detail_action(|action, arg| {
            with_ui(|ui, w| ui.lib032_action(w, &action, arg.into()));
        });
        lib.on_open_web(|path| with_ui(|ui, _| ui.open_web(&path)));
        let search = window.global::<LyricsSearch>();
        search.on_search(|name, artist| {
            with_ui(|ui, w| {
                w.global::<LyricsSearch>().set_searching(true);
                ui.send(Command::SearchLyrics {
                    name: name.to_string(),
                    artist: artist.to_string(),
                });
            });
        });
        let c = commander.clone();
        search.on_select(move |i| {
            if let Ok(i) = usize::try_from(i) {
                c.send(Command::PreviewLyrics(i));
            }
        });
        let c = commander.clone();
        search.on_apply(move |i| {
            if let Ok(i) = usize::try_from(i) {
                c.send(Command::ApplyLyrics(i));
            }
        });
    }
    window.on_lyrics_tool(|tool| {
        with_ui(|ui, w| match tool.as_str() {
            "search" => ui.open_lyrics_search(w),
            "clear" => ui.send(Command::ClearLyrics),
            "translate" => ui.send(Command::ToggleTranslation),
            _ => {}
        });
    });
    let c = commander.clone();
    window.on_lyrics_offset_changed(move |ms| c.send(Command::SetLyricsOffset(ms.into())));
    let c = commander.clone();
    window.on_lyrics_seek(move |i| {
        if let Ok(i) = usize::try_from(i) {
            c.send(Command::SeekToLyric(i));
        }
    });
    let c = commander.clone();
    let weak = window.as_weak();
    window.on_lyrics_settings_changed(move || {
        if let Some(w) = weak.upgrade() {
            c.send(Command::SetLyricsSettings(Ui::lyrics_settings(&w)));
        }
    });

    // Settings.
    let c = commander.clone();
    let weak = window.as_weak();
    window.on_panel_changed(move || {
        if let Some(w) = weak.upgrade() {
            c.send(Command::SetRightPanel(RightPanel {
                open: w.get_panel_open(),
                queue_tab: w.get_panel_queue_tab(),
                queue: w.get_panel_queue(),
                lyrics: w.get_panel_lyrics(),
                lyrics_first: w.get_panel_lyrics_first(),
                split: w.get_panel_split(),
                width: w.get_panel_width(),
            }));
        }
    });
    let c = commander.clone();
    let weak = window.as_weak();
    window.on_sidebar_resized(move || {
        if let Some(w) = weak.upgrade() {
            c.send(Command::SetSidebarWidth(w.get_sidebar_width()));
        }
    });
    // The detail header cover follows the header's cover size.
    window.on_detail_art_changed(|| with_ui(|ui, w| ui.reload_detail_cover(w)));
    // The right panel cover follows the panel width.
    window.on_np_art_changed(|| {
        with_ui(|ui, w| {
            let artwork = ui.now.borrow().as_ref().map(|n| n.artwork.clone());
            ui.load_now_large(w, &artwork.unwrap_or_default());
        });
    });
    window.on_queue_search_changed(|text| {
        with_ui(|ui, w| {
            *ui.queue_search.borrow_mut() = text.trim().to_owned();
            ui.show_queue(w);
        });
    });
    window.on_save_queue(|| {
        with_ui(|ui, w| ui.open_dialog(w, Dialog::New(Some(TrackSource::Queue)), "播放队列", ""));
    });
    let c = commander.clone();
    window.on_close_to_tray_toggled(move |on| c.send(Command::SetCloseToTray(on)));
    let c = commander.clone();
    window.on_fade_on_pause_toggled(move |on| c.send(Command::SetFadeOnPause(on)));
    let c = commander.clone();
    let weak = window.as_weak();
    window.on_loudness_changed(move || {
        if let Some(w) = weak.upgrade() {
            c.send(Command::SetLoudness(Ui::loudness_settings(&w)));
        }
    });
    let c = commander.clone();
    let weak = window.as_weak();
    window.on_theme_selected(move |i| {
        c.send(Command::SetTheme(match i {
            0 => ThemeMode::System,
            2 => ThemeMode::Light,
            _ => ThemeMode::Dark,
        }));
        if let Some(w) = weak.upgrade() {
            w.set_theme_mode(i);
            crate::window_chrome::sync(&w);
        }
    });
    let c = commander.clone();
    window.on_settings_opened(move || c.send(Command::RefreshAudioSettings));
    let c = commander.clone();
    let weak = window.as_weak();
    window.on_audio_device_selected(move |index| {
        let name = UI.with(|cell| {
            let ui = cell.borrow();
            let names = ui.as_ref()?.device_names.borrow();
            usize::try_from(index)
                .ok()
                .and_then(|i| names.get(i).cloned())
        });
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
    window.on_exclusive_toggled(move |on| {
        c.send(Command::SetExclusiveMode(if on {
            ExclusiveMode::Wasapi
        } else {
            ExclusiveMode::Off
        }));
    });
    let c = commander.clone();
    window.on_exclusive_quick_toggled(move || {
        c.send(Command::ToggleExclusive);
    });
    let c = commander.clone();
    window.on_exclusive_mode_selected(move |index| {
        let mode = match index {
            1 => ExclusiveMode::Wasapi,
            2 => ExclusiveMode::Asio,
            _ => ExclusiveMode::Off,
        };
        c.send(Command::SetExclusiveMode(mode));
    });
    let c = commander.clone();
    window.on_asio_driver_selected(move |index| {
        let clsid = UI.with(|cell| {
            let ui = cell.borrow();
            let clsids = ui.as_ref()?.asio_driver_clsids.borrow();
            usize::try_from(index)
                .ok()
                .and_then(|i| clsids.get(i).cloned())
        });
        if let Some(clsid) = clsid {
            c.send(Command::SetAsioDriver(clsid));
        }
    });
    let c = commander.clone();
    window.on_asio_channel_pair_selected(move |index| {
        let pair = UI.with(|cell| {
            let ui = cell.borrow();
            let pairs = ui.as_ref()?.asio_channel_pairs.borrow();
            usize::try_from(index)
                .ok()
                .and_then(|i| pairs.get(i).copied())
        });
        if let Some((left, right)) = pair {
            c.send(Command::SetAsioChannelPair(left, right));
        }
    });
    let c = commander.clone();
    window.on_refresh_asio_driver(move || c.send(Command::RefreshAsioDriver));
    let c = commander.clone();
    window.on_open_asio_control_panel(move || c.send(Command::OpenAsioControlPanel));
    commander.send(Command::RefreshAudioDevices);
    window.on_transcode_selected(|index| {
        with_ui(|ui, _| {
            let choice = usize::try_from(index)
                .ok()
                .and_then(|i| ui.transcode_choices.borrow().get(i).copied());
            if let Some(transcode) = choice {
                ui.transcode.set(transcode);
                ui.send(Command::SetTranscode(transcode));
            }
        });
    });

    // Queue and transport.
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
    window.on_move_queue_item(move |uid, to| {
        if let (Ok(uid), Ok(to)) = (u64::try_from(uid), usize::try_from(to)) {
            c.send(Command::MoveQueueItem { uid, to });
        }
    });
    window.on_queue_row_action(|what, uid, arg| {
        with_ui(|ui, w| {
            if let Ok(uid) = u64::try_from(uid) {
                ui.queue_row_action(w, &what, uid, arg.into());
            }
        });
    });
    {
        let radio = window.global::<RadioState>();
        radio.set_auto_play(settings.auto_play_similar);
        let c = commander.clone();
        radio.on_toggle_auto_play(move |on| c.send(Command::SetAutoPlaySimilar(on)));
        let c = commander.clone();
        radio.on_start_from_current(move || c.send(Command::StartSimilarRadioCurrent));
        let c = commander.clone();
        radio.on_refresh_batch(move || c.send(Command::RefreshSimilarRadio));
        let c = commander.clone();
        radio.on_load_more(move || c.send(Command::LoadMoreSimilarRadio));
        let c = commander.clone();
        radio.on_switch_sequential(move || c.send(Command::SetRepeat(Repeat::Off)));
    }
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
    let c = commander;
    window.on_volume_changed(move |v| c.send(Command::SetVolume(f64::from(v))));
}

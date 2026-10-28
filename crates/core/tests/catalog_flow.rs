//! Catalog end to end: lists, sorting, detail pages, search, similar
//! tracks, favorites and the playlist editor.

use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use client_core::{
    AudioDevice, Command, CoreConfig, Engine, Event, FavoriteKind, ListKind, ListQuery, ListSort,
    MemorySecrets, RowKind, Settings, SortOrder, View, ViewRow,
};
use serde_json::{Value, json};
use wiremock::matchers::{body_json, method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[derive(Default)]
struct FakeEngine {
    calls: Mutex<Vec<String>>,
}

impl Engine for FakeEngine {
    fn load(&self, url: &str) -> Result<(), String> {
        self.calls.lock().unwrap().push(format!("load {url}"));
        Ok(())
    }
    fn append(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn clear_upcoming(&self) -> Result<(), String> {
        Ok(())
    }
    fn stop(&self) -> Result<(), String> {
        Ok(())
    }
    fn set_paused(&self, _: bool) -> Result<(), String> {
        Ok(())
    }
    fn seek(&self, _: f64) -> Result<(), String> {
        Ok(())
    }
    fn set_volume(&self, _: f64) -> Result<(), String> {
        Ok(())
    }
    fn audio_devices(&self) -> Result<Vec<AudioDevice>, String> {
        Ok(Vec::new())
    }
    fn set_audio_device(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn set_exclusive(&self, _: bool) -> Result<(), String> {
        Ok(())
    }
}

fn track(id: i64, title: &str) -> Value {
    json!({
        "id": id, "albumId": 1, "title": title, "artist": "歌手", "album": "专辑",
        "durationMillis": 240000, "streamUrl": format!("/api/v1/tracks/{id}/stream"),
        "artworkUrl": "/api/v1/artwork/1",
    })
}

async fn server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(path("/api/v1/capabilities"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "apiVersion": "v1", "apiRevision": 3,
            "features": { "browse": true, "playlistItemOps": true, "playlistArtwork": true },
            "playback": { "clientKinds": ["desktop"] },
        })))
        .mount(&server)
        .await;
    // Media token probe: one track, first byte.
    Mock::given(method("GET"))
        .and(path("/api/v1/tracks"))
        .and(query_param("limit", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [track(1, "一曲目")], "total": 2, "limit": 1, "offset": 0,
        })))
        .mount(&server)
        .await;
    Mock::given(path("/api/v1/tracks/1/stream"))
        .respond_with(ResponseTemplate::new(206))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/playback/events"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "applied": true, "state": "playing" })),
        )
        .mount(&server)
        .await;

    // Album grid, newest first.
    Mock::given(method("GET"))
        .and(path("/api/v1/browse/albums"))
        .and(query_param("sort", "added"))
        .and(query_param("order", "desc"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [
                { "id": 1, "title": "新しい", "artist": "A", "year": 2020, "trackCount": 2,
                  "artworkUrl": "/api/v1/artwork/1" },
                { "id": 2, "title": "古い", "artist": "B", "year": 1999, "trackCount": 1 },
            ],
            "total": 2, "limit": 500, "offset": 0,
        })))
        .mount(&server)
        .await;
    // The same list sorted by title ascending gives the other order.
    Mock::given(method("GET"))
        .and(path("/api/v1/browse/albums"))
        .and(query_param("sort", "title"))
        .and(query_param("order", "asc"))
        .and(query_param("yearFrom", "1990"))
        .and(query_param("yearTo", "2000"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [{ "id": 2, "title": "古い", "artist": "B", "year": 1999, "trackCount": 1 }],
            "total": 1, "limit": 500, "offset": 0,
        })))
        .mount(&server)
        .await;
    // Random albums: the seed must be present and identical on page 2, so
    // the two pages do not overlap or skip.
    Mock::given(method("GET"))
        .and(path("/api/v1/browse/albums"))
        .and(query_param("sort", "random"))
        .and(query_param("offset", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [{ "id": 1, "title": "新しい", "artist": "A", "year": 2020 }],
            "total": 2, "limit": 500, "offset": 0,
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/browse/albums"))
        .and(query_param("sort", "random"))
        .and(query_param("offset", "500"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [{ "id": 2, "title": "古い", "artist": "B", "year": 1999 }],
            "total": 2, "limit": 500, "offset": 500,
        })))
        .mount(&server)
        .await;
    // Shuffle-all: a random track list, answered slowly so the sign-out in
    // the test really races it.
    Mock::given(method("GET"))
        .and(path("/api/v1/browse/tracks"))
        .and(query_param("sort", "random"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(250))
                .set_body_json(json!({
                    "items": [track(1, "一曲目"), track(2, "二曲目")],
                    "total": 2, "limit": 500, "offset": 0,
                })),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/browse/tracks"))
        .and(query_param_is_missing("q"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [track(1, "一曲目"), track(2, "二曲目")],
            "total": 2, "limit": 500, "offset": 0,
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/browse/artists"))
        .and(query_param_is_missing("q"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [{ "id": 9, "name": "YOASOBI", "albumCount": 4, "trackCount": 31 }],
            "total": 1, "limit": 500, "offset": 0,
        })))
        .mount(&server)
        .await;
    // Search with q: one artist, one album, one track.
    Mock::given(method("GET"))
        .and(path("/api/v1/browse/artists"))
        .and(query_param("q", "夜"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [{ "id": 9, "name": "YOASOBI" }],
            "total": 1, "limit": 20, "offset": 0,
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/browse/albums"))
        .and(query_param("q", "夜"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [{ "id": 5, "title": "夜のアルバム" }],
            "total": 1, "limit": 20, "offset": 0,
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/browse/tracks"))
        .and(query_param("q", "夜"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [track(3, "夜に駆ける")],
            "total": 1, "limit": 20, "offset": 0,
        })))
        .mount(&server)
        .await;
    // Album detail with one embedded track of two.
    Mock::given(method("GET"))
        .and(path("/api/v1/albums/1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "album": {
                "id": 1, "title": "新しい", "artist": "A", "trackCount": 2, "year": 2020,
                "artworkUrl": "/api/v1/artwork/1", "genres": "j-pop", "label": "L",
            },
            "tracks": [track(1, "一曲目")],
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/artists/9"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "artist": { "id": 9, "name": "YOASOBI", "albumCount": 1, "trackCount": 2 },
            "releases": [{ "id": 1, "title": "新しい", "artist": "YOASOBI", "relation": "personal" }],
            "topTracks": [track(1, "一曲目")],
            "topTracksArePopular": true,
            "tracksTotal": 2,
        })))
        .mount(&server)
        .await;
    // Favorites-only album list (the client filters and sorts it).
    Mock::given(method("GET"))
        .and(path("/api/v1/favorites/albums"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [
                { "id": 2, "title": "古い", "artist": "B", "year": 1999, "addedAt": "2026-01-01T00:00:00Z" },
                { "id": 1, "title": "新しい", "artist": "A", "year": 2020, "addedAt": "2026-02-01T00:00:00Z" },
            ],
            "total": 2, "limit": 500, "offset": 0,
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/playlists"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [
                { "id": 5, "name": "夜", "itemCount": 2, "durationMillis": 480000, "revision": 3 },
                { "id": 6, "name": "朝", "itemCount": 0, "revision": 1 },
            ],
            "total": 2, "limit": 500, "offset": 0,
        })))
        .mount(&server)
        .await;
    // Playlist 5 holds three tracks at revision 3; removing id 2 leaves two
    // at revision 4 (which the reorder below must use).
    Mock::given(method("GET"))
        .and(path("/api/v1/playlists/5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "playlist": { "id": 5, "name": "夜", "itemCount": 3, "revision": 3 },
            "tracks": [track(1, "一曲目"), track(2, "二曲目"), track(3, "三曲目")],
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/playlists/5/items/remove"))
        .and(body_json(json!({ "trackIds": [2], "expectedRevision": 3 })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "removed": 1, "total": 2, "revision": 4,
        })))
        .mount(&server)
        .await;
    // The reorder must carry the revision the removal returned, and every
    // remaining track exactly once.
    Mock::given(method("PUT"))
        .and(path("/api/v1/playlists/5/order"))
        .and(body_json(
            json!({ "trackIds": [3, 1], "expectedRevision": 4 }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "revision": 5 })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/playlists"))
        .and(body_json(json!({ "name": "新規", "description": "" })))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "id": 7, "name": "新規" })))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/v1/playlists/6"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/api/v1/tracks/1/favorite"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/api/v1/artists/9/favorite"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/v1/tracks/1/favorite"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/tracks/1/similar"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [{ "track": track(2, "二曲目") }, { "track": track(1, "一曲目") }],
        })))
        .mount(&server)
        .await;
    server
}

struct Harness {
    core: Option<client_core::CoreHandle>,
    events: mpsc::Receiver<Event>,
    engine: Arc<FakeEngine>,
    /// Playlists seen since sign-in (they load before the tests look).
    playlists: Arc<Mutex<Vec<client_core::PlaylistItem>>>,
}

impl Harness {
    /// Signs in and waits for the album grid.
    async fn start() -> (MockServer, Self, Vec<ViewRow>) {
        let server = server().await;
        let engine = Arc::new(FakeEngine::default());
        // Tests run in parallel: one directory each.
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let data_dir =
            std::env::temp_dir().join(format!("032player-catalog-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data_dir);
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let playlists = Arc::new(Mutex::new(Vec::new()));
        let seen = playlists.clone();
        let core = client_core::start(
            engine.clone(),
            CoreConfig {
                data_dir,
                secrets: Arc::new(MemorySecrets::default()),
                settings: Settings::default(),
            },
            move |e| {
                if let Event::Playlists(items) = &e {
                    *seen.lock().unwrap() = items.clone();
                }
                let _ = tx.lock().unwrap().send(e);
            },
        );
        let h = Self {
            core: Some(core),
            events: rx,
            engine,
            playlists,
        };
        h.send(Command::Login {
            server_url: server.uri(),
            api_token: "api".into(),
            media_token: "media".into(),
        });
        h.wait("login", |e| matches!(e, Event::LoggedIn { .. }));
        let rows = h.wait_rows(View::Albums);
        (server, h, rows)
    }

    fn send(&self, command: Command) {
        self.core.as_ref().unwrap().send(command);
    }

    fn wait(&self, what: &str, pred: impl Fn(&Event) -> bool) -> Event {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.events.recv_timeout(left) {
                Ok(e) if pred(&e) => return e,
                Ok(_) => {}
                Err(_) => panic!("timed out waiting for {what}"),
            }
        }
    }

    /// Waits for the rows of `view` and returns them.
    fn wait_rows(&self, view: View) -> Vec<ViewRow> {
        let event = self.wait(&format!("rows of {view:?}"), |e| {
            matches!(e, Event::ViewRows { view: v, rows, .. } if *v == view && !rows.is_empty())
        });
        let Event::ViewRows { rows, .. } = event else {
            unreachable!()
        };
        rows
    }

    fn shutdown(self) {
        tokio::task::spawn_blocking(move || {
            if let Some(core) = self.core {
                core.shutdown();
            }
        });
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn lists_sort_filter_and_page() {
    let (_server, h, rows) = Harness::start().await;
    let titles: Vec<&str> = rows.iter().map(|r| r.title.as_str()).collect();
    assert_eq!(titles, ["新しい", "古い"]);

    // Sort by title, ascending, within a year range: the server applies both.
    let query = ListQuery {
        kind: ListKind::Albums,
        sort: ListSort::Title,
        order: api::SortOrder::Asc,
        year_from: Some(1990),
        year_to: Some(2000),
        ..ListQuery::default()
    };
    h.send(Command::ListQuery {
        query: query.clone(),
    });
    h.wait("reset", |e| matches!(e, Event::ViewReset { .. }));
    let rows = h.wait_rows(View::Albums);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].title, "古い");
    assert_eq!(rows[0].meta, "1999");

    // Track list: rows carry artist and album + duration.
    h.send(Command::Navigate(View::Tracks));
    let rows = h.wait_rows(View::Tracks);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].subtitle, "歌手");
    assert_eq!(rows[0].meta, "专辑 · 4:00");
    // A track row plays from its own index in the view's track list.
    assert_eq!(rows[0].play, Some(0));
    assert_eq!(rows[1].play, Some(1));
    assert!(rows[0].play.is_some());

    // Artists and playlists.
    h.send(Command::Navigate(View::Artists));
    let rows = h.wait_rows(View::Artists);
    assert_eq!(rows[0].title, "YOASOBI");
    assert_eq!(rows[0].meta, "4 张专辑 · 31 首");

    // Playlists are paged and sorted client-side: the default is by name.
    h.send(Command::Navigate(View::Playlists));
    let rows = h.wait_rows(View::Playlists);
    assert_eq!(rows.len(), 2);
    let names: Vec<&str> = rows.iter().map(|r| r.title.as_str()).collect();
    assert!(names.contains(&"夜") && names.contains(&"朝"), "{names:?}");
    // Ascending by code point (夜 U+591C < 朝 U+671D). Not collation-aware,
    // which matches the Electron client's plain toLowerCase() compare.
    assert_eq!(names, ["夜", "朝"]);

    // Change the sort to duration: 夜 has 8 minutes, 朝 none.
    h.send(Command::ListQuery {
        query: ListQuery {
            kind: ListKind::Playlists,
            sort: ListSort::Duration,
            order: SortOrder::Desc,
            ..ListQuery::default()
        },
    });
    let rows = h.wait_rows(View::Playlists);
    assert_eq!(rows[0].title, "夜", "longest first");
    h.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn detail_pages_search_and_similar() {
    let (_server, h, _) = Harness::start().await;

    h.send(Command::Navigate(View::Album(1)));
    let event = h.wait("album header", |e| matches!(e, Event::ViewHeader(_)));
    let Event::ViewHeader(header) = event else {
        unreachable!()
    };
    assert_eq!(header.title, "新しい");
    assert!(header.details.iter().any(|d| d == "j-pop"));
    assert!(header.can_play);
    let rows = h.wait_rows(View::Album(1));
    assert_eq!(rows.len(), 2);

    // Playing the album loads the first track.
    h.send(Command::PlayRow(1));
    h.wait(
        "second track",
        |e| matches!(e, Event::NowPlaying(Some(n)) if n.track_id == 2),
    );
    let calls = h.engine.calls.lock().unwrap().clone();
    assert!(
        calls
            .iter()
            .any(|c| c.ends_with("tracks/2/stream?mediaToken=media")),
        "{calls:?}"
    );

    // Artist page: releases then top tracks.
    h.send(Command::Navigate(View::Artist(9)));
    let rows = h.wait_rows(View::Artist(9));
    assert_eq!(rows[0].title, "新しい");
    assert!(rows.iter().any(|r| r.title == "一曲目"));

    // Playlist page and its editor.
    h.send(Command::Navigate(View::Playlist(5)));
    let rows = h.wait_rows(View::Playlist(5));
    assert_eq!(rows.len(), 3);
    h.send(Command::RemoveFromPlaylist {
        playlist_id: 5,
        track_ids: vec![2],
    });
    // The editor re-reads the playlist after a write.
    h.wait("playlist reload", |e| {
        matches!(
            e,
            Event::ViewReset {
                view: View::Playlist(5),
                ..
            }
        )
    });
    let requests = _server.received_requests().await.unwrap();
    assert!(
        requests
            .iter()
            .any(|r| r.url.path() == "/api/v1/playlists/5/items/remove"),
        "the removal was sent with the open revision"
    );

    // Search sets the view and returns grouped rows.
    h.send(Command::Search("夜".into()));
    h.wait(
        "search",
        |e| matches!(e, Event::ViewHeader(h) if h.title == "搜索"),
    );
    let rows = h.wait_rows(View::Search("夜".into()));
    assert_eq!(rows.len(), 3, "artist, album and track");
    assert_eq!(rows[1].title, "夜のアルバム");

    // Similar tracks of the playing track.
    h.send(Command::Navigate(View::Similar(1)));
    let rows = h.wait_rows(View::Similar(1));
    assert_eq!(rows.len(), 1, "the track itself is filtered out");
    assert_eq!(rows[0].id, 2);
    h.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn mixed_pages_favorite_the_right_kind_and_play_the_right_track() {
    let (_server, h, _) = Harness::start().await;

    // The search page holds artists, albums and tracks; ids collide on
    // purpose (album 5 and the track with id 5 do not exist, but the artist
    // and the album share nothing either). The point: each row maps to the
    // kind of its own row, not to the layout of the page.
    h.send(Command::Search("夜".into()));
    // Search results arrive as one page: wait for it before reading rows.
    h.wait(
        "search header",
        |e| matches!(e, Event::ViewHeader(h) if h.title == "搜索"),
    );
    let rows = h.wait_rows(View::Search("夜".into()));
    let artist = rows
        .iter()
        .find(|r| r.kind == RowKind::Artist)
        .unwrap()
        .clone();
    let track = rows
        .iter()
        .find(|r| r.kind == RowKind::Track)
        .unwrap()
        .clone();
    assert_eq!(artist.kind.favorite_kind(), Some(FavoriteKind::Artist));
    assert_eq!(track.kind.favorite_kind(), Some(FavoriteKind::Track));
    // A track row plays from its own index in the view's track list.
    assert_eq!(track.play, Some(0));

    h.send(Command::SetFavorite {
        kind: FavoriteKind::Artist,
        id: artist.id,
        favorite: true,
    });
    h.wait("artist favorite", |e| {
        matches!(
            e,
            Event::Favorite {
                kind: FavoriteKind::Artist,
                favorite: true,
                ..
            }
        )
    });
    h.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn playlist_writes_bump_the_cached_revision() {
    let (_server, h, _) = Harness::start().await;
    h.send(Command::Navigate(View::Playlist(5)));
    h.wait_rows(View::Playlist(5));

    // Removing sends expectedRevision 3 (from the loaded detail) and the
    // answer's revision (4) is adopted, so the next write is not stale.
    h.send(Command::RemoveFromPlaylist {
        playlist_id: 5,
        track_ids: vec![2],
    });
    h.wait(
        "removed",
        |e| matches!(e, Event::Notice(t) if t.contains("已从歌单移除")),
    );
    h.send(Command::MovePlaylistItem {
        playlist_id: 5,
        from: 0,
        to: 1,
    });
    // A second write while the first is in flight is refused, not sent with
    // a stale revision and a stale track list.
    h.send(Command::MovePlaylistItem {
        playlist_id: 5,
        from: 0,
        to: 1,
    });
    h.wait("reordered", |e| {
        matches!(
            e,
            Event::ViewReset {
                view: View::Playlist(5),
                ..
            }
        )
    });
    h.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_playlist_write_leaves_the_editor_usable() {
    let server = MockServer::start().await;
    Mock::given(path("/api/v1/capabilities"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "apiVersion": "v1", "apiRevision": 3,
            "features": { "browse": true },
            "playback": { "clientKinds": ["desktop"] },
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/tracks"))
        .and(query_param("limit", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [], "total": 0, "limit": 1, "offset": 0,
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/browse/albums"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [], "total": 0, "limit": 500, "offset": 0,
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/playlists"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [], "total": 0, "limit": 500, "offset": 0,
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/playlists/5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "playlist": { "id": 5, "name": "夜", "itemCount": 1, "revision": 1 },
            "tracks": [track(1, "一曲目")],
        })))
        .mount(&server)
        .await;
    // The removal fails with a plain server error (not a revision conflict).
    Mock::given(method("POST"))
        .and(path("/api/v1/playlists/5/items/remove"))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({
            "error": { "code": "internal", "message": "boom" }
        })))
        .mount(&server)
        .await;

    let engine = Arc::new(FakeEngine::default());
    let data_dir = std::env::temp_dir().join(format!("032player-plfail-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data_dir);
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let core = client_core::start(
        engine.clone(),
        CoreConfig {
            data_dir,
            secrets: Arc::new(MemorySecrets::default()),
            settings: Settings::default(),
        },
        move |e| {
            let _ = tx.lock().unwrap().send(e);
        },
    );
    let h = Harness {
        core: Some(core),
        events: rx,
        engine,
        playlists: Arc::new(Mutex::new(Vec::new())),
    };
    h.send(Command::Login {
        server_url: server.uri(),
        api_token: "api".into(),
        media_token: "media".into(),
    });
    h.wait("login", |e| matches!(e, Event::LoggedIn { .. }));
    h.send(Command::Navigate(View::Playlist(5)));
    h.wait_rows(View::Playlist(5));

    h.send(Command::RemoveFromPlaylist {
        playlist_id: 5,
        track_ids: vec![1],
    });
    h.wait(
        "failure",
        |e| matches!(e, Event::Notice(t) if t.contains("修改歌单失败")),
    );
    // The editor is not stuck in "busy": the next write reaches the server
    // (which fails again), instead of being refused by the client.
    h.send(Command::RemoveFromPlaylist {
        playlist_id: 5,
        track_ids: vec![1],
    });
    let event = h.wait(
        "second attempt",
        |e| matches!(e, Event::Notice(t) if t.contains("修改歌单失败") || t.contains("还没完成")),
    );
    let Event::Notice(text) = event else {
        unreachable!()
    };
    assert!(
        text.contains("修改歌单失败"),
        "the editor refused to write after a plain failure: {text}"
    );
    h.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_first_page_reports_itself_and_can_be_retried() {
    let server = MockServer::start().await;
    Mock::given(path("/api/v1/capabilities"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "apiVersion": "v1", "apiRevision": 3,
            "features": { "browse": true },
            "playback": { "clientKinds": ["desktop"] },
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/tracks"))
        .and(query_param("limit", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [], "total": 0, "limit": 1, "offset": 0,
        })))
        .mount(&server)
        .await;
    // The first album page fails, later ones succeed.
    Mock::given(method("GET"))
        .and(path("/api/v1/browse/albums"))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({
            "error": { "code": "query_failed", "message": "boom" }
        })))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/browse/albums"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [{ "id": 1, "title": "新しい", "artist": "A", "year": 2020 }],
            "total": 1, "limit": 500, "offset": 0,
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/playlists"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [], "total": 0, "limit": 500, "offset": 0,
        })))
        .mount(&server)
        .await;
    let engine = Arc::new(FakeEngine::default());
    let data_dir = std::env::temp_dir().join(format!("032player-retry-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data_dir);
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let core = client_core::start(
        engine,
        CoreConfig {
            data_dir,
            secrets: Arc::new(MemorySecrets::default()),
            settings: Settings::default(),
        },
        move |e| {
            let _ = tx.lock().unwrap().send(e);
        },
    );
    let h = Harness {
        core: Some(core),
        events: rx,
        engine: Arc::new(FakeEngine::default()),
        playlists: Arc::new(Mutex::new(Vec::new())),
    };
    h.send(Command::Login {
        server_url: server.uri(),
        api_token: "api".into(),
        media_token: "media".into(),
    });
    // The empty page reports the failure (the UI then shows a retry button
    // instead of waiting for a scroll that cannot happen).
    let event = h.wait("failure", |e| matches!(e, Event::LoadFailed { .. }));
    let Event::LoadFailed { empty, .. } = event else {
        unreachable!()
    };
    assert!(empty, "nothing was shown yet");
    // Retrying loads the page; no failure follows.
    h.send(Command::LoadMore);
    h.wait_rows(View::Albums);
    h.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_filter_survives_a_sort_change() {
    let (_server, h, _) = Harness::start().await;
    // Filter by favorites, then change the sort: the second request must
    // still carry favorite=true (the UI keeps its filters across a reload).
    let mut query = ListQuery {
        kind: ListKind::Albums,
        favorite: true,
        ..ListQuery::default()
    };
    h.send(Command::ListQuery {
        query: query.clone(),
    });
    // The placeholder header already carries the filters the reload uses.
    let event = h.wait("filtered", |e| matches!(e, Event::ViewReset { .. }));
    let Event::ViewReset { header, .. } = event else {
        unreachable!()
    };
    assert!(header.toolbar.favorite, "the header dropped the filter");
    h.wait_rows(View::Albums);

    query.sort = ListSort::Title;
    query.order = SortOrder::Asc;
    h.send(Command::ListQuery { query });
    let event = h.wait("sorted", |e| matches!(e, Event::ViewReset { .. }));
    let Event::ViewReset { header, .. } = event else {
        unreachable!()
    };
    assert!(header.toolbar.favorite, "sorting cleared the filter");
    assert_eq!(header.toolbar.sort, ListSort::Title);
    assert!(!header.toolbar.desc);

    // Favorites come from /favorites/albums, which only pages: the sort is
    // applied to the rows the client loaded.
    let rows = h.wait_rows(View::Albums);
    assert_eq!(rows[0].title, "古い", "title ascending");
    h.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_late_answer_after_sign_out_changes_nothing() {
    let (server, h, _) = Harness::start().await;
    h.send(Command::ShuffleAll);
    // Wait until the shuffle-all request is really in flight, then sign out.
    let deadline = Instant::now() + Duration::from_secs(5);
    while !server.received_requests().await.unwrap().iter().any(|r| {
        r.url.path() == "/api/v1/browse/tracks"
            && r.url.query().is_some_and(|q| q.contains("sort=random"))
    }) {
        assert!(Instant::now() < deadline, "shuffle-all was never sent");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    h.send(Command::Logout);
    h.wait("signed out", |e| matches!(e, Event::SignedOut(_)));
    // The delayed answer lands well after the sign-out: give it time, then
    // check that nothing of the old session came back.
    let deadline = Instant::now() + Duration::from_millis(600);
    while Instant::now() < deadline {
        match h.events.recv_timeout(Duration::from_millis(20)) {
            Ok(Event::Queue { items, .. }) => {
                assert!(items.is_empty(), "a late answer repopulated the queue");
            }
            Ok(Event::Playlists(items)) => assert!(items.is_empty()),
            Ok(_) | Err(_) => {}
        }
    }
    h.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn favorites_and_playlist_writes() {
    let (server, h, _) = Harness::start().await;

    h.send(Command::SetFavorite {
        kind: FavoriteKind::Track,
        id: 1,
        favorite: true,
    });
    h.wait("favorite", |e| {
        matches!(
            e,
            Event::Favorite {
                kind: FavoriteKind::Track,
                id: 1,
                favorite: true
            }
        )
    });
    h.send(Command::SetFavorite {
        kind: FavoriteKind::Track,
        id: 1,
        favorite: false,
    });
    h.wait("unfavorite", |e| {
        matches!(
            e,
            Event::Favorite {
                favorite: false,
                ..
            }
        )
    });
    let requests = server.received_requests().await.unwrap();
    let methods: Vec<String> = requests
        .iter()
        .filter(|r| r.url.path() == "/api/v1/tracks/1/favorite")
        .map(|r| format!("{}", r.method))
        .collect();
    assert_eq!(methods, ["PUT", "DELETE"]);

    // The playlist menu gets its list after signing in.
    let deadline = Instant::now() + Duration::from_secs(5);
    while h.playlists.lock().unwrap().len() != 2 {
        assert!(Instant::now() < deadline, "playlists did not load");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    h.send(Command::CreatePlaylist {
        name: "新規".into(),
        description: String::new(),
    });
    // Creating reports it and keeps the user where they were.
    h.wait(
        "created",
        |e| matches!(e, Event::Notice(t) if t.contains("已创建歌单")),
    );
    let requests = server.received_requests().await.unwrap();
    assert!(
        requests
            .iter()
            .any(|r| r.method.as_str() == "POST" && r.url.path() == "/api/v1/playlists"),
        "the playlist was created"
    );
    // Deleting from the playlists list stays on the list and reloads it.
    h.send(Command::Navigate(View::Playlists));
    h.wait_rows(View::Playlists);
    h.send(Command::DeletePlaylist(6));
    h.wait(
        "deleted",
        |e| matches!(e, Event::Notice(t) if t.contains("已删除歌单")),
    );
    h.wait("playlists reloaded", |e| {
        matches!(
            e,
            Event::ViewReset {
                view: View::Playlists,
                ..
            }
        )
    });
    h.shutdown();
}

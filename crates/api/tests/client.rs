use api::{
    AlbumListQuery, AlbumSort, ArtistListQuery, ArtistSort, AuthError, Client, Feature, SortOrder,
    TrackListQuery, TrackSort, authenticate,
};
use serde_json::{Value, json};
use wiremock::matchers::{header, method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

const API: &str = "api-token";
const MEDIA: &str = "media-token";

fn capabilities(revision: i64) -> Value {
    json!({
        "apiVersion": "v1",
        "apiRevision": revision,
        "features": { "browse": true, "lyrics": true, "unknownFlag": true },
        "playback": { "clientKinds": ["web", "android", "desktop"], "contextTypes": ["dailyMix"] },
        "limits": { "maxPageSize": 500, "maxPlaylistTracks": 5000 },
        "extra": { "ignored": true },
    })
}

fn track_json(id: i64) -> Value {
    json!({
        "id": id, "albumId": 3, "title": "曲", "album": "アルバム", "artist": "A",
        "durationMillis": 240000, "streamUrl": format!("/api/v1/tracks/{id}/stream"),
        "artworkUrl": "/api/v1/artwork/9", "codec": "flac", "sampleRate": 48000, "bitDepth": 24,
        "artists": [{ "id": 1, "name": "A", "imageUrl": "", "albumCount": 2 }],
    })
}

async fn mock_caps(server: &MockServer, revision: i64) {
    Mock::given(method("GET"))
        .and(path("/api/v1/capabilities"))
        .and(header("authorization", format!("Bearer {API}").as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(capabilities(revision)))
        .mount(server)
        .await;
}

async fn mock_first_track(server: &MockServer, items: Value) {
    Mock::given(method("GET"))
        .and(path("/api/v1/tracks"))
        .and(query_param("limit", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": items, "limit": 1, "offset": 0, "total": 1
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn authenticate_validates_revision_and_probes_the_media_token() {
    let server = MockServer::start().await;
    mock_caps(&server, 3).await;
    mock_first_track(&server, json!([track_json(7)])).await;
    Mock::given(method("GET"))
        .and(path("/api/v1/tracks/7/stream"))
        .and(query_param("mediaToken", MEDIA))
        .and(header("range", "bytes=0-0"))
        .respond_with(ResponseTemplate::new(206).set_body_bytes(vec![0u8]))
        .expect(1)
        .mount(&server)
        .await;

    let url = format!("{}/", server.uri());
    let (client, caps) = authenticate(&url, &format!(" {API} "), MEDIA)
        .await
        .unwrap();
    assert_eq!(client.base_url(), server.uri());
    assert_eq!(client.api_token(), API);
    assert!(caps.has(Feature::Browse));
    assert!(!caps.has(Feature::DailyMixes));
    assert_eq!(caps.client_kind(), "desktop");
    assert_eq!(caps.page_size(), 500);
}

#[tokio::test]
async fn authenticate_rejects_bad_input_before_any_request() {
    let r = authenticate("http://127.0.0.1:9", "", MEDIA).await;
    assert!(matches!(r, Err(AuthError::CredentialsRequired)));
    let r = authenticate("http://127.0.0.1:9", "same", "same").await;
    assert!(matches!(r, Err(AuthError::TokensMustDiffer)));
    let r = authenticate("not a url", API, MEDIA).await;
    assert!(matches!(r, Err(AuthError::Api(api::ApiError::Url(_)))));
}

#[tokio::test]
async fn authenticate_maps_401_to_invalid_api_token() {
    let server = MockServer::start().await;
    Mock::given(path("/api/v1/capabilities"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "error": { "code": "unauthorized", "message": "bad token" }
        })))
        .mount(&server)
        .await;
    let r = authenticate(&server.uri(), API, MEDIA).await;
    assert!(matches!(r, Err(AuthError::ApiTokenInvalid)), "{r:?}");
}

#[tokio::test]
async fn authenticate_rejects_other_revisions() {
    let server = MockServer::start().await;
    mock_caps(&server, 2).await;
    let r = authenticate(&server.uri(), API, MEDIA).await;
    assert!(matches!(r, Err(AuthError::UnsupportedRevision(2))), "{r:?}");
}

#[tokio::test]
async fn authenticate_detects_an_invalid_media_token() {
    let server = MockServer::start().await;
    mock_caps(&server, 3).await;
    mock_first_track(&server, json!([track_json(7)])).await;
    Mock::given(path("/api/v1/tracks/7/stream"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let r = authenticate(&server.uri(), API, MEDIA).await;
    assert!(matches!(r, Err(AuthError::MediaTokenInvalid)), "{r:?}");
}

#[tokio::test]
async fn authenticate_skips_the_media_probe_on_an_empty_library() {
    let server = MockServer::start().await;
    mock_caps(&server, 3).await;
    mock_first_track(&server, json!([])).await;
    authenticate(&server.uri(), API, MEDIA).await.unwrap();
}

#[tokio::test]
async fn error_bodies_are_parsed_and_fall_back_to_a_generic_message() {
    let server = MockServer::start().await;
    Mock::given(path("/api/v1/browse/albums"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": { "code": "invalid_filter", "message": "sort=nope" }
        })))
        .mount(&server)
        .await;
    Mock::given(path("/api/v1/browse/tracks"))
        .respond_with(ResponseTemplate::new(502).set_body_string("<html>"))
        .mount(&server)
        .await;
    let client = Client::new(&server.uri(), API, MEDIA).unwrap();

    let e = client
        .albums(true, &AlbumListQuery::default(), 0, 10)
        .await
        .unwrap_err();
    assert_eq!(e.status(), Some(400));
    assert_eq!(e.code(), Some("invalid_filter"));
    assert_eq!(e.to_string(), "sort=nope");

    let e = client
        .tracks(true, &TrackListQuery::default(), 0, 10)
        .await
        .unwrap_err();
    assert_eq!(e.status(), Some(502));
    assert_eq!(e.code(), None);
    assert_eq!(e.to_string(), "032 Music Server request failed (502)");
    assert!(!e.is_unauthorized());
}

#[tokio::test]
async fn browse_albums_sends_order_year_range_and_seed() {
    let server = MockServer::start().await;
    Mock::given(path("/api/v1/browse/albums"))
        .and(query_param("sort", "random"))
        .and(query_param("order", "desc"))
        .and(query_param("seed", "42"))
        .and(query_param("yearFrom", "1990"))
        .and(query_param("yearTo", "1999"))
        .and(query_param("limit", "500"))
        .and(query_param("offset", "1000"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [{ "id": 3, "title": "T", "artists": [{ "id": 1, "name": "A" }] }],
            "limit": 500, "offset": 1000, "total": 1001
        })))
        .mount(&server)
        .await;
    let client = Client::new(&server.uri(), API, MEDIA).unwrap();
    let query = AlbumListQuery {
        sort: Some(AlbumSort::Random),
        order: Some(SortOrder::Desc),
        seed: Some(42),
        year_from: Some(1990),
        year_to: Some(1999),
        ..AlbumListQuery::default()
    };
    // limit is clamped to the server maximum.
    let page = client.albums(true, &query, 1000, 10_000).await.unwrap();
    assert_eq!(page.total, 1001);
    assert_eq!(page.items[0].artists[0].name, "A");
    // Missing fields decode to defaults.
    assert_eq!(page.items[0].track_count, 0);
}

#[test]
fn legacy_album_list_drops_what_it_does_not_understand() {
    let query = AlbumListQuery {
        sort: Some(AlbumSort::Random),
        order: Some(SortOrder::Desc),
        seed: Some(42),
        year_from: Some(1990),
        year_to: Some(1999),
        ..AlbumListQuery::default()
    };
    let (path, q) = query.source(false);
    assert_eq!(path, "/albums");
    assert!(q.is_empty(), "{q:?}");

    let exact_year = AlbumListQuery {
        sort: Some(AlbumSort::Added),
        year_from: Some(2001),
        year_to: Some(2001),
        ..AlbumListQuery::default()
    };
    let (_, q) = exact_year.source(false);
    assert_eq!(q.get("sort"), Some("added"));
    assert_eq!(q.get("year"), Some("2001"));
}

#[test]
fn track_list_order_is_only_sent_with_a_sort() {
    let only_order = TrackListQuery {
        order: Some(SortOrder::Asc),
        ..TrackListQuery::default()
    };
    assert_eq!(only_order.source(true).1.get("order"), None);

    let sorted = TrackListQuery {
        sort: Some(TrackSort::Duration),
        order: Some(SortOrder::Asc),
        seed: Some(1),
        ..TrackListQuery::default()
    };
    let (path, q) = sorted.source(true);
    assert_eq!(path, "/browse/tracks");
    assert_eq!(q.get("sort"), Some("duration"));
    assert_eq!(q.get("order"), Some("asc"));
    assert_eq!(q.get("seed"), None, "seed only rides on sort=random");

    let (path, q) = sorted.source(false);
    assert_eq!(path, "/tracks");
    assert_eq!(q.get("sort"), None);
}

#[tokio::test]
async fn legacy_tracks_endpoint_is_used_without_browse() {
    let server = MockServer::start().await;
    Mock::given(path("/api/v1/tracks"))
        .and(query_param("sort", "plays"))
        .and(query_param_is_missing("order"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [track_json(1)], "limit": 50, "offset": 0, "total": 1
        })))
        .mount(&server)
        .await;
    let client = Client::new(&server.uri(), API, MEDIA).unwrap();
    let query = TrackListQuery {
        sort: Some(TrackSort::Plays),
        order: Some(SortOrder::Desc),
        ..TrackListQuery::default()
    };
    let page = client.tracks(false, &query, 0, 50).await.unwrap();
    let t = &page.items[0];
    assert_eq!(t.title, "曲");
    assert_eq!(t.sample_rate, Some(48000));
    assert_eq!(t.bit_depth, Some(24));
    assert_eq!(t.channels, None);
}

#[tokio::test]
async fn lyrics_404_is_none() {
    let server = MockServer::start().await;
    Mock::given(path("/api/v1/tracks/1/lyrics"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "error": { "code": "not_found", "message": "no lyrics" }
        })))
        .mount(&server)
        .await;
    Mock::given(path("/api/v1/tracks/2/lyrics"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "lines": [{ "text": "一行目", "timeMs": 1200 }], "synced": true, "sourceName": "LRCLIB"
        })))
        .mount(&server)
        .await;
    let client = Client::new(&server.uri(), API, MEDIA).unwrap();
    assert_eq!(client.lyrics(1).await.unwrap(), None);
    let lyrics = client.lyrics(2).await.unwrap().unwrap();
    assert!(lyrics.synced);
    assert_eq!(lyrics.lines[0].time_ms, 1200);
    assert_eq!(lyrics.source_name.as_deref(), Some("LRCLIB"));
}

#[test]
fn media_urls_carry_the_media_token_never_the_api_token() {
    let client = Client::new("https://music.example.com/", API, "m t&").unwrap();
    let stream = client.stream_url(5);
    assert_eq!(
        stream.as_str(),
        "https://music.example.com/api/v1/tracks/5/stream?mediaToken=m+t%26"
    );
    assert!(!stream.as_str().contains(API));

    let art = client
        .image_url("/api/v1/artwork/9?v=2", Some(512))
        .unwrap();
    assert_eq!(
        art.as_str(),
        "https://music.example.com/api/v1/artwork/9?v=2&mediaToken=m+t%26&size=512"
    );
    // size 0 means "original": the server rejects size=0.
    let original = client.image_url("/api/v1/artwork/9", Some(0)).unwrap();
    assert!(!original.as_str().contains("size="));
    assert!(client.image_url("", Some(256)).is_none());

    // An existing mediaToken is replaced, not duplicated.
    let replaced = client
        .media_url("/api/v1/artwork/9?mediaToken=old", &[])
        .unwrap();
    assert_eq!(
        replaced.as_str(),
        "https://music.example.com/api/v1/artwork/9?mediaToken=m+t%26"
    );
    assert_eq!(
        client.artist_image_url(4, None).as_str(),
        "https://music.example.com/api/v1/artists/4/image?mediaToken=m+t%26"
    );
}

#[tokio::test]
async fn artists_go_through_browse_or_the_legacy_list() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/browse/artists"))
        .and(query_param("role", "album"))
        .and(query_param("sort", "name"))
        .and(query_param("order", "desc"))
        .and(query_param("offset", "0"))
        .and(query_param("limit", "20"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [{ "id": 3, "name": "YOASOBI", "imageUrl": "/api/v1/artists/3/image", "albumCount": 4, "trackCount": 31, "isFavorite": true }],
            "total": 1, "limit": 20, "offset": 0,
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/artists"))
        .and(query_param("role", "all"))
        .and(query_param_is_missing("sort"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [], "total": 0, "limit": 20, "offset": 0,
        })))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri(), API, MEDIA).unwrap();
    let browse = ArtistListQuery {
        album_artists_only: true,
        sort: Some(ArtistSort::Name),
        order: Some(SortOrder::Desc),
        ..ArtistListQuery::default()
    };
    let page = client.artists(true, &browse, 0, 20).await.unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].name, "YOASOBI");
    assert!(page.items[0].is_favorite);

    // Without browse the random sort is dropped rather than sent.
    let legacy = ArtistListQuery {
        album_artists_only: false,
        sort: Some(ArtistSort::Random),
        ..ArtistListQuery::default()
    };
    assert!(
        client
            .artists(false, &legacy, 0, 20)
            .await
            .unwrap()
            .items
            .is_empty()
    );
}

#[tokio::test]
async fn album_detail_pages_the_tracks_the_embed_did_not_return() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/albums/7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "album": {
                "id": 7, "title": "アルバム", "artist": "A", "trackCount": 3,
                "artworkUrl": "/api/v1/artwork/9", "genres": "rock, j-pop", "label": "L",
            },
            "tracks": [{ "id": 70, "albumId": 7, "title": "一曲目" }],
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/tracks"))
        .and(query_param("album", "7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [
                { "id": 70, "albumId": 7, "title": "一曲目", "codec": "flac", "bitDepth": 24, "sampleRate": 96000 },
                { "id": 71, "albumId": 7, "title": "二曲目", "codec": "flac", "bitDepth": 16, "sampleRate": 44100 },
                { "id": 72, "albumId": 7, "title": "三曲目", "codec": "mp3", "bitrateKbps": 320 },
            ],
            "total": 3, "limit": 500, "offset": 0,
        })))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri(), API, MEDIA).unwrap();
    let (album, embedded) = client.album(7).await.unwrap();
    assert_eq!((album.id, embedded.len()), (7, 1));
}

#[tokio::test]
async fn playlist_writes_send_the_revision_and_read_json_back() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/playlists/5/items/remove"))
        .and(header("authorization", format!("Bearer {API}").as_str()))
        .and(wiremock::matchers::body_json(json!({
            "trackIds": [3, 4], "expectedRevision": 11,
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "removed": 2, "total": 8, "revision": 12,
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/api/v1/playlists/5/order"))
        .and(wiremock::matchers::body_json(json!({
            "trackIds": [4, 3], "expectedRevision": 12,
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "revision": 13 })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/api/v1/playlists/5/artwork"))
        .and(header("content-type", "multipart/form-data; boundary="))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "playlist": { "id": 5, "name": "P", "revision": 14 },
        })))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri(), API, MEDIA).unwrap();
    let result = client.remove_playlist_items(5, &[3, 4], 11).await.unwrap();
    assert_eq!((result.removed, result.revision), (2, 12));
    let reordered = client.reorder_playlist_items(5, &[4, 3], 12).await.unwrap();
    assert_eq!(reordered.revision, 13);
    client
        .upload_playlist_artwork(5, b"jpegbytes".to_vec(), "cover.jpg")
        .await
        .unwrap_or_default();
}

#[tokio::test]
async fn stale_playlist_revision_is_a_recognizable_conflict() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/playlists/5/items/remove"))
        .respond_with(ResponseTemplate::new(409).set_body_json(json!({
            "error": { "code": "playlist_conflict", "message": "stale revision" }
        })))
        .mount(&server)
        .await;
    let client = Client::new(&server.uri(), API, MEDIA).unwrap();
    let error = client.remove_playlist_items(5, &[1], 3).await.unwrap_err();
    assert_eq!(
        (error.status(), error.code()),
        (Some(409), Some("playlist_conflict"))
    );
}

#[tokio::test]
async fn favorites_sort_is_client_side_and_similar_tracks_drop_errors() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/favorites/albums"))
        .and(query_param("limit", "500"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [{ "id": 1, "title": "A", "year": 2001 }],
            "total": 1, "limit": 500, "offset": 0,
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/tracks/9/similar"))
        .and(query_param("limit", "30"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [
                { "track": { "id": 10, "title": "近い" }, "distance": 0.2, "score": 0.8 },
                { "track": { "id": 9, "title": "自分" }, "distance": 0, "score": 1 },
            ],
        })))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri(), API, MEDIA).unwrap();
    let albums: Vec<api::Album> = client.favorites("albums").await.unwrap();
    assert_eq!(albums[0].id, 1);
    let similar = client.similar_tracks(9, 30).await.unwrap();
    assert_eq!(similar.len(), 2);
    assert_eq!(similar[0].track.id, 10);
}

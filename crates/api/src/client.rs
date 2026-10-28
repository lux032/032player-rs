use reqwest::{Method, StatusCode, header};
use serde::Serialize;
use serde::de::DeserializeOwned;
use url::Url;

use crate::error::{ApiError, AuthError};
use crate::query::{AlbumListQuery, ArtistListQuery, Query, TrackListQuery};
use crate::types::{
    Album, Artist, ArtistDetail, Capabilities, ErrorBody, Lyrics, Page, Playlist, PlaylistDetail,
    PlaylistItemResult, SimilarTrack, Track,
};

/// Largest page the 032 list endpoints accept (`capabilities.limits.maxPageSize`).
pub const MAX_PAGE_SIZE: u32 = 500;

/// apiRevision this client implements (playback session protocol).
pub const API_REVISION: i64 = 3;

/// HTTP client for one 032 Music Server.
///
/// The API token goes in the `Authorization` header only; media URLs
/// (stream, artwork) carry the separate media token as a query parameter.
#[derive(Clone, Debug)]
pub struct Client {
    http: reqwest::Client,
    base_url: String,
    api_token: String,
    media_token: String,
}

/// Turns a user-entered server URL into the base URL (no trailing slash).
pub fn normalize_base_url(url: &str) -> Result<String, ApiError> {
    let trimmed = url.trim().trim_end_matches('/');
    let parsed = Url::parse(trimmed).map_err(|e| ApiError::Url(e.to_string()))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host().is_none() {
        return Err(ApiError::Url(format!("not an http(s) URL: {trimmed}")));
    }
    Ok(trimmed.to_owned())
}

fn set_param(url: &mut Url, key: &str, value: &str) {
    let mut pairs: Vec<(String, String)> = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    let mut found = false;
    pairs.retain_mut(|(k, v)| {
        if k != key {
            return true;
        }
        if found {
            return false;
        }
        found = true;
        value.clone_into(v);
        true
    });
    if !found {
        pairs.push((key.to_owned(), value.to_owned()));
    }
    url.query_pairs_mut().clear().extend_pairs(pairs);
}

impl Client {
    pub fn new(
        base_url: &str,
        api_token: impl Into<String>,
        media_token: impl Into<String>,
    ) -> Result<Self, ApiError> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("032player-rs/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Self::with_http(http, base_url, api_token, media_token)
    }

    pub fn with_http(
        http: reqwest::Client,
        base_url: &str,
        api_token: impl Into<String>,
        media_token: impl Into<String>,
    ) -> Result<Self, ApiError> {
        Ok(Self {
            http,
            base_url: normalize_base_url(base_url)?,
            api_token: api_token.into(),
            media_token: media_token.into(),
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn api_token(&self) -> &str {
        &self.api_token
    }

    pub fn media_token(&self) -> &str {
        &self.media_token
    }

    // ------------------------------------------------------------ requests

    fn api_url(&self, path: &str, query: &Query) -> Result<Url, ApiError> {
        let mut url = Url::parse(&format!("{}/api/v1{path}", self.base_url))
            .map_err(|e| ApiError::Url(e.to_string()))?;
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query.pairs());
        }
        Ok(url)
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        query: &Query,
        body: Option<&(dyn erased::Body + Sync)>,
    ) -> Result<reqwest::Response, ApiError> {
        let url = self.api_url(path, query)?;
        let mut req = self
            .http
            .request(method, url)
            .header(header::ACCEPT, "application/json")
            .bearer_auth(&self.api_token);
        if let Some(body) = body {
            req = req
                .header(header::CONTENT_TYPE, "application/json")
                .body(body.to_json()?);
        }
        let res = req.send().await?;
        if res.status().is_success() {
            return Ok(res);
        }
        Err(error_from_response(res).await)
    }

    /// `GET /api/v1{path}` decoding a JSON body.
    pub async fn get<T: DeserializeOwned>(&self, path: &str, query: &Query) -> Result<T, ApiError> {
        let res = self.send(Method::GET, path, query, None).await?;
        decode(res).await
    }

    /// Request with a JSON body, decoding a JSON response.
    pub async fn send_json<B: Serialize + Sync, T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: &B,
    ) -> Result<T, ApiError> {
        let res = self
            .send(method, path, &Query::new(), Some(&erased::Json(body)))
            .await?;
        decode(res).await
    }

    // ------------------------------------------------------------ endpoints

    pub async fn capabilities(&self) -> Result<Capabilities, ApiError> {
        self.get("/capabilities", &Query::new()).await
    }

    /// Albums via `/browse/albums`, or legacy `/albums` when `browse` is false.
    pub async fn albums(
        &self,
        browse: bool,
        query: &AlbumListQuery,
        offset: u32,
        limit: u32,
    ) -> Result<Page<Album>, ApiError> {
        let (path, mut q) = query.source(browse);
        q.page(offset, limit);
        self.get(path, &q).await
    }

    /// Tracks via `/browse/tracks`, or legacy `/tracks` when `browse` is false.
    pub async fn tracks(
        &self,
        browse: bool,
        query: &TrackListQuery,
        offset: u32,
        limit: u32,
    ) -> Result<Page<Track>, ApiError> {
        let (path, mut q) = query.source(browse);
        q.page(offset, limit);
        self.get(path, &q).await
    }

    /// `POST /api/v1/playback/events`. Any HTTP status is a result (the
    /// session interprets it); only network failures are errors.
    /// `keepalive` (app close) authenticates with the media token in the
    /// query, like the web client's `sendBeacon`.
    pub async fn post_playback_event<B: Serialize + Sync>(
        &self,
        body: &B,
        keepalive: bool,
    ) -> Result<(u16, serde_json::Value), ApiError> {
        let mut url = self.api_url("/playback/events", &Query::new())?;
        let req = if keepalive {
            set_param(&mut url, "mediaToken", &self.media_token);
            self.http.post(url)
        } else {
            self.http.post(url).bearer_auth(&self.api_token)
        };
        let body = serde_json::to_vec(body).map_err(|e| ApiError::Decode(e.to_string()))?;
        let res = req
            .header(header::ACCEPT, "application/json")
            .header(header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await?;
        let status = res.status().as_u16();
        let json = res
            .bytes()
            .await
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or(serde_json::Value::Null);
        Ok((status, json))
    }

    // ------------------------------------------------------------ catalog

    /// Artists via `/browse/artists`, or legacy `/artists` without `browse`.
    pub async fn artists(
        &self,
        browse: bool,
        query: &ArtistListQuery,
        offset: u32,
        limit: u32,
    ) -> Result<Page<Artist>, ApiError> {
        let (path, mut q) = query.source(browse);
        q.page(offset, limit);
        self.get(path, &q).await
    }

    /// `GET /artists/{id}`: the artist, its discography and top tracks.
    pub async fn artist(&self, artist_id: i64) -> Result<ArtistDetail, ApiError> {
        self.get(&format!("/artists/{artist_id}"), &Query::new())
            .await
    }

    /// `GET /albums/{id}`: the album and up to 500 of its tracks.
    pub async fn album(&self, album_id: i64) -> Result<(Album, Vec<Track>), ApiError> {
        let body: AlbumDetailBody = self
            .get(&format!("/albums/{album_id}"), &Query::new())
            .await?;
        Ok((body.album, body.tracks))
    }

    /// `GET /playlists`, whole list in pages.
    pub async fn playlists(&self) -> Result<Vec<Playlist>, ApiError> {
        let mut out = Vec::new();
        loop {
            let offset = u32::try_from(out.len()).unwrap_or(u32::MAX);
            let mut q = Query::new();
            q.page(offset, MAX_PAGE_SIZE);
            let page: Page<Playlist> = self.get("/playlists", &q).await?;
            let n = page.items.len();
            out.extend(page.items);
            if n == 0 || i64::try_from(out.len()).unwrap_or(i64::MAX) >= page.total {
                return Ok(out);
            }
        }
    }

    /// `GET /playlists/{id}`: the playlist and its tracks in order.
    pub async fn playlist(&self, playlist_id: i64) -> Result<PlaylistDetail, ApiError> {
        self.get(&format!("/playlists/{playlist_id}"), &Query::new())
            .await
    }

    /// `POST /playlists`; returns the created playlist.
    pub async fn create_playlist(
        &self,
        name: &str,
        description: &str,
    ) -> Result<Playlist, ApiError> {
        let body = serde_json::json!({ "name": name, "description": description });
        self.send_json(Method::POST, "/playlists", &body).await
    }

    /// `PATCH /playlists/{id}`; returns the updated playlist.
    pub async fn update_playlist(
        &self,
        playlist_id: i64,
        name: &str,
        description: &str,
    ) -> Result<Playlist, ApiError> {
        let body = serde_json::json!({ "name": name, "description": description });
        self.send_json(Method::PATCH, &format!("/playlists/{playlist_id}"), &body)
            .await
    }

    /// `DELETE /playlists/{id}`.
    pub async fn delete_playlist(&self, playlist_id: i64) -> Result<(), ApiError> {
        self.send(
            Method::DELETE,
            &format!("/playlists/{playlist_id}"),
            &Query::new(),
            None,
        )
        .await?;
        Ok(())
    }

    /// `PUT /playlists/{id}/items`: replaces the whole ordered content.
    pub async fn replace_playlist_items(
        &self,
        playlist_id: i64,
        track_ids: &[i64],
    ) -> Result<PlaylistDetail, ApiError> {
        let body = serde_json::json!({ "trackIds": track_ids });
        self.send_json(
            Method::PUT,
            &format!("/playlists/{playlist_id}/items"),
            &body,
        )
        .await
    }

    /// `POST /playlists/{id}/items/append` (first occurrence order).
    pub async fn append_playlist_items(
        &self,
        playlist_id: i64,
        track_ids: &[i64],
    ) -> Result<PlaylistItemResult, ApiError> {
        let body = serde_json::json!({ "trackIds": track_ids });
        self.send_json(
            Method::POST,
            &format!("/playlists/{playlist_id}/items/append"),
            &body,
        )
        .await
    }

    /// `POST /playlists/{id}/items/append` with an album.
    pub async fn append_playlist_album(
        &self,
        playlist_id: i64,
        album_id: i64,
    ) -> Result<PlaylistItemResult, ApiError> {
        let body = serde_json::json!({ "albumId": album_id });
        self.send_json(
            Method::POST,
            &format!("/playlists/{playlist_id}/items/append"),
            &body,
        )
        .await
    }

    /// `POST /playlists/{id}/items/remove`; 409 `playlist_conflict` when
    /// `expected_revision` is stale.
    pub async fn remove_playlist_items(
        &self,
        playlist_id: i64,
        track_ids: &[i64],
        expected_revision: i64,
    ) -> Result<PlaylistItemResult, ApiError> {
        let body = serde_json::json!({
            "trackIds": track_ids,
            "expectedRevision": expected_revision,
        });
        self.send_json(
            Method::POST,
            &format!("/playlists/{playlist_id}/items/remove"),
            &body,
        )
        .await
    }

    /// `PUT /playlists/{id}/order`: `track_ids` must hold every track once.
    pub async fn reorder_playlist_items(
        &self,
        playlist_id: i64,
        track_ids: &[i64],
        expected_revision: i64,
    ) -> Result<PlaylistItemResult, ApiError> {
        let body = serde_json::json!({
            "trackIds": track_ids,
            "expectedRevision": expected_revision,
        });
        self.send_json(
            Method::PUT,
            &format!("/playlists/{playlist_id}/order"),
            &body,
        )
        .await
    }

    /// `DELETE /playlists/{id}/artwork`: back to the automatic album artwork.
    pub async fn delete_playlist_artwork(&self, playlist_id: i64) -> Result<(), ApiError> {
        self.send(
            Method::DELETE,
            &format!("/playlists/{playlist_id}/artwork"),
            &Query::new(),
            None,
        )
        .await?;
        Ok(())
    }

    /// `PUT /playlists/{id}/artwork` with a multipart `image` field
    /// (JPEG / PNG / WebP, at most 10 MiB).
    pub async fn upload_playlist_artwork(
        &self,
        playlist_id: i64,
        bytes: Vec<u8>,
        filename: &str,
    ) -> Result<Playlist, ApiError> {
        let part = reqwest::multipart::Part::bytes(bytes).file_name(filename.to_owned());
        let form = reqwest::multipart::Form::new().part("image", part);
        let url = self.api_url(&format!("/playlists/{playlist_id}/artwork"), &Query::new())?;
        let res = self
            .http
            .put(url)
            .header(header::ACCEPT, "application/json")
            .bearer_auth(&self.api_token)
            .multipart(form)
            .send()
            .await?;
        if !res.status().is_success() {
            return Err(error_from_response(res).await);
        }
        decode(res).await
    }

    /// `PUT` / `DELETE /{segment}/{id}/favorite` for albums, tracks and
    /// artists; a server without the endpoint fails with 404 / 405.
    pub async fn set_favorite(
        &self,
        segment: &str,
        id: i64,
        favorite: bool,
    ) -> Result<(), ApiError> {
        let method = if favorite {
            Method::PUT
        } else {
            Method::DELETE
        };
        self.send(
            method,
            &format!("/{segment}/{id}/favorite"),
            &Query::new(),
            None,
        )
        .await?;
        Ok(())
    }

    /// `GET /favorites/{segment}` (albums or tracks), whole list in pages.
    pub async fn favorites<T: DeserializeOwned>(&self, segment: &str) -> Result<Vec<T>, ApiError> {
        let mut out = Vec::new();
        loop {
            let offset = u32::try_from(out.len()).unwrap_or(u32::MAX);
            let mut q = Query::new();
            q.page(offset, MAX_PAGE_SIZE);
            let page: Page<T> = self.get(&format!("/favorites/{segment}"), &q).await?;
            let n = page.items.len();
            out.extend(page.items);
            if n == 0 || i64::try_from(out.len()).unwrap_or(i64::MAX) >= page.total {
                return Ok(out);
            }
        }
    }

    /// `GET /tracks/{id}/similar` (metadata similarity, not acoustic).
    pub async fn similar_tracks(
        &self,
        track_id: i64,
        limit: u32,
    ) -> Result<Vec<SimilarTrack>, ApiError> {
        let mut q = Query::new();
        q.push("limit", limit.clamp(1, 100));
        let body: SimilarBody = self.get(&format!("/tracks/{track_id}/similar"), &q).await?;
        Ok(body.items)
    }

    /// Lyrics of a track; `None` when the server has none (404).
    pub async fn lyrics(&self, track_id: i64) -> Result<Option<Lyrics>, ApiError> {
        match self
            .get(&format!("/tracks/{track_id}/lyrics"), &Query::new())
            .await
        {
            Ok(lyrics) => Ok(Some(lyrics)),
            Err(e) if e.status() == Some(404) => Ok(None),
            Err(e) => Err(e),
        }
    }

    // ------------------------------------------------------------ media URLs

    /// Turns a server-relative media path (artwork, artist image, stream)
    /// into an absolute URL carrying the media token. Empty paths give `None`.
    pub fn media_url(&self, path: &str, params: &[(&str, String)]) -> Option<Url> {
        if path.is_empty() {
            return None;
        }
        let base = Url::parse(&format!("{}/", self.base_url)).ok()?;
        let mut url = base.join(path).ok()?;
        set_param(&mut url, "mediaToken", &self.media_token);
        for (key, value) in params {
            set_param(&mut url, key, value);
        }
        Some(url)
    }

    /// Original file stream (what makes bit-perfect output possible).
    pub fn stream_url(&self, track_id: i64) -> Url {
        self.media_url(&format!("/api/v1/tracks/{track_id}/stream"), &[])
            .expect("base URL was validated")
    }

    /// Artwork / image URL. `size` `None` or `0` requests the original:
    /// the server rejects `size=0`.
    pub fn image_url(&self, path: &str, size: Option<u32>) -> Option<Url> {
        match size {
            Some(size) if size > 0 => self.media_url(path, &[("size", size.to_string())]),
            _ => self.media_url(path, &[]),
        }
    }

    pub fn artist_image_url(&self, artist_id: i64, size: Option<u32>) -> Url {
        self.image_url(&format!("/api/v1/artists/{artist_id}/image"), size)
            .expect("base URL was validated")
    }

    pub fn playlist_artwork_url(&self, playlist_id: i64, size: Option<u32>) -> Url {
        self.image_url(&format!("/api/v1/playlists/{playlist_id}/artwork"), size)
            .expect("base URL was validated")
    }

    /// Fetches a media URL with the shared HTTP client (images, probes).
    pub async fn fetch_media(&self, url: Url) -> Result<reqwest::Response, ApiError> {
        Ok(self.http.get(url).send().await?)
    }

    // ------------------------------------------------------------ auth

    /// Probes the media token: Range-requests the first byte of one track.
    /// An empty library cannot be probed and passes.
    async fn verify_media_token(&self) -> Result<(), AuthError> {
        let mut q = Query::new();
        q.push("limit", 1);
        let page: Page<Track> = self.get("/tracks", &q).await?;
        let Some(track) = page.items.first() else {
            return Ok(());
        };
        let Some(url) = self.media_url(&track.stream_url, &[]) else {
            return Ok(());
        };
        let res = self
            .http
            .get(url)
            .header(header::RANGE, "bytes=0-0")
            .send()
            .await
            .map_err(ApiError::from)?;
        if res.status() == StatusCode::UNAUTHORIZED {
            return Err(AuthError::MediaTokenInvalid);
        }
        Ok(())
    }
}

/// Signs in: validates the credentials, the API revision and the media token.
pub async fn authenticate(
    server_url: &str,
    api_token: &str,
    media_token: &str,
) -> Result<(Client, Capabilities), AuthError> {
    let client = Client::new(server_url, api_token.trim(), media_token.trim())?;
    authenticate_client(client).await
}

/// [`authenticate`] with a preconfigured client (custom HTTP settings, tests).
pub async fn authenticate_client(client: Client) -> Result<(Client, Capabilities), AuthError> {
    if client.api_token.is_empty() || client.media_token.is_empty() {
        return Err(AuthError::CredentialsRequired);
    }
    if client.api_token == client.media_token {
        return Err(AuthError::TokensMustDiffer);
    }
    let capabilities = match client.capabilities().await {
        Ok(c) => c,
        Err(e) if e.is_unauthorized() => return Err(AuthError::ApiTokenInvalid),
        Err(e) => return Err(e.into()),
    };
    if capabilities.api_revision != API_REVISION {
        return Err(AuthError::UnsupportedRevision(capabilities.api_revision));
    }
    client.verify_media_token().await?;
    Ok((client, capabilities))
}

#[derive(serde::Deserialize)]
struct AlbumDetailBody {
    #[serde(default)]
    album: Album,
    #[serde(default)]
    tracks: Vec<Track>,
}

#[derive(serde::Deserialize)]
struct SimilarBody {
    #[serde(default)]
    items: Vec<SimilarTrack>,
}

async fn decode<T: DeserializeOwned>(res: reqwest::Response) -> Result<T, ApiError> {
    let bytes = res.bytes().await?;
    serde_json::from_slice(&bytes).map_err(|e| ApiError::Decode(e.to_string()))
}

async fn error_from_response(res: reqwest::Response) -> ApiError {
    let status = res.status().as_u16();
    let body: ErrorBody = res
        .bytes()
        .await
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let detail = body.error.unwrap_or_default();
    let message = detail
        .message
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| format!("032 Music Server request failed ({status})"));
    ApiError::Http {
        status,
        code: detail.code,
        message,
    }
}

/// Object-safe JSON body so `send` is not generic.
mod erased {
    use crate::error::ApiError;

    pub trait Body {
        fn to_json(&self) -> Result<Vec<u8>, ApiError>;
    }

    pub struct Json<'a, T>(pub &'a T);

    impl<T: serde::Serialize> Body for Json<'_, T> {
        fn to_json(&self) -> Result<Vec<u8>, ApiError> {
            serde_json::to_vec(self.0).map_err(|e| ApiError::Decode(e.to_string()))
        }
    }
}

//! Catalog state: the paged lists (albums, tracks, artists, playlists,
//! favorites), the detail pages (album, artist, playlist), search and
//! similar tracks. Mirrors `music032-controller.ts`: browse endpoints when
//! the server has `features.browse`, otherwise the legacy lists with
//! client-side sorting for the favorites and playlists.

use std::collections::VecDeque;

pub use api::SortOrder;
use api::{
    Album, AlbumListQuery, AlbumSort, ApiError, Artist, ArtistListQuery, ArtistSort, Capabilities,
    Client, Feature, Playlist, SimilarTrack, Track, TrackListQuery, TrackSort,
};
use serde::{Deserialize, Serialize};

use crate::format;

/// Rows requested per page (the server's `limits.maxPageSize`).
pub const PAGE: u32 = api::MAX_PAGE_SIZE;
/// Random seeds remembered (`MAX_RANDOM_SEEDS` in the Electron client).
const MAX_SEEDS: usize = 50;
/// Items fetched per source by a search.
const SEARCH_LIMIT: u32 = 20;

/// One paged list of the sidebar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ListKind {
    #[default]
    Albums,
    Tracks,
    Artists,
    Playlists,
    FavoriteAlbums,
    FavoriteTracks,
    /// The full track list of one artist (artist detail "全部曲目").
    ArtistTracks,
}

impl ListKind {
    pub fn is_tracks(self) -> bool {
        matches!(
            self,
            Self::Tracks | Self::FavoriteTracks | Self::ArtistTracks
        )
    }

    /// The list's initial query (its first sort option).
    pub fn default_query(self) -> ListQuery {
        let sort = match self {
            Self::Albums | Self::FavoriteAlbums => ListSort::Added,
            Self::Tracks | Self::FavoriteTracks | Self::ArtistTracks => ListSort::Added,
            Self::Artists => ListSort::Name,
            Self::Playlists => ListSort::Name,
        };
        let order = match self {
            Self::Albums | Self::FavoriteAlbums => SortOrder::Desc,
            Self::Tracks | Self::FavoriteTracks | Self::ArtistTracks => SortOrder::Desc,
            Self::Artists | Self::Playlists => SortOrder::Asc,
        };
        ListQuery {
            kind: self,
            sort,
            order,
            ..ListQuery::default()
        }
    }
}

/// Sort keys of the lists. Each list kind offers a subset, in this order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ListSort {
    #[default]
    Added,
    Title,
    RecentlyPlayed,
    ReleaseDate,
    Year,
    /// Tracks only.
    Album,
    /// Tracks only.
    Plays,
    /// Tracks, playlists.
    Duration,
    Name,
    AlbumCount,
    TrackCount,
    Random,
}

/// The sort keys a list kind offers (browse-only keys are filtered out when
/// the server has no `/browse/*`).
pub fn sort_options(kind: ListKind, browse: bool) -> Vec<ListSort> {
    let browse_only = |sort: ListSort| {
        !browse
            && matches!(
                sort,
                ListSort::Random | ListSort::Title | ListSort::Album | ListSort::Duration
            )
    };
    let all: &[ListSort] = match kind {
        ListKind::Albums | ListKind::FavoriteAlbums => &[
            ListSort::Added,
            ListSort::Title,
            ListSort::RecentlyPlayed,
            ListSort::ReleaseDate,
            ListSort::Year,
            ListSort::Random,
        ],
        ListKind::Tracks | ListKind::FavoriteTracks | ListKind::ArtistTracks => &[
            ListSort::Added,
            ListSort::Album,
            ListSort::Title,
            ListSort::Plays,
            ListSort::RecentlyPlayed,
            ListSort::ReleaseDate,
            ListSort::Year,
            ListSort::Duration,
            ListSort::Random,
        ],
        ListKind::Artists => &[
            ListSort::Name,
            ListSort::AlbumCount,
            ListSort::TrackCount,
            ListSort::Random,
        ],
        ListKind::Playlists => &[ListSort::Name, ListSort::Duration, ListSort::TrackCount],
    };
    all.iter().copied().filter(|s| !browse_only(*s)).collect()
}

/// A list request: kind, ordering and filters.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ListQuery {
    pub kind: ListKind,
    pub sort: ListSort,
    pub order: SortOrder,
    /// `q`, matched by the server (albums, tracks, artists).
    pub search: String,
    pub year_from: Option<i32>,
    pub year_to: Option<i32>,
    /// `artist`, albums / tracks only.
    pub artist_id: Option<i64>,
    /// Only the favorites (`favorite=true` where the server supports it).
    pub favorite: bool,
}

impl ListQuery {
    /// Identity of the query without paging: the random seed key.
    fn key(&self) -> String {
        format!(
            "{:?}|{:?}|{:?}|{}|{:?}|{:?}|{:?}|{}",
            self.kind,
            self.sort,
            self.order,
            self.search,
            self.year_from,
            self.year_to,
            self.artist_id,
            self.favorite
        )
    }
}

/// A row of a list.
#[derive(Clone, Debug, PartialEq)]
pub enum ListItem {
    Album(AlbumItem),
    Track(TrackItem),
    Artist(ArtistItem),
    Playlist(PlaylistItem),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AlbumItem {
    pub id: i64,
    pub title: String,
    pub artist: String,
    pub year: i32,
    /// Artwork path (server relative) or empty.
    pub artwork: String,
    pub track_count: i64,
    pub favorite: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrackItem {
    pub id: i64,
    pub album_id: i64,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: String,
    pub artwork: String,
    pub favorite: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArtistItem {
    pub id: i64,
    pub name: String,
    pub album_count: i64,
    pub track_count: i64,
    /// Artist image path (server relative) or empty.
    pub artwork: String,
    pub favorite: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlaylistItem {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub item_count: i64,
    /// Hidden when the server cannot tell (0).
    pub duration: String,
    /// Artwork path (server relative) or empty.
    pub artwork: String,
}

/// One item's favorite state to flip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FavoriteKind {
    Album,
    Track,
    Artist,
}

impl FavoriteKind {
    pub fn segment(self) -> &'static str {
        match self {
            Self::Album => "albums",
            Self::Track => "tracks",
            Self::Artist => "artists",
        }
    }
}

/// A page of one list, plus the tracks behind it (for playback).
pub struct Page {
    pub items: Vec<ListItem>,
    pub tracks: Vec<Track>,
    pub total: usize,
}

pub fn album_item(album: &Album) -> AlbumItem {
    AlbumItem {
        id: album.id,
        title: album.title.clone(),
        artist: album.artist.clone(),
        year: album.year,
        artwork: album.artwork_url.clone(),
        track_count: album.track_count,
        favorite: album.is_favorite,
    }
}

pub fn track_item(track: &Track) -> TrackItem {
    TrackItem {
        id: track.id,
        album_id: track.album_id,
        title: track.title.clone(),
        artist: track.artist.clone(),
        album: track.album.clone(),
        duration: format::duration(track.duration_millis),
        artwork: track.artwork_url.clone(),
        favorite: track.is_favorite,
    }
}

pub fn artist_item(artist: &Artist) -> ArtistItem {
    ArtistItem {
        id: artist.id,
        name: artist.name.clone(),
        album_count: artist.album_count,
        track_count: artist.track_count,
        artwork: artist.image_url.clone(),
        favorite: artist.is_favorite,
    }
}

pub fn playlist_item(playlist: &Playlist) -> PlaylistItem {
    PlaylistItem {
        id: playlist.id,
        name: playlist.name.clone(),
        description: playlist.description.clone(),
        item_count: playlist.item_count,
        duration: if playlist.duration_millis > 0 {
            format::duration(playlist.duration_millis)
        } else {
            String::new()
        },
        artwork: playlist.artwork_url.clone().unwrap_or_default(),
    }
}

/// Remembers the `seed` of each `sort=random` query; a new one is drawn when
/// the list is loaded from the top again.
#[derive(Default)]
pub struct Seeds(VecDeque<(String, i64)>);

impl Seeds {
    /// Seed for `query` at `offset` (`None` when the sort is not random).
    pub fn for_query(&mut self, query: &ListQuery, offset: u32) -> Option<i64> {
        if query.sort != ListSort::Random {
            return None;
        }
        let key = query.key();
        if let Some(pos) = self.0.iter().position(|(k, _)| *k == key)
            && offset != 0
        {
            return Some(self.0[pos].1);
        }
        let seed = new_seed();
        self.0.retain(|(k, _)| *k != key);
        self.0.push_back((key, seed));
        while self.0.len() > MAX_SEEDS {
            self.0.pop_front();
        }
        Some(seed)
    }
}

/// Random positive seed without an RNG dependency. A counter is mixed in so
/// two calls in the same clock tick still differ (Windows ticks are coarse).
pub fn new_seed() -> i64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mixed = (u64::try_from(now & u128::from(u64::MAX)).unwrap_or(0)
        ^ count.wrapping_mul(0x9e37_79b9_7f4a_7c15))
    .wrapping_mul(0x2545_f491_4f6c_dd1d)
        >> 33;
    i64::try_from(mixed % (1 << 31)).unwrap_or(0)
}

fn album_sort(sort: ListSort) -> Option<AlbumSort> {
    Some(match sort {
        ListSort::Added => AlbumSort::Added,
        ListSort::Title => AlbumSort::Title,
        ListSort::RecentlyPlayed => AlbumSort::RecentlyPlayed,
        ListSort::ReleaseDate => AlbumSort::Date,
        ListSort::Year => AlbumSort::Year,
        ListSort::Random => AlbumSort::Random,
        _ => return None,
    })
}

fn track_sort(sort: ListSort) -> Option<TrackSort> {
    Some(match sort {
        ListSort::Added => TrackSort::Added,
        ListSort::Album => TrackSort::Album,
        ListSort::Title => TrackSort::Title,
        ListSort::Plays => TrackSort::Plays,
        ListSort::RecentlyPlayed => TrackSort::RecentlyPlayed,
        ListSort::ReleaseDate => TrackSort::Date,
        ListSort::Year => TrackSort::Year,
        ListSort::Duration => TrackSort::Duration,
        ListSort::Random => TrackSort::Random,
        _ => return None,
    })
}

fn artist_sort(sort: ListSort) -> Option<ArtistSort> {
    Some(match sort {
        ListSort::Name => ArtistSort::Name,
        ListSort::AlbumCount => ArtistSort::AlbumCount,
        ListSort::TrackCount => ArtistSort::TrackCount,
        ListSort::Random => ArtistSort::Random,
        _ => return None,
    })
}

fn album_query(query: &ListQuery, browse: bool) -> AlbumListQuery {
    AlbumListQuery {
        artist_id: query.artist_id,
        genre: None,
        search: non_empty(&query.search),
        sort: album_sort(query.sort),
        order: browse.then_some(query.order),
        year_from: query.year_from,
        year_to: query.year_to,
        seed: None,
    }
}

fn track_query(query: &ListQuery, browse: bool) -> TrackListQuery {
    TrackListQuery {
        album_id: None,
        artist_id: query.artist_id,
        genre: None,
        search: non_empty(&query.search),
        sort: track_sort(query.sort),
        order: browse.then_some(query.order),
        year_from: query.year_from,
        year_to: query.year_to,
        seed: None,
    }
}

fn artist_query(query: &ListQuery, browse: bool) -> ArtistListQuery {
    ArtistListQuery {
        album_artists_only: true,
        favorite: query.favorite,
        search: non_empty(&query.search),
        sort: artist_sort(query.sort),
        order: browse.then_some(query.order),
        seed: None,
    }
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// Fetches one page of `query`. `seed` comes from [`Seeds::for_query`]
/// (`sort=random` only).
pub async fn fetch_page(
    client: &Client,
    capabilities: &Capabilities,
    query: &ListQuery,
    offset: u32,
    seed: Option<i64>,
) -> Result<Page, ApiError> {
    let browse = capabilities.has(Feature::Browse);
    // "Only favorites" on a normal list reads the favorites endpoints, which
    // the server filters itself.
    if query.favorite {
        return match query.kind {
            ListKind::Albums => {
                let all = favorite_albums(client, query, seed).await?;
                Ok(client_page(
                    all.iter().map(|a| ListItem::Album(album_item(a))).collect(),
                    offset,
                ))
            }
            ListKind::Tracks | ListKind::ArtistTracks => {
                let all = favorite_tracks(client, query, seed).await?;
                let start = usize::try_from(offset).unwrap_or(0).min(all.len());
                let end = (start + usize::try_from(PAGE).unwrap_or(0)).min(all.len());
                let tracks: Vec<Track> = all[start..end].to_vec();
                Ok(Page {
                    items: tracks
                        .iter()
                        .map(|t| ListItem::Track(track_item(t)))
                        .collect(),
                    tracks,
                    total: all.len(),
                })
            }
            _ => sorted_artist_page(client, capabilities, query, offset, seed).await,
        };
    }
    match query.kind {
        ListKind::Albums => {
            let mut q = album_query(query, browse);
            q.seed = seed;
            let page = client.albums(browse, &q, offset, PAGE).await?;
            Ok(Page {
                items: page
                    .items
                    .iter()
                    .map(|a| ListItem::Album(album_item(a)))
                    .collect(),
                tracks: Vec::new(),
                total: usize::try_from(page.total).unwrap_or(0),
            })
        }
        ListKind::Tracks | ListKind::ArtistTracks => {
            let mut q = track_query(query, browse);
            q.seed = seed;
            let page = client.tracks(browse, &q, offset, PAGE).await?;
            Ok(Page {
                items: page
                    .items
                    .iter()
                    .map(|t| ListItem::Track(track_item(t)))
                    .collect(),
                tracks: page.items,
                total: usize::try_from(page.total).unwrap_or(0),
            })
        }
        ListKind::Artists => {
            let mut q = artist_query(query, browse);
            q.seed = seed;
            let page = client.artists(browse, &q, offset, PAGE).await?;
            Ok(Page {
                items: page
                    .items
                    .iter()
                    .map(|a| ListItem::Artist(artist_item(a)))
                    .collect(),
                tracks: Vec::new(),
                total: usize::try_from(page.total).unwrap_or(0),
            })
        }
        ListKind::Playlists => {
            let all = sorted_playlists(client, query, seed).await?;
            Ok(client_page(
                all.into_iter()
                    .map(|p| ListItem::Playlist(playlist_item(&p)))
                    .collect(),
                offset,
            ))
        }
        ListKind::FavoriteAlbums => {
            let all = favorite_albums(client, query, seed).await?;
            Ok(client_page(
                all.iter().map(|a| ListItem::Album(album_item(a))).collect(),
                offset,
            ))
        }
        ListKind::FavoriteTracks => {
            // The favorite list is loaded whole by the caller (playback needs
            // the tracks), so it is fetched in one page.
            let all = favorite_tracks(client, query, seed).await?;
            let start = usize::try_from(offset).unwrap_or(0).min(all.len());
            let end = (start + usize::try_from(PAGE).unwrap_or(0)).min(all.len());
            let tracks: Vec<Track> = all[start..end].to_vec();
            Ok(Page {
                items: tracks
                    .iter()
                    .map(|t| ListItem::Track(track_item(t)))
                    .collect(),
                tracks,
                total: all.len(),
            })
        }
    }
}

/// Artists (the `favorite` filter is honoured by `/artists` itself).
async fn sorted_artist_page(
    client: &Client,
    capabilities: &Capabilities,
    query: &ListQuery,
    offset: u32,
    seed: Option<i64>,
) -> Result<Page, ApiError> {
    let browse = capabilities.has(Feature::Browse);
    let mut q = artist_query(query, browse);
    q.seed = seed;
    let page = client.artists(browse, &q, offset, PAGE).await?;
    Ok(Page {
        items: page
            .items
            .iter()
            .map(|a| ListItem::Artist(artist_item(a)))
            .collect(),
        tracks: Vec::new(),
        total: usize::try_from(page.total).unwrap_or(0),
    })
}

/// A page of a list that is fully loaded and filtered client-side.
fn client_page(all: Vec<ListItem>, offset: u32) -> Page {
    let start = usize::try_from(offset).unwrap_or(0).min(all.len());
    let end = (start + usize::try_from(PAGE).unwrap_or(0)).min(all.len());
    Page {
        items: all[start..end].to_vec(),
        tracks: Vec::new(),
        total: all.len(),
    }
}

/// Playlists, searched and sorted client-side (the endpoint only pages).
async fn sorted_playlists(
    client: &Client,
    query: &ListQuery,
    seed: Option<i64>,
) -> Result<Vec<Playlist>, ApiError> {
    let mut items = client.playlists().await?;
    let needle = query.search.trim().to_lowercase();
    if !needle.is_empty() {
        items.retain(|p| p.name.to_lowercase().contains(&needle));
    }
    if query.sort == ListSort::Random {
        shuffle_seeded(&mut items, seed.unwrap_or(0));
        return Ok(items);
    }
    let desc = query.order == SortOrder::Desc;
    items.sort_by(|a, b| {
        let ordering = match query.sort {
            ListSort::Duration => a.duration_millis.cmp(&b.duration_millis),
            ListSort::TrackCount => a.item_count.cmp(&b.item_count),
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        };
        if desc { ordering.reverse() } else { ordering }
    });
    Ok(items)
}

/// `GET /favorites/albums`, filtered and sorted client-side.
async fn favorite_albums(
    client: &Client,
    query: &ListQuery,
    seed: Option<i64>,
) -> Result<Vec<Album>, ApiError> {
    let mut items: Vec<Album> = client.favorites("albums").await?;
    if let Some(artist_id) = query.artist_id {
        let ids: Vec<i64> = client
            .albums(
                false,
                &AlbumListQuery {
                    artist_id: Some(artist_id),
                    ..AlbumListQuery::default()
                },
                0,
                PAGE,
            )
            .await?
            .items
            .iter()
            .map(|a| a.id)
            .collect();
        items.retain(|a| ids.contains(&a.id));
    }
    let needle = query.search.trim().to_lowercase();
    items.retain(|a| {
        (needle.is_empty()
            || a.title.to_lowercase().contains(&needle)
            || a.artist.to_lowercase().contains(&needle))
            && in_year_range(a.year, query.year_from, query.year_to)
    });
    sort_favorites_albums(&mut items, query, seed);
    Ok(items)
}

/// `GET /favorites/tracks`, filtered and sorted client-side.
async fn favorite_tracks(
    client: &Client,
    query: &ListQuery,
    seed: Option<i64>,
) -> Result<Vec<Track>, ApiError> {
    let mut items: Vec<Track> = client.favorites("tracks").await?;
    let needle = query.search.trim().to_lowercase();
    items.retain(|t| {
        (needle.is_empty()
            || t.title.to_lowercase().contains(&needle)
            || t.album.to_lowercase().contains(&needle)
            || t.artist.to_lowercase().contains(&needle))
            && in_year_range(t.year, query.year_from, query.year_to)
            && query.artist_id.is_none_or(|id| {
                t.artists.iter().any(|a| a.id == id) || t.album_artists.iter().any(|a| a.id == id)
            })
    });
    sort_favorites_tracks(&mut items, query, seed);
    Ok(items)
}

/// Client-side sort of the favorite albums (`favoriteAlbums` in the Electron
/// client): the endpoints only page, so filter + order happen here.
fn sort_favorites_albums(items: &mut [Album], query: &ListQuery, seed: Option<i64>) {
    if query.sort == ListSort::Random {
        shuffle_seeded(items, seed.unwrap_or(0));
        return;
    }
    let desc = query.order == SortOrder::Desc;
    items.sort_by(|a, b| {
        let ordering = match query.sort {
            ListSort::Year => a.year.cmp(&b.year),
            ListSort::ReleaseDate => a.release_date.cmp(&b.release_date),
            ListSort::RecentlyPlayed => a.last_played_at.cmp(&b.last_played_at),
            ListSort::Added => a.added_at.cmp(&b.added_at),
            _ => a.title.to_lowercase().cmp(&b.title.to_lowercase()),
        };
        if desc { ordering.reverse() } else { ordering }
    });
}

/// Client-side sort of the favorite tracks (`favoriteSongs`).
fn sort_favorites_tracks(items: &mut [Track], query: &ListQuery, seed: Option<i64>) {
    if query.sort == ListSort::Random {
        shuffle_seeded(items, seed.unwrap_or(0));
        return;
    }
    let desc = query.order == SortOrder::Desc;
    items.sort_by(|a, b| {
        let ordering = match query.sort {
            ListSort::Album => a
                .album
                .to_lowercase()
                .cmp(&b.album.to_lowercase())
                .then(a.disc_number.cmp(&b.disc_number))
                .then(a.track_number.cmp(&b.track_number)),
            ListSort::Plays => a.play_count.cmp(&b.play_count),
            ListSort::RecentlyPlayed => a.last_played_at.cmp(&b.last_played_at),
            // Tracks have addedAt, not a per-track release date: newest
            // first is what the list shows.
            ListSort::ReleaseDate | ListSort::Added => a.added_at.cmp(&b.added_at),
            ListSort::Year => a.year.cmp(&b.year),
            ListSort::Duration => a.duration_millis.cmp(&b.duration_millis),
            _ => a.title.to_lowercase().cmp(&b.title.to_lowercase()),
        };
        if desc { ordering.reverse() } else { ordering }
    });
}

fn in_year_range(year: i32, from: Option<i32>, to: Option<i32>) -> bool {
    let from_ok = from.is_none_or(|from| from <= 0 || year >= from);
    let to_ok = to.is_none_or(|to| to <= 0 || year <= to);
    from_ok && to_ok
}

/// Fisher-Yates with xorshift64*, seeded from the clock.
fn shuffle<T>(items: &mut [T]) {
    shuffle_seeded(items, new_seed());
}

/// The same shuffle for a given seed: every page of one list must agree.
fn shuffle_seeded<T>(items: &mut [T], seed: i64) {
    let mut state = u64::try_from(seed).unwrap_or(1) | 1;
    for i in (1..items.len()).rev() {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        let r = state.wrapping_mul(0x2545_f491_4f6c_dd1d);
        let j = usize::try_from(r % (i as u64 + 1)).unwrap_or(0);
        items.swap(i, j);
    }
}

// ---------------------------------------------------------------- detail

/// One album with its tracks.
#[derive(Clone, Debug)]
pub struct AlbumDetail {
    pub album: Album,
    pub tracks: Vec<Track>,
    /// Highest audio quality across the tracks.
    pub quality: Option<String>,
}

/// One artist with the discography shown on its page.
#[derive(Clone, Debug)]
pub struct ArtistDetail {
    pub artist: Artist,
    pub biography: String,
    pub country: String,
    pub artist_type: String,
    pub aliases: Vec<String>,
    pub tracks_total: i64,
    pub top_popular: bool,
    pub top_tracks: Vec<Track>,
    /// Own and collaboration albums, then appearances.
    pub releases: Vec<Album>,
}

/// One playlist with its ordered tracks.
#[derive(Clone, Debug)]
pub struct PlaylistDetail {
    pub playlist: Playlist,
    pub tracks: Vec<Track>,
}

/// Fetches the album and every track of it (`/albums/{id}` embeds at most
/// 500; the rest is paged from `/tracks?album=`).
pub async fn fetch_album_detail(
    client: &Client,
    capabilities: &Capabilities,
    id: i64,
) -> Result<AlbumDetail, ApiError> {
    let (album, mut tracks) = client.album(id).await?;
    if album.track_count > i64::try_from(tracks.len()).unwrap_or(0) {
        // /albums/{id} embeds at most 500 tracks; page the rest in the same
        // disc / track order.
        let query = TrackListQuery {
            album_id: Some(id),
            ..TrackListQuery::default()
        };
        tracks = fetch_all_tracks(client, capabilities.has(Feature::Browse), &query).await?;
    }
    let quality = album_quality(&tracks);
    Ok(AlbumDetail {
        album,
        tracks,
        quality,
    })
}

/// Every page of a track list.
pub async fn fetch_all_tracks(
    client: &Client,
    browse: bool,
    query: &TrackListQuery,
) -> Result<Vec<Track>, ApiError> {
    let mut out = Vec::new();
    loop {
        let offset = u32::try_from(out.len()).unwrap_or(u32::MAX);
        let page = client.tracks(browse, query, offset, PAGE).await?;
        let n = page.items.len();
        out.extend(page.items);
        if n == 0 || i64::try_from(out.len()).unwrap_or(i64::MAX) >= page.total {
            return Ok(out);
        }
    }
}

/// Highest quality across the tracks; "最高" suffix like `getAlbumAudioQuality`.
fn album_quality(tracks: &[Track]) -> Option<String> {
    let mut best: Option<(i64, String)> = None;
    for track in tracks {
        let Some(label) = format::audio_quality(track) else {
            continue;
        };
        let rank = i64::from(track.bit_depth.unwrap_or(0)) * 1_000_000
            + i64::from(track.sample_rate.unwrap_or(0));
        if best.as_ref().is_none_or(|(r, _)| rank > *r) {
            best = Some((rank, label));
        }
    }
    best.map(|(_, label)| label)
}

/// `GET /artists/{id}`: the artist, its releases and top tracks.
pub async fn fetch_artist_detail(client: &Client, id: i64) -> Result<ArtistDetail, ApiError> {
    let detail = client.artist(id).await?;
    let mut releases: Vec<Album> = detail.releases.iter().map(|r| r.album.clone()).collect();
    if releases.is_empty() {
        releases = detail.albums;
    }
    Ok(ArtistDetail {
        artist: detail.artist,
        biography: detail.biography.unwrap_or_default(),
        country: detail.country,
        artist_type: detail.artist_type,
        aliases: detail.aliases,
        tracks_total: detail.tracks_total,
        top_popular: detail.top_tracks_are_popular,
        top_tracks: detail.top_tracks,
        releases,
    })
}

/// `GET /playlists/{id}`.
pub async fn fetch_playlist_detail(client: &Client, id: i64) -> Result<PlaylistDetail, ApiError> {
    let detail = client.playlist(id).await?;
    Ok(PlaylistDetail {
        playlist: detail.playlist,
        tracks: detail.tracks,
    })
}

/// `GET /tracks/{id}/similar` without its own track.
pub async fn fetch_similar(
    client: &Client,
    track_id: i64,
    count: u32,
) -> Result<Vec<Track>, ApiError> {
    let items: Vec<SimilarTrack> = client.similar_tracks(track_id, count).await?;
    Ok(items
        .into_iter()
        .map(|item| item.track)
        .filter(|track| track.id != track_id)
        .collect())
}

/// Results of one search, grouped like the Electron application.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchResults {
    pub albums: Vec<AlbumItem>,
    pub tracks: Vec<TrackItem>,
    pub artists: Vec<ArtistItem>,
    /// The tracks behind `tracks` (playing a search result uses them).
    pub track_list: Vec<Track>,
}

/// One query, three serverside searches (`/artists`, `/albums`, `/tracks`).
pub async fn search(
    client: &Client,
    capabilities: &Capabilities,
    needle: &str,
) -> Result<SearchResults, ApiError> {
    let browse = capabilities.has(Feature::Browse);
    let query = ListQuery {
        search: needle.to_owned(),
        ..ListQuery::default()
    };
    let mut albums = album_query(&query, browse);
    albums.search = non_empty(needle);
    let mut tracks = track_query(&query, browse);
    tracks.search = non_empty(needle);
    let mut artists = artist_query(&query, browse);
    artists.search = non_empty(needle);
    artists.favorite = false;
    let (albums, tracks, artists) = tokio::try_join!(
        client.albums(browse, &albums, 0, SEARCH_LIMIT),
        client.tracks(browse, &tracks, 0, SEARCH_LIMIT),
        client.artists(browse, &artists, 0, SEARCH_LIMIT),
    )?;
    Ok(SearchResults {
        albums: albums.items.iter().map(album_item).collect(),
        tracks: tracks.items.iter().map(track_item).collect(),
        artists: artists.items.iter().map(artist_item).collect(),
        // Only the tracks are playable; a search is a one-shot list.
        track_list: tracks.items,
    })
}

/// A random sample of the whole library (`getRandomSongList`).
pub async fn random_tracks(
    client: &Client,
    capabilities: &Capabilities,
    limit: u32,
) -> Result<Vec<Track>, ApiError> {
    let browse = capabilities.has(Feature::Browse);
    let limit = limit.clamp(1, PAGE);
    if browse {
        let query = TrackListQuery {
            sort: Some(TrackSort::Random),
            seed: Some(new_seed()),
            ..TrackListQuery::default()
        };
        let page = client.tracks(true, &query, 0, limit).await?;
        return Ok(page.items);
    }
    // Legacy servers: sample a random window and shuffle it.
    let mut count = api::Query::new();
    count.page(0, 1);
    let probe: api::Page<Track> = client.get("/tracks", &count).await?;
    let total = u32::try_from(probe.total.max(0)).unwrap_or(0);
    if total == 0 {
        return Ok(Vec::new());
    }
    let window = total.min(PAGE);
    let offset = u32::try_from(new_seed()).unwrap_or(0) % (total - window + 1);
    let page = client
        .tracks(false, &TrackListQuery::default(), offset, window.max(1))
        .await?;
    let mut items = page.items;
    shuffle(&mut items);
    items.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    Ok(items)
}

/// Every playlist, for the "add to playlist" menu.
pub async fn fetch_playlists(client: &Client) -> Result<Vec<PlaylistItem>, ApiError> {
    Ok(client
        .playlists()
        .await?
        .iter()
        .map(playlist_item)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_options_follow_browse() {
        let albums = sort_options(ListKind::Albums, true);
        assert_eq!(
            albums,
            vec![
                ListSort::Added,
                ListSort::Title,
                ListSort::RecentlyPlayed,
                ListSort::ReleaseDate,
                ListSort::Year,
                ListSort::Random
            ]
        );
        // Without browse: title and random are gone.
        let albums = sort_options(ListKind::Albums, false);
        assert!(!albums.contains(&ListSort::Title));
        assert!(!albums.contains(&ListSort::Random));
        assert_eq!(
            sort_options(ListKind::Playlists, true),
            vec![ListSort::Name, ListSort::Duration, ListSort::TrackCount]
        );
    }

    #[test]
    fn random_seeds_are_kept_per_query_and_rotate_from_the_top() {
        let mut seeds = Seeds::default();
        let mut random = ListQuery {
            kind: ListKind::Tracks,
            ..ListQuery::default()
        };
        assert_eq!(seeds.for_query(&random, 0), None, "not random when sorted");
        random.sort = ListSort::Random;
        let first = seeds.for_query(&random, 0).unwrap();
        assert_eq!(seeds.for_query(&random, 500), Some(first));
        // A different filter is a different list.
        let mut other = random.clone();
        other.search = "x".into();
        let other_seed = seeds.for_query(&other, 0).unwrap();
        assert_ne!(first, other_seed);
        // Loading the same list from the top shuffles again. The seed is
        // drawn from a counter as well as the clock, so this holds even when
        // two calls land in the same coarse clock tick.
        let new = seeds.for_query(&random, 0).unwrap();
        assert_ne!(first, new);
    }

    #[test]
    fn year_range_matches_both_ends() {
        assert!(in_year_range(2000, None, None));
        assert!(in_year_range(2000, Some(2000), Some(2000)));
        assert!(!in_year_range(1999, Some(2000), None));
        assert!(!in_year_range(2001, None, Some(2000)));
        assert!(in_year_range(0, Some(0), Some(0)));
    }
}

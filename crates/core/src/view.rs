//! What the main pane shows: one view at a time (a library list, a detail
//! page, the search results or the similar tracks of a track). Every view
//! resolves to the same shape - a header, rows and (for playable views) the
//! tracks behind the rows - so the UI has a single list renderer.

use api::{Capabilities, Client, Feature};

use crate::PlaylistEdit;
use serde::{Deserialize, Serialize};

use crate::library::{
    self, AlbumDetail, AlbumItem, ArtistDetail, FavoriteKind, ListItem, ListKind, ListQuery,
    ListSort, PlaylistDetail, PlaylistItem, TrackItem,
};

/// Where the UI is.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum View {
    /// Sidebar lists, paged.
    #[default]
    Albums,
    Tracks,
    Artists,
    Playlists,
    FavoriteAlbums,
    FavoriteTracks,
    /// `GET /albums/{id}`.
    Album(i64),
    /// `GET /artists/{id}`.
    Artist(i64),
    /// `GET /playlists/{id}`.
    Playlist(i64),
    /// Search results for the text (three lists at once).
    Search(String),
    /// `GET /tracks/{id}/similar`.
    Similar(i64),
}

impl View {
    /// The list behind a sidebar view (`None` for details and search).
    pub fn list_kind(&self) -> Option<ListKind> {
        Some(match self {
            Self::Albums => ListKind::Albums,
            Self::Tracks => ListKind::Tracks,
            Self::Artists => ListKind::Artists,
            Self::Playlists => ListKind::Playlists,
            Self::FavoriteAlbums => ListKind::FavoriteAlbums,
            Self::FavoriteTracks => ListKind::FavoriteTracks,
            _ => return None,
        })
    }

    pub fn is_list(&self) -> bool {
        self.list_kind().is_some()
    }

    /// A view that is fetched page by page (the details are one block).
    pub fn is_paged(&self) -> bool {
        self.is_list()
    }

    /// A playlist's detail page.
    pub fn is_playlist(&self) -> bool {
        matches!(self, Self::Playlist(_))
    }

    fn default_query(&self) -> ListQuery {
        match self.list_kind() {
            Some(kind) => ListQuery {
                kind,
                search: match self {
                    Self::Search(text) => text.clone(),
                    _ => String::new(),
                },
                ..kind.default_query()
            },
            None => ListQuery::default(),
        }
    }
}

/// Rows are album cards or text rows depending on the view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Layout {
    /// Square artwork cards (albums).
    Grid,
    /// Text rows (tracks, artists, playlists).
    #[default]
    List,
}

/// What a row stands for (decides what clicking it opens).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RowKind {
    #[default]
    Album,
    Track,
    Artist,
    Playlist,
}

impl RowKind {
    /// The favorite the row toggles; playlists have none.
    pub fn favorite_kind(self) -> Option<FavoriteKind> {
        Some(match self {
            Self::Album => FavoriteKind::Album,
            Self::Track => FavoriteKind::Track,
            Self::Artist => FavoriteKind::Artist,
            Self::Playlist => return None,
        })
    }
}

/// One row of the main pane.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewRow {
    pub kind: RowKind,
    pub id: i64,
    pub title: String,
    /// Artist (albums), artist (tracks) or album (tracks).
    pub subtitle: String,
    /// Year, counts, duration: whatever the view shows beside the title.
    pub meta: String,
    /// Artwork path (server relative); empty when there is none.
    pub artwork: String,
    pub favorite: bool,
    /// Index into the view's tracks when the row is a track ("play from
    /// here"); `None` for album / artist / playlist rows.
    pub play: Option<usize>,
}

impl ViewRow {
    /// What a click opens (albums, artists, playlists) or plays (a track,
    /// from its own index in the view's track list).
    pub fn open_command(&self, row: usize) -> crate::Command {
        match self.kind {
            RowKind::Album => crate::Command::Navigate(View::Album(self.id)),
            RowKind::Artist => crate::Command::Navigate(View::Artist(self.id)),
            RowKind::Playlist => crate::Command::Navigate(View::Playlist(self.id)),
            RowKind::Track => crate::Command::PlayRow(self.play.unwrap_or(row)),
        }
    }

    /// What the context menu's "播放" does: an album plays whole, a track
    /// plays from its row, and artists / playlists open like a click.
    pub fn play_command(&self, row: usize) -> crate::Command {
        match self.kind {
            RowKind::Album => crate::Command::PlayAlbum { album_id: self.id },
            _ => self.open_command(row),
        }
    }

    /// What "next / add to the end of the queue" enqueues.
    pub fn enqueue_source(&self) -> crate::TrackSource {
        match self.kind {
            RowKind::Album => crate::TrackSource::Album(self.id),
            _ => crate::TrackSource::Track(self.id),
        }
    }
}

/// Sort and filter controls of a list view.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ViewToolbar {
    /// Sort keys on this server, in menu order.
    pub sort_options: Vec<ListSort>,
    /// The sort this page was loaded with (the UI shows it back).
    pub sort: ListSort,
    pub desc: bool,
    pub search: String,
    pub year_from: Option<i32>,
    pub year_to: Option<i32>,
    pub favorite: bool,
    /// The list can be filtered by year and favorites.
    pub filterable: bool,
}

impl ViewToolbar {
    /// The toolbar of the query in `query` (an empty one for detail pages).
    fn of(query: &ListQuery, options: Vec<ListSort>, filterable: bool) -> Self {
        Self {
            sort_options: options,
            sort: query.sort,
            desc: query.order == api::SortOrder::Desc,
            search: query.search.clone(),
            year_from: query.year_from,
            year_to: query.year_to,
            favorite: query.favorite,
            filterable,
        }
    }
}

/// The header of a view: what the page is and how much of it is loaded.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewHeader {
    pub view: View,
    pub layout: Layout,
    pub title: String,
    /// Second line: artist, counts, genres, description.
    pub subtitle: String,
    /// Extra lines of a detail page ("1999 · FLAC / 24bit / 48kHz", label).
    pub details: Vec<String>,
    /// Artwork path of the detail page (server relative).
    pub artwork: String,
    /// Cover shown large on a detail page.
    pub total: usize,
    pub loaded: usize,
    pub toolbar: ViewToolbar,
    /// The view can be played as a whole (details, search).
    pub can_play: bool,
    /// The playlist editor is open for this playlist.
    pub editable: bool,
}

/// The current view: its query, the rows loaded so far and the tracks.
pub struct ViewState {
    pub view: View,
    pub query: ListQuery,
    pub header: ViewHeader,
    pub rows: Vec<ViewRow>,
    /// The tracks behind `rows` (index-aligned for playable views).
    pub tracks: Vec<api::Track>,
    pub total: usize,
    /// Offset of the next page; `None` while a request runs or at the end.
    pub next: Option<u32>,
    pub loading: bool,
    /// Bumped per navigation; stale answers are dropped.
    pub revision: u64,
}

impl Default for ViewState {
    fn default() -> Self {
        Self::new(View::Albums)
    }
}

impl ViewState {
    pub fn new(view: View) -> Self {
        Self {
            query: view.default_query(),
            view,
            header: ViewHeader::default(),
            rows: Vec::new(),
            tracks: Vec::new(),
            total: 0,
            next: None,
            loading: false,
            revision: 0,
        }
    }

    /// Starts over for `view` with the default query.
    pub fn reset(&mut self, view: View) {
        let query = view.default_query();
        self.reset_with(view, query, None);
    }

    /// Starts over for `view`: clears the rows, asks for page 0 and leaves a
    /// placeholder header that already describes the view it is about to
    /// load - the same title, layout and sort options, with the query's sort,
    /// direction and filters filled in.
    ///
    /// `skeleton` is the header the UI is showing now: when it belongs to the
    /// same view, its static parts are reused, so changing a filter does not
    /// make the pane collapse to an empty list for one round trip.
    pub fn reset_with(&mut self, view: View, query: ListQuery, skeleton: Option<&ViewHeader>) {
        let toolbar_options = match skeleton.filter(|h| h.view == view) {
            Some(header) => header.toolbar.sort_options.clone(),
            None => ViewToolbar::default().sort_options,
        };
        let mut header = match (skeleton.filter(|h| h.view == view), view.list_kind()) {
            // Same view, reloading: keep everything the user is looking at.
            (Some(header), _) => header.clone(),
            // A different list: the sidebar lists have a known shape.
            (None, Some(kind)) => list_header(&view, kind, ViewToolbar::default()),
            // A detail page or the search view: rows are pending.
            (None, None) => ViewHeader {
                view: view.clone(),
                layout: Layout::List,
                title: match &view {
                    View::Search(_) => "搜索".to_owned(),
                    _ => String::new(),
                },
                ..ViewHeader::default()
            },
        };
        header.view = view.clone();
        if !toolbar_options.is_empty() {
            header.toolbar.sort_options = toolbar_options;
        }
        header.toolbar.sort = query.sort;
        header.toolbar.desc = query.order == api::SortOrder::Desc;
        header.toolbar.search = query.search.clone();
        header.toolbar.year_from = query.year_from;
        header.toolbar.year_to = query.year_to;
        header.toolbar.favorite = query.favorite;
        header.loaded = 0;
        header.total = 0;
        self.header = header;
        self.view = view;
        self.query = query;
        self.rows.clear();
        self.tracks.clear();
        self.total = 0;
        self.next = Some(0);
        self.loading = false;
        self.revision = self.revision.wrapping_add(1);
    }
}

/// A page of a view, ready to be appended.
pub struct ViewPage {
    /// Set on the first page only (title, layout, toolbar).
    pub header: Option<ViewHeader>,
    pub rows: Vec<ViewRow>,
    pub tracks: Vec<api::Track>,
    pub total: usize,
    /// The revision of a loaded playlist (item writes need it).
    pub playlist: Option<PlaylistEdit>,
    /// A user-facing result shown once the page is applied.
    pub notice: Option<String>,
}

/// How a grid model built from `rows` rows in chunks of `columns` changes
/// when `added` rows are appended: the chunk that grew (if the old length was
/// mid-chunk) and the new chunks to insert.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChunkDelta {
    /// The last existing chunk grew (its index).
    pub changed: Option<usize>,
    /// Chunks to append, and how many.
    pub added_from: usize,
    pub added: usize,
}

pub fn chunk_delta(before: usize, added: usize, columns: usize) -> ChunkDelta {
    let columns = columns.max(1);
    if added == 0 {
        return ChunkDelta::default();
    }
    let old_chunks = before.div_ceil(columns);
    let new_chunks = (before + added).div_ceil(columns);
    ChunkDelta {
        changed: (!before.is_multiple_of(columns) && old_chunks > 0).then(|| old_chunks - 1),
        added_from: old_chunks,
        added: new_chunks.saturating_sub(old_chunks),
    }
}

/// The grid chunks holding these row indexes, deduplicated and ordered.
pub fn chunk_indexes(indexes: &[usize], columns: usize) -> Vec<usize> {
    let columns = columns.max(1);
    let mut chunks: Vec<usize> = indexes.iter().map(|i| i / columns).collect();
    chunks.sort_unstable();
    chunks.dedup();
    chunks
}

fn album_rows(items: &[AlbumItem]) -> Vec<ViewRow> {
    items
        .iter()
        .map(|album| ViewRow {
            kind: RowKind::Album,
            id: album.id,
            title: album.title.clone(),
            subtitle: album.artist.clone(),
            meta: if album.year > 0 {
                album.year.to_string()
            } else {
                String::new()
            },
            artwork: album.artwork.clone(),
            favorite: album.favorite,
            play: None,
        })
        .collect()
}

/// Track rows, numbered from `offset` in the view's track list.
fn track_rows(items: &[TrackItem], offset: usize) -> Vec<ViewRow> {
    items
        .iter()
        .enumerate()
        .map(|(i, track)| ViewRow {
            kind: RowKind::Track,
            play: Some(offset + i),
            id: track.id,
            title: track.title.clone(),
            subtitle: track.artist.clone(),
            meta: if track.album.is_empty() {
                track.duration.clone()
            } else {
                format!("{} · {}", track.album, track.duration)
            },
            artwork: track.artwork.clone(),
            favorite: track.favorite,
        })
        .collect()
}

fn artist_rows(items: &[library::ArtistItem]) -> Vec<ViewRow> {
    items
        .iter()
        .map(|artist| ViewRow {
            kind: RowKind::Artist,
            id: artist.id,
            title: artist.name.clone(),
            subtitle: String::new(),
            meta: format!("{} 张专辑 · {} 首", artist.album_count, artist.track_count),
            artwork: artist.artwork.clone(),
            favorite: artist.favorite,
            play: None,
        })
        .collect()
}

fn playlist_rows(items: &[PlaylistItem]) -> Vec<ViewRow> {
    items
        .iter()
        .map(|playlist| ViewRow {
            kind: RowKind::Playlist,
            id: playlist.id,
            title: playlist.name.clone(),
            subtitle: playlist.description.clone(),
            meta: if playlist.duration.is_empty() {
                format!("{} 首", playlist.item_count)
            } else {
                format!("{} 首 · {}", playlist.item_count, playlist.duration)
            },
            artwork: playlist.artwork.clone(),
            favorite: false,
            play: None,
        })
        .collect()
}

/// Title and layout of a list view.
fn list_header(view: &View, kind: ListKind, toolbar: ViewToolbar) -> ViewHeader {
    let (title, layout) = match kind {
        ListKind::Albums => ("专辑", Layout::Grid),
        ListKind::Tracks => ("曲目", Layout::List),
        ListKind::Artists => ("艺人", Layout::List),
        ListKind::Playlists => ("歌单", Layout::List),
        ListKind::FavoriteAlbums => ("收藏的专辑", Layout::Grid),
        ListKind::FavoriteTracks => ("收藏的曲目", Layout::List),
        ListKind::ArtistTracks => ("曲目", Layout::List),
    };
    ViewHeader {
        view: view.clone(),
        layout,
        title: title.to_owned(),
        toolbar,
        ..ViewHeader::default()
    }
}

/// The list of sort keys a view offers.
pub fn sort_options(view: &View, capabilities: &Capabilities) -> Vec<ListSort> {
    let browse = capabilities.has(Feature::Browse);
    match view.list_kind() {
        Some(kind) => library::sort_options(kind, browse),
        None => Vec::new(),
    }
}

/// Loads page `offset` of `view` for the query in `state`.
pub async fn load_page(
    client: &Client,
    capabilities: &Capabilities,
    view: &View,
    query: &ListQuery,
    offset: u32,
    seed: Option<i64>,
) -> Result<ViewPage, api::ApiError> {
    match view {
        View::Album(id) => album_page(client, capabilities, *id).await,
        View::Artist(id) => artist_page(client, *id).await,
        View::Playlist(id) => playlist_page(client, *id).await,
        View::Search(text) => search_page(client, capabilities, view, text).await,
        View::Similar(id) => similar_page(client, capabilities, view, *id).await,
        list => list_page(client, capabilities, list, query, offset, seed).await,
    }
}

async fn list_page(
    client: &Client,
    capabilities: &Capabilities,
    view: &View,
    query: &ListQuery,
    offset: u32,
    seed: Option<i64>,
) -> Result<ViewPage, api::ApiError> {
    let kind = query.kind;
    let page = library::fetch_page(client, capabilities, query, offset, seed).await?;
    let rows = match &page.items {
        _ if page.items.iter().all(|i| matches!(i, ListItem::Album(_))) => {
            let albums: Vec<AlbumItem> = page
                .items
                .iter()
                .filter_map(|i| match i {
                    ListItem::Album(a) => Some(a.clone()),
                    _ => None,
                })
                .collect();
            album_rows(&albums)
        }
        _ if page.items.iter().all(|i| matches!(i, ListItem::Track(_))) => {
            let tracks: Vec<TrackItem> = page
                .items
                .iter()
                .filter_map(|i| match i {
                    ListItem::Track(t) => Some(t.clone()),
                    _ => None,
                })
                .collect();
            track_rows(&tracks, offset as usize)
        }
        _ if page.items.iter().all(|i| matches!(i, ListItem::Artist(_))) => {
            let artists: Vec<library::ArtistItem> = page
                .items
                .iter()
                .filter_map(|i| match i {
                    ListItem::Artist(a) => Some(a.clone()),
                    _ => None,
                })
                .collect();
            artist_rows(&artists)
        }
        _ => {
            let playlists: Vec<PlaylistItem> = page
                .items
                .iter()
                .filter_map(|i| match i {
                    ListItem::Playlist(p) => Some(p.clone()),
                    _ => None,
                })
                .collect();
            playlist_rows(&playlists)
        }
    };
    // Only albums / tracks / artists have year and favorite filters, and
    // playlists have no favorites at all.
    let filterable = matches!(
        kind,
        ListKind::Albums
            | ListKind::Tracks
            | ListKind::Artists
            | ListKind::FavoriteAlbums
            | ListKind::FavoriteTracks
    );
    let toolbar = ViewToolbar::of(query, sort_options(view, capabilities), filterable);
    let header = (offset == 0).then(|| list_header(view, kind, toolbar));
    Ok(ViewPage {
        header,
        rows,
        tracks: page.tracks,
        total: page.total,
        playlist: None,
        notice: None,
    })
}

async fn album_page(
    client: &Client,
    capabilities: &Capabilities,
    id: i64,
) -> Result<ViewPage, api::ApiError> {
    let detail = library::fetch_album_detail(client, capabilities, id).await?;
    Ok(album_detail_page(detail))
}

pub fn album_detail_page(detail: AlbumDetail) -> ViewPage {
    let mut details = Vec::new();
    if detail.album.year > 0 {
        details.push(detail.album.year.to_string());
    }
    if !detail.album.genres.is_empty() {
        details.push(detail.album.genres.clone());
    }
    if let Some(quality) = &detail.quality {
        details.push(quality.clone());
    }
    if !detail.album.label.is_empty() {
        details.push(detail.album.label.clone());
    }
    let header = ViewHeader {
        view: View::Album(detail.album.id),
        layout: Layout::List,
        title: detail.album.title.clone(),
        subtitle: detail.album.artist.clone(),
        details,
        artwork: detail.album.artwork_url.clone(),
        total: detail.tracks.len(),
        loaded: detail.tracks.len(),
        can_play: true,
        ..ViewHeader::default()
    };
    let total = detail.tracks.len();
    let items: Vec<TrackItem> = detail.tracks.iter().map(library::track_item).collect();
    let rows = track_rows(&items, 0);
    ViewPage {
        header: Some(header),
        total,
        rows,
        tracks: detail.tracks,
        playlist: None,
        notice: None,
    }
}

async fn artist_page(client: &Client, id: i64) -> Result<ViewPage, api::ApiError> {
    let detail = library::fetch_artist_detail(client, id).await?;
    Ok(artist_detail_page(detail))
}

pub fn artist_detail_page(detail: ArtistDetail) -> ViewPage {
    let mut details = Vec::new();
    if !detail.artist_type.is_empty() {
        details.push(detail.artist_type.clone());
    }
    if !detail.country.is_empty() {
        details.push(detail.country.clone());
    }
    if !detail.biography.is_empty() {
        details.push(detail.biography.clone());
    }
    // Releases first (the artist's own albums), then the top tracks.
    let mut rows = album_rows(
        &detail
            .releases
            .iter()
            .map(library::album_item)
            .collect::<Vec<_>>(),
    );
    let mut tracks = Vec::new();
    if !detail.top_tracks.is_empty() {
        let top: Vec<TrackItem> = detail.top_tracks.iter().map(library::track_item).collect();
        rows.extend(track_rows(&top, 0));
        tracks = detail.top_tracks;
    }
    let header = ViewHeader {
        view: View::Artist(detail.artist.id),
        layout: Layout::List,
        title: detail.artist.name.clone(),
        subtitle: format!(
            "{} 张专辑 · {} 首",
            detail.artist.album_count, detail.tracks_total
        ),
        details,
        artwork: detail.artist.image_url.clone(),
        total: rows.len(),
        loaded: rows.len(),
        can_play: !tracks.is_empty(),
        ..ViewHeader::default()
    };
    ViewPage {
        header: Some(header),
        total: rows.len(),
        rows,
        tracks,
        playlist: None,
        notice: None,
    }
}

async fn playlist_page(client: &Client, id: i64) -> Result<ViewPage, api::ApiError> {
    let detail = library::fetch_playlist_detail(client, id).await?;
    Ok(playlist_detail_page(detail))
}

pub fn playlist_detail_page(detail: PlaylistDetail) -> ViewPage {
    let mut details = Vec::new();
    if !detail.playlist.description.is_empty() {
        details.push(detail.playlist.description.clone());
    }
    details.push(format!(
        "{} 首{}",
        detail.playlist.item_count,
        if detail.playlist.duration_millis > 0 {
            format!(
                " · {}",
                crate::format::duration(detail.playlist.duration_millis)
            )
        } else {
            String::new()
        }
    ));
    let header = ViewHeader {
        view: View::Playlist(detail.playlist.id),
        layout: Layout::List,
        title: detail.playlist.name.clone(),
        subtitle: String::new(),
        details,
        artwork: detail.playlist.artwork_url.clone().unwrap_or_default(),
        total: detail.tracks.len(),
        loaded: detail.tracks.len(),
        can_play: !detail.tracks.is_empty(),
        editable: true,
        ..ViewHeader::default()
    };
    let items: Vec<TrackItem> = detail.tracks.iter().map(library::track_item).collect();
    let total = detail.tracks.len();
    ViewPage {
        header: Some(header),
        total,
        rows: track_rows(&items, 0),
        tracks: detail.tracks,
        playlist: Some(PlaylistEdit {
            id: detail.playlist.id,
            name: detail.playlist.name.clone(),
            revision: detail.playlist.revision,
            track_ids: items.iter().map(|t| t.id).collect(),
            description: detail.playlist.description.clone(),
            busy: false,
        }),
        notice: None,
    }
}

async fn search_page(
    client: &Client,
    capabilities: &Capabilities,
    view: &View,
    text: &str,
) -> Result<ViewPage, api::ApiError> {
    let results = library::search(client, capabilities, text).await?;
    let mut rows = artist_rows(&results.artists);
    rows.extend(album_rows(&results.albums));
    // The track rows are playable: their rows index the view's tracks.
    let tracks = results.track_list;
    rows.extend(track_rows(&results.tracks, 0));
    let header = ViewHeader {
        view: view.clone(),
        layout: Layout::List,
        title: "搜索".to_owned(),
        subtitle: format!("“{text}”"),
        total: rows.len(),
        loaded: rows.len(),
        ..ViewHeader::default()
    };
    Ok(ViewPage {
        header: Some(header),
        total: rows.len(),
        rows,
        tracks,
        playlist: None,
        notice: None,
    })
}

async fn similar_page(
    client: &Client,
    capabilities: &Capabilities,
    view: &View,
    track_id: i64,
) -> Result<ViewPage, api::ApiError> {
    let tracks = library::fetch_similar(client, track_id, 50).await?;
    let items: Vec<TrackItem> = tracks.iter().map(library::track_item).collect();
    let total = items.len();
    let _ = capabilities;
    let header = ViewHeader {
        view: view.clone(),
        layout: Layout::List,
        title: "相似歌曲".to_owned(),
        subtitle: String::new(),
        total: items.len(),
        loaded: items.len(),
        can_play: !items.is_empty(),
        ..ViewHeader::default()
    };
    Ok(ViewPage {
        header: Some(header),
        total,
        rows: track_rows(&items, 0),
        tracks,
        playlist: None,
        notice: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SortOrder;

    #[test]
    fn views_map_to_lists_and_defaults() {
        assert_eq!(View::Albums.list_kind(), Some(ListKind::Albums));
        assert_eq!(
            View::FavoriteTracks.list_kind(),
            Some(ListKind::FavoriteTracks)
        );
        assert!(View::Album(3).list_kind().is_none());
        assert!(!View::Album(3).is_list());
        assert_eq!(View::Albums.default_query().sort, ListSort::Added);
        assert_eq!(View::Albums.default_query().order, SortOrder::Desc);
        assert_eq!(View::Artists.default_query().sort, ListSort::Name);
        // The search view has no list: its text lives in the view itself.
        assert!(View::Search("x".into()).default_query().search.is_empty());
    }

    #[test]
    fn default_orders_match_the_server() {
        assert!(View::Albums.is_paged());
        assert!(!View::Album(1).is_paged());
    }

    #[test]
    fn mixed_pages_resolve_rows_to_the_right_command() {
        // Search results: an artist, an album and a track under the same ids.
        let artist = ViewRow {
            kind: RowKind::Artist,
            id: 7,
            ..ViewRow::default()
        };
        let album = ViewRow {
            kind: RowKind::Album,
            id: 7,
            ..ViewRow::default()
        };
        let track = ViewRow {
            kind: RowKind::Track,
            id: 7,
            play: Some(1),
            ..ViewRow::default()
        };
        // A row click opens albums / artists and plays tracks from the
        // row's own index in the view's track list (not the row number).
        assert_eq!(
            artist.open_command(0),
            crate::Command::Navigate(View::Artist(7))
        );
        assert_eq!(
            album.open_command(0),
            crate::Command::Navigate(View::Album(7))
        );
        assert_eq!(track.open_command(0), crate::Command::PlayRow(1));
        // "播放" on an album plays the whole album.
        assert_eq!(
            album.play_command(0),
            crate::Command::PlayAlbum { album_id: 7 }
        );
        // Queueing an album uses the album id, not a track id.
        assert_eq!(album.enqueue_source(), crate::TrackSource::Album(7));
        assert_eq!(track.enqueue_source(), crate::TrackSource::Track(7));
    }

    #[test]
    fn a_shared_id_only_flips_the_row_of_the_same_kind() {
        // A search page can hold an album 7 and a track 7. Toggling one must
        // not touch the other, and a playlist row never carries a favorite.
        let mut state = ViewState::new(View::Search("x".into()));
        state.rows = vec![
            ViewRow {
                kind: RowKind::Album,
                id: 7,
                ..ViewRow::default()
            },
            ViewRow {
                kind: RowKind::Track,
                id: 7,
                play: Some(0),
                ..ViewRow::default()
            },
        ];
        for row in &mut state.rows {
            if row.id == 7 && row.kind.favorite_kind() == Some(crate::FavoriteKind::Track) {
                row.favorite = true;
            }
        }
        assert!(!state.rows[0].favorite, "the album row must not flip");
        assert!(state.rows[1].favorite);
    }

    #[test]
    fn only_matching_row_kinds_carry_a_favorite() {
        assert_eq!(
            RowKind::Album.favorite_kind(),
            Some(crate::FavoriteKind::Album)
        );
        assert_eq!(
            RowKind::Track.favorite_kind(),
            Some(crate::FavoriteKind::Track)
        );
        assert_eq!(
            RowKind::Artist.favorite_kind(),
            Some(crate::FavoriteKind::Artist)
        );
        assert_eq!(RowKind::Playlist.favorite_kind(), None);
    }

    #[test]
    fn the_toolbar_reports_the_query_it_loaded() {
        let query = ListQuery {
            kind: ListKind::Tracks,
            sort: ListSort::Plays,
            order: SortOrder::Asc,
            favorite: true,
            year_from: Some(2001),
            ..ListQuery::default()
        };
        let toolbar = ViewToolbar::of(&query, vec![ListSort::Added, ListSort::Plays], true);
        // The header carries the sort and direction actually used, so the UI
        // does not fall back to the first menu entry after a reload.
        assert_eq!(toolbar.sort, ListSort::Plays);
        assert!(!toolbar.desc);
        assert!(toolbar.favorite);
        assert_eq!(toolbar.year_from, Some(2001));
        assert!(toolbar.filterable);
    }

    #[test]
    fn a_reload_keeps_the_query_it_was_asked_with() {
        // The user changed the sort and the direction: the placeholder header
        // the UI gets back must carry both, otherwise the arrow snaps back and
        // the next query loses the direction.
        let mut state = ViewState::new(View::Albums);
        let query = ListQuery {
            kind: ListKind::Albums,
            sort: ListSort::Title,
            order: SortOrder::Desc,
            favorite: true,
            year_from: Some(1990),
            ..ListQuery::default()
        };
        state.reset_with(View::Albums, query.clone(), None);
        assert_eq!(state.query, query);
        assert_eq!(state.header.toolbar.sort, ListSort::Title);
        assert!(state.header.toolbar.desc);
        assert!(state.header.toolbar.favorite);
        assert_eq!(state.header.toolbar.year_from, Some(1990));
    }

    #[test]
    fn grid_chunks_are_indexed_right() {
        // Appending inside the last chunk only changes that chunk.
        assert_eq!(
            chunk_delta(2, 1, 3),
            ChunkDelta {
                changed: Some(0),
                added_from: 1,
                added: 0
            }
        );
        // Appending past a chunk boundary inserts new chunks, and the old
        // (full) last chunk is untouched.
        assert_eq!(
            chunk_delta(3, 1, 3),
            ChunkDelta {
                changed: None,
                added_from: 1,
                added: 1
            }
        );
        // Filling from an empty grid.
        assert_eq!(
            chunk_delta(0, 7, 3),
            ChunkDelta {
                changed: None,
                added_from: 0,
                added: 3
            }
        );
        // Nothing added, nothing to tell the view.
        assert_eq!(chunk_delta(5, 0, 3), ChunkDelta::default());

        // Row indexes map to chunks and are deduplicated.
        assert_eq!(chunk_indexes(&[], 3), Vec::<usize>::new());
        assert_eq!(chunk_indexes(&[0, 1, 2, 3, 7], 3), vec![0, 1, 2]);
        // A bad column count must not divide by zero.
        assert_eq!(chunk_indexes(&[5], 0), vec![5]);
    }

    #[test]
    fn a_reload_keeps_the_header_it_was_showing() {
        // Changing a filter must not make the pane fall back to an empty list
        // layout for one round trip: the placeholder header keeps the view's
        // layout, title and sort options.
        let mut state = ViewState::new(View::Albums);
        state.reset_with(
            View::Albums,
            ListQuery {
                kind: ListKind::Albums,
                sort: ListSort::Added,
                ..ListQuery::default()
            },
            None,
        );
        assert_eq!(state.header.layout, Layout::Grid);
        assert_eq!(state.header.title, "专辑");

        let skeleton = state.header.clone();
        let query = ListQuery {
            kind: ListKind::Albums,
            sort: ListSort::Title,
            order: SortOrder::Asc,
            favorite: true,
            ..ListQuery::default()
        };
        state.reset_with(View::Albums, query, Some(&skeleton));
        assert_eq!(state.header.layout, Layout::Grid, "layout must not flip");
        assert_eq!(state.header.title, "专辑");
        assert_eq!(state.header.toolbar.sort, ListSort::Title);
        assert!(!state.header.toolbar.desc);
        assert!(state.header.toolbar.favorite);
        // The rows are gone while the first page loads.
        assert_eq!((state.header.loaded, state.header.total), (0, 0));
    }

    #[test]
    fn reset_clears_and_bumps_the_revision() {
        let mut state = ViewState::new(View::Albums);
        state.rows.push(ViewRow::default());
        state.total = 10;
        let revision = state.revision;
        state.reset(View::Tracks);
        assert!(state.rows.is_empty());
        assert_eq!(state.total, 0);
        assert_eq!(state.next, Some(0));
        assert_eq!(state.revision, revision + 1);
        assert_eq!(state.header.view, View::Tracks);
    }
}

//! List queries for albums, tracks, artists and the favorites / search
//! helpers.
//!
//! Servers with `features.browse` are read through `/browse/*`, which honours
//! `order`, `yearFrom`/`yearTo` and adds `sort=random&seed=`. Legacy
//! `/albums` and `/tracks` have a fixed direction per sort key and ignore
//! sort values they never understood (see `ALBUM_SORT` / `SONG_SORT` in
//! `music032-controller.ts`).

use std::fmt::Display;

use crate::client::MAX_PAGE_SIZE;

/// Ordered query parameters. Empty values are skipped, like `toParams`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Query(Vec<(String, String)>);

impl Query {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, key: &str, value: impl Display) -> &mut Self {
        let value = value.to_string();
        if !value.is_empty() {
            self.0.push((key.to_owned(), value));
        }
        self
    }

    pub fn push_opt(&mut self, key: &str, value: Option<impl Display>) -> &mut Self {
        if let Some(value) = value {
            self.push(key, value);
        }
        self
    }

    /// `limit` (clamped to 1..=500) and `offset`.
    pub fn page(&mut self, offset: u32, limit: u32) -> &mut Self {
        self.push("limit", limit.clamp(1, MAX_PAGE_SIZE));
        self.push("offset", offset)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn pairs(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SortOrder {
    #[default]
    Asc,
    Desc,
}

impl SortOrder {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlbumSort {
    Added,
    RecentlyPlayed,
    Date,
    Year,
    /// Browse only.
    Title,
    /// Browse only.
    Random,
}

impl AlbumSort {
    /// Every sort the browse list offers, in menu order.
    pub const ALL: [Self; 6] = [
        Self::Added,
        Self::Title,
        Self::RecentlyPlayed,
        Self::Date,
        Self::Year,
        Self::Random,
    ];

    fn browse(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::RecentlyPlayed => "recentlyPlayed",
            Self::Date => "date",
            Self::Year => "year",
            Self::Title => "title",
            Self::Random => "random",
        }
    }

    fn legacy(self) -> Option<&'static str> {
        match self {
            Self::Title | Self::Random => None,
            other => Some(other.browse()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackSort {
    Title,
    Plays,
    RecentlyPlayed,
    Date,
    Year,
    /// Browse only.
    Album,
    /// Browse only.
    Duration,
    /// Browse only.
    Random,
    /// Browse only.
    Added,
}

impl TrackSort {
    /// Every sort the browse list offers, in menu order.
    pub const ALL: [Self; 8] = [
        Self::Added,
        Self::Album,
        Self::Title,
        Self::Plays,
        Self::RecentlyPlayed,
        Self::Date,
        Self::Year,
        Self::Duration,
    ];

    fn browse(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Plays => "plays",
            Self::RecentlyPlayed => "recentlyPlayed",
            Self::Date => "date",
            Self::Year => "year",
            Self::Album => "album",
            Self::Duration => "duration",
            Self::Random => "random",
            Self::Added => "added",
        }
    }

    fn legacy(self) -> Option<&'static str> {
        match self {
            Self::Album | Self::Duration | Self::Random | Self::Added => None,
            other => Some(other.browse()),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AlbumListQuery {
    pub artist_id: Option<i64>,
    pub genre: Option<String>,
    pub search: Option<String>,
    pub sort: Option<AlbumSort>,
    pub order: Option<SortOrder>,
    pub year_from: Option<i32>,
    pub year_to: Option<i32>,
    /// Seed of `sort=random`; keep it for every page of one list.
    pub seed: Option<i64>,
}

impl AlbumListQuery {
    /// Endpoint path and parameters (without paging).
    pub fn source(&self, browse: bool) -> (&'static str, Query) {
        let mut q = Query::new();
        q.push_opt("artist", self.artist_id);
        q.push_opt("genre", self.genre.as_deref());
        q.push_opt("q", self.search.as_deref());
        if !browse {
            q.push_opt("sort", self.sort.and_then(AlbumSort::legacy));
            // /albums only filters an exact year.
            if self.year_from.is_some_and(|y| y > 0) && self.year_from == self.year_to {
                q.push_opt("year", self.year_from);
            }
            return ("/albums", q);
        }
        let sort = self.sort.map(AlbumSort::browse);
        if sort.is_some() {
            q.push_opt("order", self.order.map(SortOrder::as_str));
        }
        q.push_opt("sort", sort);
        q.push_opt("yearFrom", self.year_from.filter(|y| *y > 0));
        q.push_opt("yearTo", self.year_to.filter(|y| *y > 0));
        if self.sort == Some(AlbumSort::Random) {
            q.push_opt("seed", self.seed);
        }
        ("/browse/albums", q)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TrackListQuery {
    pub album_id: Option<i64>,
    pub artist_id: Option<i64>,
    pub genre: Option<String>,
    pub search: Option<String>,
    pub sort: Option<TrackSort>,
    pub order: Option<SortOrder>,
    pub year_from: Option<i32>,
    pub year_to: Option<i32>,
    /// Seed of `sort=random`; keep it for every page of one list.
    pub seed: Option<i64>,
}

impl TrackListQuery {
    /// Endpoint path and parameters (without paging).
    pub fn source(&self, browse: bool) -> (&'static str, Query) {
        let mut q = Query::new();
        q.push_opt("album", self.album_id);
        q.push_opt("artist", self.artist_id);
        q.push_opt("genre", self.genre.as_deref());
        q.push_opt("q", self.search.as_deref());
        if !browse {
            q.push_opt("sort", self.sort.and_then(TrackSort::legacy));
            q.push_opt("yearFrom", self.year_from.filter(|y| *y > 0));
            q.push_opt("yearTo", self.year_to.filter(|y| *y > 0));
            return ("/tracks", q);
        }
        let sort = self.sort.map(TrackSort::browse);
        q.push_opt("sort", sort);
        q.push_opt("yearFrom", self.year_from.filter(|y| *y > 0));
        q.push_opt("yearTo", self.year_to.filter(|y| *y > 0));
        if sort.is_some() {
            q.push_opt("order", self.order.map(SortOrder::as_str));
        }
        if self.sort == Some(TrackSort::Random) {
            q.push_opt("seed", self.seed);
        }
        ("/browse/tracks", q)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtistSort {
    Name,
    AlbumCount,
    TrackCount,
    /// Browse only.
    Random,
}

impl ArtistSort {
    /// Every sort the browse list offers, in menu order.
    pub const ALL: [Self; 4] = [Self::Name, Self::AlbumCount, Self::TrackCount, Self::Random];

    fn browse(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::AlbumCount => "albums",
            Self::TrackCount => "tracks",
            Self::Random => "random",
        }
    }

    fn legacy(self) -> Option<&'static str> {
        match self {
            Self::Random => None,
            other => Some(other.browse()),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ArtistListQuery {
    /// Only album artists (`role=album`), the default list.
    pub album_artists_only: bool,
    pub favorite: bool,
    pub search: Option<String>,
    pub sort: Option<ArtistSort>,
    pub order: Option<SortOrder>,
    /// Seed of `sort=random`; keep it for every page of one list.
    pub seed: Option<i64>,
}

impl ArtistListQuery {
    /// Endpoint path and parameters (without paging).
    pub fn source(&self, browse: bool) -> (&'static str, Query) {
        let mut q = Query::new();
        q.push(
            "role",
            if self.album_artists_only {
                "album"
            } else {
                "all"
            },
        );
        if self.favorite {
            q.push("favorite", "true");
        }
        q.push_opt("q", self.search.as_deref());
        if !browse {
            q.push_opt("sort", self.sort.and_then(ArtistSort::legacy));
            return ("/artists", q);
        }
        let sort = self.sort.map(ArtistSort::browse);
        q.push_opt("sort", sort);
        if sort.is_some() {
            q.push_opt("order", self.order.map(SortOrder::as_str));
        }
        if self.sort == Some(ArtistSort::Random) {
            q.push_opt("seed", self.seed);
        }
        ("/browse/artists", q)
    }
}

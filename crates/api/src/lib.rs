//! 032 Music Server HTTP client and types. No UI, no mpv.

mod client;
mod error;
mod query;
mod types;

pub use client::{
    API_REVISION, Client, MAX_PAGE_SIZE, authenticate, authenticate_client, normalize_base_url,
};
pub use error::{ApiError, AuthError};
pub use query::{
    AlbumListQuery, AlbumSort, ArtistListQuery, ArtistSort, Query, SortOrder, TrackListQuery,
    TrackSort,
};
pub use reqwest::Method;
pub use types::*;
pub use url::Url;

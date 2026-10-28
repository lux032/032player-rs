//! Client state and the glue between api, session and the audio engine.
//! No UI and no libmpv: the engine comes in through [`Engine`].

pub mod artwork_keys;
mod controller;
mod engine;
pub mod format;
mod images;
pub mod library;
mod queue;
mod reporter;
mod secrets;
mod settings;
mod storage;
pub mod view;

pub use controller::{
    Command, Commander, CoreConfig, CoreHandle, Event, LoginFailure, NowPlaying, PlaylistEdit,
    QueueEntry, QueuePosition, SavedLogin, SignOutReason, SleepStatus, SleepTimer, TrackSource,
    start,
};
pub use engine::{AudioDevice, Engine, EngineEnd, EngineEvent};
pub use images::{Rgba, decode, server_size};
pub use library::{
    AlbumItem, ArtistItem, FavoriteKind, ListItem, ListKind, ListQuery, ListSort, PlaylistItem,
    SortOrder, TrackItem,
};
pub use queue::{Queue, QueueItem, Repeat, Step};
pub use secrets::{MemorySecrets, SecretStore};
pub use settings::Settings;
pub use storage::FileStorage;
pub use view::{Layout, RowKind, View, ViewHeader, ViewRow, ViewState, ViewToolbar};

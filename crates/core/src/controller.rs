//! The core loop: owns all client state, runs on tokio, takes [`Command`]s
//! from the UI and [`EngineEvent`]s from the audio engine, and pushes
//! [`Event`]s to a sink. The UI never blocks on the network.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use api::{AuthError, Capabilities, Client, Feature, Track};
use session::{AdapterStatus, AdapterTrack};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::engine::{AudioDevice, Engine, EngineEnd, EngineEvent};
use crate::format;
use crate::images::{Images, Rgba};
use crate::library::{self, FavoriteKind, ListQuery, PlaylistItem, Seeds};
use crate::queue::{Queue, QueueSnapshot, Repeat, Step};
use crate::reporter::{Reporter, ReporterMsg};
use crate::secrets::{SecretStore, TokenKind, token_key};
use crate::settings::Settings;
use crate::storage::FileStorage;
use crate::view as views;
use crate::view::{View, ViewHeader, ViewPage, ViewRow, ViewState};
use session::KeyValueStorage;

/// "Previous" restarts the track instead after this much playback.
const RESTART_THRESHOLD_MS: i64 = 10_000;
/// Position updates to the UI and the reporter are coarsened to this step.
const POSITION_STEP_MS: i64 = 250;

/// The timed sleep timer counts down in steps of this many ms of playback.
const SLEEP_TICK_MS: i64 = 1_000;
/// Settings changes are written this long after the last one (volume drags).
const SETTINGS_SAVE_DELAY: Duration = Duration::from_millis(500);

/// Key of the last signed-in server in `session.json`.
const SERVER_KEY: &str = "server";
/// Playback position (ms) of the saved queue's current entry, in `session.json`.
const POSITION_KEY: &str = "positionMs";
/// The queue file is written this long after the last change.
const QUEUE_SAVE_DELAY: Duration = Duration::from_secs(1);
/// While playing, the position is saved every this many ms.
const POSITION_SAVE_STEP_MS: i64 = 10_000;
const QUEUE_FILE: &str = "queue.json";

/// `queue.json`: the queue of one server.
#[derive(serde::Serialize, serde::Deserialize)]
struct SavedQueue {
    server: String,
    queue: QueueSnapshot,
}

#[derive(Clone)]
pub struct CoreConfig {
    /// Where the playback resume snapshot and `session.json` live.
    pub data_dir: PathBuf,
    /// API token and media token per server.
    pub secrets: Arc<dyn SecretStore>,
    /// Loaded by the app with [`Settings::load`] (the engine is created
    /// with them); the core saves changes to `settings.json`.
    pub settings: Settings,
}

#[derive(Debug, PartialEq)]
pub enum Command {
    /// Announce the saved sign-in ([`Event::SavedLogin`]) and sign in with
    /// it when both tokens are saved. Sent once at startup.
    Restore,
    /// Sign in; an empty token falls back to the one saved for the server.
    /// On success the server and both tokens are saved.
    Login {
        server_url: String,
        api_token: String,
        media_token: String,
    },
    /// Sign out and forget both saved tokens (the server stays prefilled).
    Logout,
    /// Play row `index` of the current view ("play from here").
    PlayRow(usize),
    /// Play every track of the current view.
    PlayView,
    /// Play a whole album (grid card context menu).
    PlayAlbum {
        album_id: i64,
    },
    /// Insert tracks after the current entry or at the end of the queue.
    Enqueue {
        source: TrackSource,
        at: QueuePosition,
    },
    PlayQueueItem(u64),
    RemoveFromQueue(u64),
    /// Stops playback and empties the queue.
    ClearQueue,
    SetRepeat(Repeat),
    SetShuffle(bool),
    /// Whole-library random play: the queue becomes a random sample of every
    /// track, starting at the first one (with shuffle on).
    ShuffleAll,
    TogglePause,
    /// Stop (keeps the queue; play starts the current entry over).
    Stop,
    Next,
    Previous,
    Seek {
        position_ms: i64,
    },
    SetVolume(f64),
    /// Answered with [`Event::AudioDevices`].
    RefreshAudioDevices,
    /// `None` = system default. Answered with [`Event::AudioDevice`].
    SetAudioDevice(Option<String>),
    /// Answered with [`Event::Exclusive`] (the old value on failure).
    SetExclusive(bool),
    SetShowLyrics(bool),
    SetCloseToTray(bool),
    /// Start (`Some`) or cancel the sleep timer.
    SetSleepTimer(Option<SleepTimer>),
    /// Load artwork `path` for display at `px` physical pixels (long side);
    /// answered with [`Event::Image`] carrying `key`.
    LoadImage {
        key: String,
        path: String,
        px: u32,
    },

    // ------------------------------------------------------------ catalog
    /// Switches the main pane to `view` and loads it.
    Navigate(View),
    /// Replaces the current list's query (sort, order, filters) and reloads.
    ListQuery {
        query: ListQuery,
    },
    /// Loads more rows (scroll near the end).
    LoadMore,
    /// `q` search (`Command::Navigate(View::Search)` shows the results).
    Search(String),
    /// `PUT` / `DELETE /{albums|tracks|artists}/{id}/favorite`.
    SetFavorite {
        kind: FavoriteKind,
        id: i64,
        favorite: bool,
    },
    /// The playlists of the "add to playlist" menu.
    RefreshPlaylists,

    // ------------------------------------------------------------ playlists
    CreatePlaylist {
        name: String,
        description: String,
    },
    /// Renames a playlist, keeping its description.
    RenamePlaylist {
        id: i64,
        name: String,
        description: String,
    },
    DeletePlaylist(i64),
    /// Appends tracks (`Some`) or a whole album (`None`) to a playlist.
    AddToPlaylist {
        playlist_id: i64,
        track_ids: Vec<i64>,
        album_id: Option<i64>,
    },
    /// Removes tracks and reports the new order, in one `expectedRevision`.
    RemoveFromPlaylist {
        playlist_id: i64,
        track_ids: Vec<i64>,
    },
    /// Moves the track at `from` to `to` (playlist detail order).
    MovePlaylistItem {
        playlist_id: i64,
        from: usize,
        to: usize,
    },
    /// Replaces the playlist artwork with the file at `path`.
    UploadPlaylistArtwork {
        playlist_id: i64,
        path: PathBuf,
    },
    DeletePlaylistArtwork(i64),
}

/// When playback stops by itself (the Electron sleep timer modes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SleepTimer {
    /// At the next change of track (the current one ends or is skipped).
    EndOfTrack,
    /// After the last track of the current album.
    EndOfAlbum,
    /// After this many minutes of playback.
    Minutes(u32),
}

/// The running sleep timer, for the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SleepStatus {
    pub timer: SleepTimer,
    /// Playback time left (`Minutes` only).
    pub remaining_ms: Option<i64>,
}

struct Sleep {
    timer: SleepTimer,
    remaining_ms: i64,
    /// `EndOfAlbum`: the album to finish.
    album_id: Option<i64>,
}

/// Tracks to add to the queue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackSource {
    /// Row `index` of the current view.
    Row(usize),
    /// One track by id (a row of a mixed page, e.g. search results).
    Track(i64),
    /// The whole current view.
    View,
    /// A whole album, by id (a card's context menu).
    Album(i64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueuePosition {
    /// Right after the current entry ("play next").
    Next,
    Last,
}

/// What to do with fetched album tracks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Enqueue {
    Play,
    At(QueuePosition),
}

impl From<QueuePosition> for Enqueue {
    fn from(at: QueuePosition) -> Self {
        Self::At(at)
    }
}

/// A queue row for the UI.
#[derive(Clone, Debug, PartialEq)]
pub struct QueueEntry {
    pub uid: u64,
    pub title: String,
    pub artist: String,
    pub duration: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoginFailure {
    CredentialsRequired,
    TokensMustDiffer,
    ApiTokenInvalid,
    UnsupportedRevision(i64),
    MediaTokenInvalid,
    InvalidUrl,
    Other(String),
}

impl From<AuthError> for LoginFailure {
    fn from(e: AuthError) -> Self {
        match e {
            AuthError::CredentialsRequired => Self::CredentialsRequired,
            AuthError::TokensMustDiffer => Self::TokensMustDiffer,
            AuthError::ApiTokenInvalid => Self::ApiTokenInvalid,
            AuthError::UnsupportedRevision(r) => Self::UnsupportedRevision(r),
            AuthError::MediaTokenInvalid => Self::MediaTokenInvalid,
            AuthError::Api(api::ApiError::Url(_)) => Self::InvalidUrl,
            AuthError::Api(e) => Self::Other(e.to_string()),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NowPlaying {
    pub track_id: i64,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub artwork: String,
    /// Absolute artwork URL (with the media token) for the system media
    /// controls.
    pub artwork_url: Option<String>,
    pub duration_ms: i64,
    pub quality: Option<String>,
}

/// What the sign-in form can reuse.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SavedLogin {
    /// Last signed-in server; empty when none.
    pub server_url: String,
    pub has_api_token: bool,
    pub has_media_token: bool,
    /// Signing in with the saved tokens now (`LoggedIn` / `LoginFailed` follows).
    pub signing_in: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignOutReason {
    /// The user signed out.
    User,
    /// The server rejected the API token (rotated): enter a new one; the
    /// media token is kept.
    Expired,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    SavedLogin(SavedLogin),
    LoginFailed(LoginFailure),
    LoggedIn {
        server_url: String,
    },
    /// Back to the sign-in form; playback stopped and lists cleared.
    /// A [`Event::SavedLogin`] with what is left saved follows.
    SignedOut(SignOutReason),
    /// The main pane switched to `view`; rows follow.
    ViewReset {
        view: View,
        header: ViewHeader,
    },
    /// The header of the current view changed (first page of a list).
    ViewHeader(ViewHeader),
    /// One page of the current view, appended to its rows.
    ViewRows {
        view: View,
        rows: Vec<ViewRow>,
        total: usize,
        loaded: usize,
    },
    /// Tracks behind the current view's rows: `PlayRow(index)` resolves there.
    ViewTracks(Vec<Track>),
    /// The current view's load failed; `empty` means nothing was shown yet
    /// (the pane then offers a retry instead of waiting for a scroll).
    LoadFailed {
        view: View,
        empty: bool,
        message: String,
    },
    /// The playlists of the "add to playlist" menu.
    Playlists(Vec<PlaylistItem>),
    /// An item's favorite state changed (the loaded rows are updated).
    Favorite {
        kind: FavoriteKind,
        id: i64,
        favorite: bool,
    },
    Image {
        key: String,
        image: Rgba,
    },
    /// The artwork could not be loaded; the UI releases its request so a
    /// later repaint may try again.
    ImageFailed {
        key: String,
    },
    NowPlaying(Option<NowPlaying>),
    /// The whole queue in play order, and the current row.
    Queue {
        items: Vec<QueueEntry>,
        current: Option<usize>,
    },
    /// Only the current row moved.
    QueuePosition(Option<usize>),
    Repeat(Repeat),
    Shuffle(bool),
    SleepTimer(Option<SleepStatus>),
    Position {
        position_ms: i64,
        duration_ms: i64,
    },
    Paused(bool),
    Buffering(bool),
    Volume(f64),
    AudioDevices(Vec<AudioDevice>),
    /// Selected output device (`None` = system default).
    AudioDevice(Option<String>),
    Exclusive(bool),
    /// Lines of the current track (empty: none). `synced` false = plain text.
    Lyrics {
        lines: Vec<String>,
        synced: bool,
        source: Option<String>,
    },
    LyricsActive(Option<usize>),
    /// Something the user should see (load failure, playback error).
    Notice(String),
}

/// Messages from spawned tasks back into the loop.
enum Internal {
    Reporter(u64, ReporterMsg),
    LoggedIn(Box<(Client, Capabilities)>),
    LoginFailed(LoginFailure),
    /// A request with this API token got 401.
    Unauthorized {
        api_token: String,
    },
    /// One page of the current view; `revision` drops stale answers.
    ViewPage {
        generation: u64,
        revision: u64,
        view: View,
        query: Box<ListQuery>,
        page: Box<Result<ViewPage, api::ApiError>>,
    },
    /// A whole-library random sample for shuffle-all.
    RandomTracks {
        generation: u64,
        tracks: Result<Vec<Track>, api::ApiError>,
    },
    /// A playlist write finished.
    PlaylistChanged {
        generation: u64,
        /// `Some(id)`: re-read that playlist; `None`: it was deleted.
        reload: Option<i64>,
        /// Revision the server returned (bumped the open playlist).
        revision: Option<i64>,
        /// Tracks appended to the open playlist, in order.
        appended: Vec<i64>,
        /// Removed tracks (the open playlist drops them).
        removed: Vec<i64>,
        /// Order sent by a reorder (the open playlist adopts it).
        order: Option<Vec<i64>>,
        /// The write took the editor's `busy` flag (remove / reorder): only
        /// such a write may release it, otherwise an unrelated success would
        /// unlock a write that is still in flight.
        exclusive: bool,
        /// User-facing result on success ("已添加 3 首到歌单 X").
        notice: Option<String>,
        result: Result<(), api::ApiError>,
    },
    Playlists {
        generation: u64,
        items: Result<Vec<PlaylistItem>, api::ApiError>,
    },
    FavoriteSet {
        generation: u64,
        kind: FavoriteKind,
        id: i64,
        favorite: bool,
        result: Result<(), api::ApiError>,
    },
    /// Tracks of an album (playing it or queueing it whole).
    AlbumTracks(Result<Vec<Track>, api::ApiError>, Enqueue),
    AudioDeviceSet(Option<String>, Result<(), String>),
    ExclusiveSet(bool, Result<(), String>),
    SaveSettings(u64),
    SaveQueue(u64),
    SleepTick(u64),
    Lyrics {
        track_id: i64,
        lyrics: Option<api::Lyrics>,
    },
}

/// Handle to the running core. Dropping it does not stop the core; call
/// [`CoreHandle::shutdown`].
pub struct CoreHandle {
    commands: UnboundedSender<Command>,
    engine: UnboundedSender<EngineEvent>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

/// Cloneable command sender for UI callbacks.
#[derive(Clone, Debug)]
pub struct Commander(UnboundedSender<Command>);

impl Commander {
    pub fn send(&self, command: Command) {
        let _ = self.0.send(command);
    }
}

impl CoreHandle {
    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    pub fn commander(&self) -> Commander {
        Commander(self.commands.clone())
    }

    /// Feed an engine signal (callable from any thread).
    pub fn engine_event(&self, event: EngineEvent) {
        let _ = self.engine.send(event);
    }

    /// A cloneable sender for engine signals.
    pub fn engine_sender(&self) -> impl Fn(EngineEvent) + Send + Sync + 'static {
        let tx = self.engine.clone();
        move |event| {
            let _ = tx.send(event);
        }
    }

    /// Reports `client_closed`, stops the engine and joins the core thread.
    pub fn shutdown(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

type Sink = Arc<dyn Fn(Event) + Send + Sync>;

/// Starts the core on its own tokio runtime thread.
pub fn start(
    engine: Arc<dyn Engine>,
    config: CoreConfig,
    sink: impl Fn(Event) + Send + Sync + 'static,
) -> CoreHandle {
    let (commands, commands_rx) = mpsc::unbounded_channel();
    let (engine_tx, engine_rx) = mpsc::unbounded_channel();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let sink: Sink = Arc::new(sink);
    let thread = std::thread::Builder::new()
        .name("core".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("tokio runtime");
            runtime.block_on(async move {
                let (internal_tx, internal_rx) = mpsc::unbounded_channel();
                let mut core = Core::new(engine, config, sink, internal_tx);
                core.run(commands_rx, engine_rx, internal_rx, shutdown_rx)
                    .await;
            });
            runtime.shutdown_timeout(Duration::from_millis(500));
        })
        .expect("core thread");
    CoreHandle {
        commands,
        engine: engine_tx,
        shutdown: Some(shutdown_tx),
        thread: Some(thread),
    }
}

/// Bridges reporter messages into the internal channel, tagged with the
/// sign-in generation so a dropped reporter's timers are ignored.
fn reporter_channel(
    internal: &UnboundedSender<Internal>,
    generation: u64,
) -> UnboundedSender<ReporterMsg> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let internal = internal.clone();
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if internal.send(Internal::Reporter(generation, msg)).is_err() {
                break;
            }
        }
    });
    tx
}

struct Library {
    client: Client,
    capabilities: Capabilities,
    images: Arc<Images>,
}

/// A finished playlist write, as the loop handles it.
struct PlaylistChanged {
    generation: u64,
    reload: Option<i64>,
    revision: Option<i64>,
    appended: Vec<i64>,
    removed: Vec<i64>,
    order: Option<Vec<i64>>,
    exclusive: bool,
    notice: Option<String>,
    result: Result<(), api::ApiError>,
}

/// The playlist open in the editor: its tracks in order and the revision
/// returned by the server (`expectedRevision` of remove / order).
#[derive(Clone, Debug)]
pub struct PlaylistEdit {
    pub id: i64,
    pub name: String,
    pub revision: i64,
    pub track_ids: Vec<i64>,
    pub description: String,
    /// A write is in flight: its revision will not match the local order
    /// until it returns, so a second write is refused rather than sent with
    /// a mismatched revision and track list.
    pub busy: bool,
}

/// Runs a blocking secret-store call off the async workers.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    tokio::task::spawn_blocking(f).await.ok()
}

async fn read_token(secrets: &Arc<dyn SecretStore>, server: &str, kind: TokenKind) -> String {
    let secrets = secrets.clone();
    let key = token_key(server, kind);
    match blocking(move || secrets.get(&key)).await {
        Some(Ok(value)) => value.unwrap_or_default(),
        Some(Err(e)) => {
            tracing::warn!("reading saved {kind:?} token: {e}");
            String::new()
        }
        None => String::new(),
    }
}

/// Saves (`Some`) or deletes (`None`) a token.
async fn write_token(
    secrets: &Arc<dyn SecretStore>,
    server: &str,
    kind: TokenKind,
    value: Option<String>,
) {
    let secrets = secrets.clone();
    let key = token_key(server, kind);
    let result = blocking(move || match value {
        Some(value) => secrets.set(&key, &value),
        None => secrets.delete(&key),
    })
    .await;
    if let Some(Err(e)) = result {
        tracing::warn!("saving {kind:?} token: {e}");
    }
}

/// Signs in; empty tokens come from the store. Saves both tokens on
/// success; forgets a saved token the server rejected.
async fn sign_in(
    secrets: Arc<dyn SecretStore>,
    sink: Sink,
    server_url: String,
    api_token: String,
    media_token: String,
) -> Internal {
    let Ok(server) = api::normalize_base_url(&server_url) else {
        return Internal::LoginFailed(LoginFailure::InvalidUrl);
    };
    let api_saved = api_token.is_empty();
    let api_token = if api_saved {
        read_token(&secrets, &server, TokenKind::Api).await
    } else {
        api_token
    };
    let media_saved = media_token.is_empty();
    let media_token = if media_saved {
        read_token(&secrets, &server, TokenKind::Media).await
    } else {
        media_token
    };
    match api::authenticate(&server, &api_token, &media_token).await {
        Ok(ok) => {
            write_token(&secrets, &server, TokenKind::Api, Some(api_token)).await;
            write_token(&secrets, &server, TokenKind::Media, Some(media_token)).await;
            Internal::LoggedIn(Box::new(ok))
        }
        Err(e) => {
            let failure = LoginFailure::from(e);
            let rejected = match failure {
                LoginFailure::ApiTokenInvalid if api_saved && !api_token.is_empty() => {
                    Some(TokenKind::Api)
                }
                LoginFailure::MediaTokenInvalid if media_saved && !media_token.is_empty() => {
                    Some(TokenKind::Media)
                }
                _ => None,
            };
            if let Some(kind) = rejected {
                write_token(&secrets, &server, kind, None).await;
                sink(Event::SavedLogin(SavedLogin {
                    server_url: server,
                    has_api_token: kind != TokenKind::Api && !api_token.is_empty(),
                    has_media_token: kind != TokenKind::Media && !media_token.is_empty(),
                    signing_in: false,
                }));
            }
            Internal::LoginFailed(failure)
        }
    }
}

/// The message to send when `e` means the API token was rejected.
fn unauthorized(client: &Client, e: &api::ApiError) -> Option<Internal> {
    e.is_unauthorized().then(|| Internal::Unauthorized {
        api_token: client.api_token().to_owned(),
    })
}

struct Core {
    engine: Arc<dyn Engine>,
    config: CoreConfig,
    sink: Sink,
    internal: UnboundedSender<Internal>,
    /// `session.json`: the last signed-in server.
    session_file: FileStorage,
    library: Option<Library>,
    reporter: Option<Reporter>,
    /// Bumped on every sign-in and sign-out; results of older background
    /// work (list pages, reporter timers) are dropped.
    list_generation: u64,
    queue: Queue,
    /// Nothing loaded in the engine (before the first play, after the end
    /// of the queue or a failure): play reloads the current entry.
    idle: bool,
    /// Between `load` and the new file's `FileLoaded`: progress, duration
    /// and end-of-file signals still belong to the previous file.
    loading: bool,
    paused: bool,
    position_ms: i64,
    reported_step: i64,
    engine_duration_ms: i64,
    lyrics: Option<lyrics::SyncedLyrics>,
    active_line: Option<usize>,
    settings: Settings,
    /// Bumped per settings change; `saved_revision` is the last written.
    settings_revision: u64,
    saved_revision: u64,
    /// An output device or exclusive mode change is running.
    output_busy: bool,
    /// Queue entry appended to the engine for gapless playback.
    preloaded: Option<u64>,
    /// URL of the file being loaded; a different `FileLoaded` is a stale preload.
    expected_path: Option<String>,
    /// Entry of the last started play (repeat-one passes start it again).
    started_uid: Option<u64>,
    /// Restored queue entry and the position to continue it at.
    resume_at: Option<(u64, i64)>,
    /// Seek to this once the loading file is loaded (resumed position).
    pending_seek_ms: Option<i64>,
    /// Bumped per queue change; the debounced save writes the latest.
    queue_revision: u64,
    saved_queue_revision: u64,
    saved_position_step: i64,
    sleep: Option<Sleep>,
    /// Bumped per sleep timer start / cancel; stale ticks are ignored.
    sleep_generation: u64,
    /// The view shown in the main pane: rows, tracks and header.
    view: ViewState,
    /// Random seeds of `sort=random`.
    seeds: Seeds,
    /// Playlist being edited (its tracks and revision).
    playlist: Option<PlaylistEdit>,
    /// The playlists of the "add to playlist" menu.
    playlists: Vec<PlaylistItem>,
}

impl Core {
    fn new(
        engine: Arc<dyn Engine>,
        config: CoreConfig,
        sink: Sink,
        internal: UnboundedSender<Internal>,
    ) -> Self {
        let session_file = FileStorage::open(config.data_dir.join("session.json"));
        let settings = config.settings.clone();
        let mut queue = Queue::default();
        queue.set_shuffle(settings.shuffle);
        Self {
            engine,
            config,
            sink,
            internal,
            session_file,
            library: None,
            reporter: None,
            list_generation: 0,
            queue,
            idle: true,
            loading: false,
            paused: false,
            position_ms: 0,
            reported_step: -1,
            engine_duration_ms: 0,
            lyrics: None,
            active_line: None,
            settings,
            settings_revision: 0,
            saved_revision: 0,
            output_busy: false,
            preloaded: None,
            expected_path: None,
            started_uid: None,
            resume_at: None,
            pending_seek_ms: None,
            queue_revision: 0,
            saved_queue_revision: 0,
            saved_position_step: -1,
            sleep: None,
            sleep_generation: 0,
            view: ViewState::default(),
            seeds: Seeds::default(),
            playlist: None,
            playlists: Vec::new(),
        }
    }

    fn emit(&self, event: Event) {
        (self.sink)(event);
    }

    fn now_playing(&self, track: &Track) -> NowPlaying {
        let artwork_url = self.library.as_ref().and_then(|l| {
            l.client
                .image_url(&track.artwork_url, Some(512))
                .map(|u| u.to_string())
        });
        NowPlaying {
            track_id: track.id,
            title: track.title.clone(),
            artist: track.artist.clone(),
            album: track.album.clone(),
            artwork: track.artwork_url.clone(),
            artwork_url,
            duration_ms: track.duration_millis,
            quality: format::audio_quality(track),
        }
    }

    /// Applies `f` to the settings and schedules a save if anything changed.
    fn update_settings(&mut self, f: impl FnOnce(&mut Settings)) {
        let before = self.settings.clone();
        f(&mut self.settings);
        if self.settings == before {
            return;
        }
        self.settings_revision += 1;
        let revision = self.settings_revision;
        let tx = self.internal.clone();
        tokio::spawn(async move {
            tokio::time::sleep(SETTINGS_SAVE_DELAY).await;
            let _ = tx.send(Internal::SaveSettings(revision));
        });
    }

    fn save_settings(&mut self) {
        if self.saved_revision == self.settings_revision {
            return;
        }
        match self.settings.save(&self.config.data_dir) {
            Ok(()) => self.saved_revision = self.settings_revision,
            Err(e) => tracing::warn!("saving settings: {e}"),
        }
    }

    /// Runs an output change (device, exclusive) on a blocking thread.
    fn change_output(
        &mut self,
        change: impl FnOnce(&dyn Engine) -> Result<(), String> + Send + 'static,
        done: impl FnOnce(Result<(), String>) -> Internal + Send + 'static,
    ) {
        self.output_busy = true;
        let engine = self.engine.clone();
        let tx = self.internal.clone();
        tokio::spawn(async move {
            let result = tokio::task::spawn_blocking(move || change(engine.as_ref()))
                .await
                .unwrap_or_else(|e| Err(e.to_string()));
            let _ = tx.send(done(result));
        });
    }

    async fn run(
        &mut self,
        mut commands: UnboundedReceiver<Command>,
        mut engine: UnboundedReceiver<EngineEvent>,
        mut internal: UnboundedReceiver<Internal>,
        mut shutdown: tokio::sync::oneshot::Receiver<()>,
    ) {
        loop {
            tokio::select! {
                Some(command) = commands.recv() => self.command(command),
                Some(event) = engine.recv() => self.engine_event(event),
                Some(msg) = internal.recv() => self.internal(msg),
                _ = &mut shutdown => break,
            }
        }
        self.save_settings();
        self.save_position();
        if self.queue_revision != self.saved_queue_revision
            && let Some((path, bytes)) = self.queue_file()
        {
            write_atomic(&path, &bytes);
        }
        if let Some(reporter) = self.reporter.as_mut() {
            reporter.close(Duration::from_secs(2)).await;
        }
        let _ = self.engine.stop();
    }

    // ------------------------------------------------------------ commands

    fn command(&mut self, command: Command) {
        if Self::is_catalog(&command) {
            self.catalog_command(command);
        } else {
            self.base_command(command);
        }
    }

    /// Commands that the catalog handles (they need the signed-in server).
    fn is_catalog(command: &Command) -> bool {
        matches!(
            command,
            Command::Navigate(_)
                | Command::ListQuery { .. }
                | Command::LoadMore
                | Command::Search(_)
                | Command::RefreshPlaylists
                | Command::SetFavorite { .. }
                | Command::CreatePlaylist { .. }
                | Command::RenamePlaylist { .. }
                | Command::DeletePlaylist(_)
                | Command::AddToPlaylist { .. }
                | Command::RemoveFromPlaylist { .. }
                | Command::MovePlaylistItem { .. }
                | Command::UploadPlaylistArtwork { .. }
                | Command::DeletePlaylistArtwork(_)
        )
    }

    /// Runs a catalog command.
    fn catalog_command(&mut self, command: Command) {
        match command {
            Command::Navigate(view) => self.navigate(view),
            Command::ListQuery { query } => self.query_view(query),
            Command::LoadMore => self.load_more(),
            Command::Search(needle) => self.navigate(View::Search(needle)),
            Command::RefreshPlaylists => self.refresh_playlists(),
            Command::SetFavorite { kind, id, favorite } => self.set_favorite(kind, id, favorite),
            Command::CreatePlaylist { name, description } => {
                self.create_playlist(name, description)
            }
            Command::RenamePlaylist {
                id,
                name,
                description,
            } => self.rename_playlist(id, name, description),
            Command::DeletePlaylist(id) => self.delete_playlist(id),
            Command::AddToPlaylist {
                playlist_id,
                track_ids,
                album_id,
            } => self.add_to_playlist(playlist_id, track_ids, album_id),
            Command::RemoveFromPlaylist {
                playlist_id,
                track_ids,
            } => self.remove_from_playlist(playlist_id, track_ids),
            Command::MovePlaylistItem {
                playlist_id,
                from,
                to,
            } => self.move_playlist_item(playlist_id, from, to),
            Command::UploadPlaylistArtwork { playlist_id, path } => {
                self.upload_playlist_artwork(playlist_id, path)
            }
            Command::DeletePlaylistArtwork(id) => self.delete_playlist_artwork(id),
            _ => unreachable!("not a catalog command"),
        }
    }

    fn base_command(&mut self, command: Command) {
        match command {
            Command::Restore => {
                let server = self.session_file.get(SERVER_KEY).unwrap_or_default();
                if server.is_empty() {
                    self.emit(Event::SavedLogin(SavedLogin::default()));
                    return;
                }
                let secrets = self.config.secrets.clone();
                let sink = self.sink.clone();
                let tx = self.internal.clone();
                tokio::spawn(async move {
                    let api = read_token(&secrets, &server, TokenKind::Api).await;
                    let media = read_token(&secrets, &server, TokenKind::Media).await;
                    let signing_in = !api.is_empty() && !media.is_empty();
                    sink(Event::SavedLogin(SavedLogin {
                        server_url: server.clone(),
                        has_api_token: !api.is_empty(),
                        has_media_token: !media.is_empty(),
                        signing_in,
                    }));
                    if signing_in {
                        // Empty tokens: sign_in reads (and may forget) the saved ones.
                        let msg =
                            sign_in(secrets, sink, server, String::new(), String::new()).await;
                        let _ = tx.send(msg);
                    }
                });
            }
            Command::Login {
                server_url,
                api_token,
                media_token,
            } => {
                if self.library.is_some() {
                    return;
                }
                let secrets = self.config.secrets.clone();
                let sink = self.sink.clone();
                let tx = self.internal.clone();
                tokio::spawn(async move {
                    let msg = sign_in(secrets, sink, server_url, api_token, media_token).await;
                    let _ = tx.send(msg);
                });
            }
            Command::Logout => self.sign_out(SignOutReason::User),
            Command::PlayRow(index) => {
                let tracks = self.view.tracks.clone();
                if self.queue.replace(tracks, index).is_some() {
                    self.play_current(true);
                }
                self.queue_changed();
            }
            Command::PlayView => {
                let tracks = self.view.tracks.clone();
                if self.queue.replace(tracks, 0).is_some() {
                    self.play_current(true);
                }
                self.queue_changed();
            }
            Command::PlayAlbum { album_id } => self.fetch_album(album_id, Enqueue::Play),
            Command::Enqueue { source, at } => match source {
                TrackSource::Row(index) => {
                    if let Some(track) = self.view.tracks.get(index).cloned() {
                        self.enqueue(vec![track], at);
                    }
                }
                TrackSource::View => {
                    let tracks = self.view.tracks.clone();
                    self.enqueue(tracks, at);
                }
                TrackSource::Album(album_id) => self.fetch_album(album_id, at.into()),
                TrackSource::Track(id) => {
                    if let Some(track) = self.view.tracks.iter().find(|t| t.id == id).cloned() {
                        self.enqueue(vec![track], at);
                    }
                }
            },
            Command::PlayQueueItem(uid) => {
                if self.queue.go_to_uid(uid).is_some() {
                    self.play_current(true);
                    self.emit_queue_position();
                }
            }
            Command::RemoveFromQueue(uid) => self.remove_from_queue(uid),
            Command::ClearQueue => {
                self.stop_playback();
                self.queue.clear();
                self.emit(Event::NowPlaying(None));
                self.queue_changed();
            }
            Command::SetRepeat(repeat) => {
                self.update_settings(|s| s.repeat = repeat);
                self.emit(Event::Repeat(repeat));
                self.sync_preload();
            }
            Command::ShuffleAll => self.shuffle_all(),
            Command::SetShuffle(shuffle) => {
                self.queue.set_shuffle(shuffle);
                self.update_settings(|s| s.shuffle = shuffle);
                self.emit(Event::Shuffle(shuffle));
                self.queue_changed();
            }
            Command::TogglePause => {
                if self.queue.current().is_none() {
                    return;
                }
                if self.idle {
                    self.play_current(false);
                } else if let Err(e) = self.engine.set_paused(!self.paused) {
                    self.emit(Event::Notice(e));
                }
            }
            Command::Next => {
                let step = self.queue.manual_next(self.settings.repeat);
                self.step(step, true);
            }
            Command::Previous => {
                if self.queue.current().is_none() {
                    return;
                }
                if self.position_ms > RESTART_THRESHOLD_MS && !self.idle {
                    // Like mediaPrevious: a seek in the same session, not a new play.
                    self.seek(0);
                } else {
                    let step = self.queue.manual_previous(self.settings.repeat);
                    self.step(step, true);
                }
            }
            Command::Seek { position_ms } => self.seek(position_ms),
            Command::SetVolume(volume) => {
                if let Err(e) = self.engine.set_volume(volume) {
                    self.emit(Event::Notice(e));
                }
            }
            Command::RefreshAudioDevices => {
                let engine = self.engine.clone();
                let sink = self.sink.clone();
                tokio::spawn(async move {
                    match tokio::task::spawn_blocking(move || engine.audio_devices()).await {
                        Ok(Ok(devices)) => sink(Event::AudioDevices(devices)),
                        Ok(Err(e)) => tracing::warn!("audio devices: {e}"),
                        Err(e) => tracing::warn!("audio devices: {e}"),
                    }
                });
            }
            Command::SetAudioDevice(device) => {
                let device = device.filter(|d| !d.is_empty() && d != "auto");
                if self.output_busy || device == self.settings.audio_device {
                    self.emit(Event::AudioDevice(self.settings.audio_device.clone()));
                    return;
                }
                let name = device.clone().unwrap_or_else(|| "auto".into());
                self.change_output(
                    move |e| e.set_audio_device(&name),
                    move |r| Internal::AudioDeviceSet(device, r),
                );
            }
            Command::SetExclusive(enabled) => {
                if self.output_busy || enabled == self.settings.exclusive {
                    self.emit(Event::Exclusive(self.settings.exclusive));
                    return;
                }
                self.change_output(
                    move |e| e.set_exclusive(enabled),
                    move |r| Internal::ExclusiveSet(enabled, r),
                );
            }
            Command::SetShowLyrics(show) => self.update_settings(|s| s.show_lyrics = show),
            Command::SetCloseToTray(on) => self.update_settings(|s| s.close_to_tray = on),
            Command::SetSleepTimer(timer) => self.set_sleep_timer(timer),
            Command::Stop => {
                if !self.idle {
                    self.stop_playback();
                }
            }
            Command::LoadImage { key, path, px } => {
                let Some(library) = &self.library else {
                    // No server to load from: the request can never succeed,
                    // so release the claim on the UI side.
                    self.emit(Event::ImageFailed { key });
                    return;
                };
                let images = library.images.clone();
                let Some(url) = images.url(&path, px) else {
                    self.emit(Event::ImageFailed { key });
                    return;
                };
                let sink = self.sink.clone();
                tokio::spawn(async move {
                    match images.load(url, px).await {
                        Ok(image) => sink(Event::Image { key, image }),
                        Err(e) => {
                            tracing::debug!("artwork {path}: {e}");
                            // The UI must be able to ask again later.
                            sink(Event::ImageFailed { key });
                        }
                    }
                });
            }
            // Catalog commands: see `catalog_command`.
            Command::Navigate(_)
            | Command::ListQuery { .. }
            | Command::LoadMore
            | Command::Search(_)
            | Command::RefreshPlaylists
            | Command::SetFavorite { .. }
            | Command::CreatePlaylist { .. }
            | Command::RenamePlaylist { .. }
            | Command::DeletePlaylist(_)
            | Command::AddToPlaylist { .. }
            | Command::RemoveFromPlaylist { .. }
            | Command::MovePlaylistItem { .. }
            | Command::UploadPlaylistArtwork { .. }
            | Command::DeletePlaylistArtwork(_) => {}
        }
    }

    /// Shows `view` from scratch.
    fn navigate(&mut self, view: View) {
        self.view.reset(view);
        self.emit(Event::ViewReset {
            view: self.view.view.clone(),
            header: self.view.header.clone(),
        });
        self.load_view();
    }

    /// Replaces the query of the current list view and reloads it.
    fn query_view(&mut self, query: ListQuery) {
        let view = self.view.view.clone();
        // Keep the header the user is looking at: only the query changes.
        let skeleton = self.view.header.clone();
        self.view.reset_with(view, query, Some(&skeleton));
        self.emit(Event::ViewReset {
            view: self.view.view.clone(),
            header: self.view.header.clone(),
        });
        self.load_view();
    }

    /// Requests the next page of the current view.
    fn load_more(&mut self) {
        self.load_view();
    }

    fn load_view(&mut self) {
        let Some(library) = &self.library else { return };
        let Some(offset) = self.view.next else { return };
        if self.view.loading {
            return;
        }
        let view = self.view.view.clone();
        let query = self.view.query.clone();
        let revision = self.view.revision;
        let seed = self.seeds.for_query(&query, offset);
        self.view.next = None;
        self.view.loading = true;
        let client = library.client.clone();
        let capabilities = library.capabilities.clone();
        let generation = self.list_generation;
        let tx = self.internal.clone();
        tokio::spawn(async move {
            let page = views::load_page(&client, &capabilities, &view, &query, offset, seed).await;
            let _ = tx.send(Internal::ViewPage {
                generation,
                revision,
                view,
                query: Box::new(query),
                page: Box::new(page),
            });
        });
    }

    /// Applies a loaded page to the current view.
    fn view_page(
        &mut self,
        generation: u64,
        revision: u64,
        view: View,
        query: ListQuery,
        page: Result<ViewPage, api::ApiError>,
    ) {
        if generation != self.list_generation || self.view.revision != revision {
            return;
        }
        if self.view.view != view || self.view.query != query {
            return;
        }
        self.view.loading = false;
        let page = match page {
            Ok(page) => page,
            Err(e) => {
                // Keep the rows already loaded and let the UI retry: a first
                // page that failed has nothing to scroll, so the pane shows a
                // retry button (Event::LoadFailed) instead of `load-more`.
                self.view.next = Some(u32::try_from(self.view.rows.len()).unwrap_or(u32::MAX));
                self.emit(Event::LoadFailed {
                    view: self.view.view.clone(),
                    empty: self.view.rows.is_empty(),
                    message: e.to_string(),
                });
                self.notice_for("加载失败", &e);
                return;
            }
        };
        let n = page.rows.len();
        if let Some(header) = page.header {
            self.view.header = header;
            self.emit(Event::ViewHeader(self.view.header.clone()));
        }
        if let Some(edit) = page.playlist {
            self.playlist = Some(edit);
        }
        if let Some(notice) = page.notice {
            self.emit(Event::Notice(notice));
        }
        self.view.total = page.total;
        self.view.tracks.extend(page.tracks);
        self.view.rows.extend(page.rows);
        let loaded = self.view.rows.len();
        self.view.next = (n > 0 && loaded < page.total && view.is_paged())
            .then(|| u32::try_from(loaded).unwrap_or(u32::MAX));
        let end = loaded;
        let rows = self.view.rows[loaded - n..end].to_vec();
        self.emit(Event::ViewRows {
            view,
            rows,
            total: self.view.total,
            loaded,
        });
        let tracks = self.view.tracks.clone();
        if !tracks.is_empty() {
            self.emit(Event::ViewTracks(tracks));
        }
    }

    /// A user-facing error; an expired token signs out instead.
    fn notice_for(&mut self, what: &str, e: &api::ApiError) {
        if e.is_unauthorized() {
            self.sign_out(SignOutReason::Expired);
        } else {
            self.emit(Event::Notice(format!("{what}：{e}")));
        }
    }

    /// Flips the favorite flag of the loaded copies of an item.
    fn apply_favorite(&mut self, kind: FavoriteKind, id: i64, favorite: bool) {
        for row in &mut self.view.rows {
            // Only the rows of the same kind: a search page can hold an album
            // and a track with the same id.
            if row.id == id && row.kind.favorite_kind() == Some(kind) {
                row.favorite = favorite;
            }
        }
    }

    fn set_favorite(&mut self, kind: FavoriteKind, id: i64, favorite: bool) {
        let Some(library) = &self.library else { return };
        let client = library.client.clone();
        let tx = self.internal.clone();
        let generation = self.list_generation;
        tokio::spawn(async move {
            let result = client.set_favorite(kind.segment(), id, favorite).await;
            let _ = tx.send(Internal::FavoriteSet {
                generation,
                kind,
                id,
                favorite,
                result,
            });
        });
    }

    fn refresh_playlists(&mut self) {
        let Some(library) = &self.library else { return };
        let client = library.client.clone();
        let tx = self.internal.clone();
        let generation = self.list_generation;
        tokio::spawn(async move {
            let result = library::fetch_playlists(&client).await;
            let _ = tx.send(Internal::Playlists {
                generation,
                items: result,
            });
        });
    }

    // ------------------------------------------------------------ playlists

    /// A finished write: its result plus what the open playlist should adopt.
    fn playlist_write(&mut self, msg: PlaylistChanged) {
        if msg.generation != self.list_generation {
            return;
        }
        match msg.result {
            Ok(()) => {
                if let Some(notice) = msg.notice {
                    self.emit(Event::Notice(notice));
                }
                // Apply the change to the open editor so the next write uses
                // the new revision without a reload.
                if let Some(edit) = self.playlist.as_mut()
                    && msg.reload.is_none_or(|id| id == edit.id)
                {
                    if msg.exclusive {
                        edit.busy = false;
                    }
                    edit.track_ids.extend(msg.appended.iter().copied());
                    edit.track_ids.retain(|id| !msg.removed.contains(id));
                    if let Some(order) = &msg.order {
                        edit.track_ids.clone_from(order);
                    }
                    if let Some(revision) = msg.revision {
                        edit.revision = revision;
                    }
                }
                match msg.reload {
                    // Re-read the playlist only when it is on screen.
                    Some(id) if self.view.view == View::Playlist(id) => {
                        self.navigate(View::Playlist(id));
                    }
                    Some(_) => {
                        // Created: the playlists panel shows it too, keeping
                        // the sort the user picked.
                        if self.view.view == View::Playlists {
                            let query = self.view.query.clone();
                            self.query_view(query);
                        }
                        self.refresh_playlists();
                    }
                    None => {
                        // Deleted: drop the editor and leave a deleted
                        // playlist's page; the list is reloaded either way.
                        self.playlist = None;
                        if self.view.view.is_playlist() {
                            self.navigate(View::Playlists);
                        } else if self.view.view == View::Playlists {
                            let query = self.view.query.clone();
                            self.query_view(query);
                        }
                        self.refresh_playlists();
                    }
                }
            }
            Err(e) => {
                // Whatever failed, the editor must accept a write again: a
                // failure that kept `busy` would refuse every later edit.
                if let Some(edit) = self.playlist.as_mut() {
                    edit.busy = false;
                }
                let open = msg.reload.or_else(|| self.playlist.as_ref().map(|p| p.id));
                if e.code() == Some("playlist_conflict") {
                    // Another client edited it: reload so the next try uses
                    // the current revision.
                    if let Some(id) = open {
                        self.navigate(View::Playlist(id));
                    }
                    self.emit(Event::Notice(
                        "歌单已被其他客户端修改，已刷新，请重试".into(),
                    ));
                } else {
                    self.notice_for("修改歌单失败", &e);
                }
            }
        }
    }

    fn create_playlist(&mut self, name: String, description: String) {
        let Some(library) = &self.library else { return };
        let client = library.client.clone();
        let tx = self.internal.clone();
        let generation = self.list_generation;
        tokio::spawn(async move {
            let result = client.create_playlist(&name, &description).await;
            let _ = tx.send(Internal::PlaylistChanged {
                generation,
                reload: result.as_ref().ok().map(|playlist| playlist.id),
                revision: None,
                appended: Vec::new(),
                removed: Vec::new(),
                order: None,
                exclusive: false,
                notice: result
                    .as_ref()
                    .ok()
                    .map(|_| format!("已创建歌单「{name}」")),
                result: result.map(|_| ()),
            });
        });
    }

    /// Renames a playlist. `PATCH` replaces both fields, so the description is
    /// taken from the cached playlist, or read back when it is not loaded.
    fn rename_playlist(&mut self, id: i64, name: String, _description: String) {
        let Some(library) = &self.library else { return };
        let client = library.client.clone();
        let cached = self
            .playlist
            .as_ref()
            .filter(|p| p.id == id)
            .map(|p| p.description.clone());
        let tx = self.internal.clone();
        let generation = self.list_generation;
        tokio::spawn(async move {
            let description = match cached {
                Some(description) => description,
                None => match client.playlist(id).await {
                    Ok(detail) => detail.playlist.description,
                    Err(e) => {
                        let _ = tx.send(Internal::PlaylistChanged {
                            generation,
                            reload: Some(id),
                            revision: None,
                            appended: Vec::new(),
                            removed: Vec::new(),
                            order: None,
                            exclusive: false,
                            notice: None,
                            result: Err(e),
                        });
                        return;
                    }
                },
            };
            let result = client.update_playlist(id, &name, &description).await;
            let _ = tx.send(Internal::PlaylistChanged {
                generation,
                reload: Some(id),
                revision: None,
                appended: Vec::new(),
                removed: Vec::new(),
                order: None,
                exclusive: false,
                notice: result.as_ref().ok().map(|_| format!("已改名为「{name}」")),
                result: result.map(|_| ()),
            });
        });
    }

    fn delete_playlist(&mut self, id: i64) {
        let Some(library) = &self.library else { return };
        let client = library.client.clone();
        let tx = self.internal.clone();
        let generation = self.list_generation;
        tokio::spawn(async move {
            let result = client.delete_playlist(id).await;
            let _ = tx.send(Internal::PlaylistChanged {
                generation,
                reload: None,
                revision: None,
                appended: Vec::new(),
                removed: Vec::new(),
                order: None,
                exclusive: false,
                notice: result.as_ref().ok().map(|()| "已删除歌单".to_owned()),
                result,
            });
        });
    }

    fn add_to_playlist(&mut self, playlist_id: i64, track_ids: Vec<i64>, album_id: Option<i64>) {
        let Some(library) = &self.library else { return };
        let client = library.client.clone();
        let tx = self.internal.clone();
        let generation = self.list_generation;
        tokio::spawn(async move {
            let result = match album_id {
                Some(album_id) => client.append_playlist_album(playlist_id, album_id).await,
                None => client.append_playlist_items(playlist_id, &track_ids).await,
            };
            let notice = result.as_ref().ok().map(|items| {
                let mut text = format!("已添加 {} 首到歌单", items.added);
                if items.skipped_duplicate > 0 {
                    text.push_str(&format!("，跳过 {} 首重复", items.skipped_duplicate));
                }
                text
            });
            let _ = tx.send(Internal::PlaylistChanged {
                generation,
                reload: Some(playlist_id),
                revision: result.as_ref().ok().map(|items| items.revision),
                appended: Vec::new(),
                removed: Vec::new(),
                order: None,
                exclusive: false,
                notice,
                result: result.map(|_| ()),
            });
        });
    }

    /// Removes tracks and reports the new order, in one `expectedRevision`.
    fn remove_from_playlist(&mut self, playlist_id: i64, track_ids: Vec<i64>) {
        let Some(library) = &self.library else { return };
        let Some(revision) = self.playlist_revision(playlist_id) else {
            self.emit(Event::Notice("歌单还在加载，请稍后重试".into()));
            return;
        };
        if self.playlist_busy() {
            self.emit(Event::Notice("上一次修改还没完成，请稍后重试".into()));
            return;
        }
        if let Some(edit) = self.playlist.as_mut() {
            edit.busy = true;
        }
        let client = library.client.clone();
        let tx = self.internal.clone();
        let generation = self.list_generation;
        tokio::spawn(async move {
            let result = client
                .remove_playlist_items(playlist_id, &track_ids, revision)
                .await;
            let removed = result
                .as_ref()
                .map(|_| track_ids.clone())
                .unwrap_or_default();
            let notice = result
                .as_ref()
                .ok()
                .map(|items| format!("已从歌单移除 {} 首", items.removed));
            let _ = tx.send(Internal::PlaylistChanged {
                generation,
                reload: Some(playlist_id),
                revision: result.as_ref().ok().map(|items| items.revision),
                appended: Vec::new(),
                removed,
                order: None,
                exclusive: true,
                notice,
                result: result.map(|_| ()),
            });
        });
    }

    fn move_playlist_item(&mut self, playlist_id: i64, from: usize, to: usize) {
        let Some(library) = &self.library else { return };
        let Some(edit) = self.playlist.as_ref().filter(|p| p.id == playlist_id) else {
            self.emit(Event::Notice("歌单还在加载，请稍后重试".into()));
            return;
        };
        if edit.busy {
            self.emit(Event::Notice("上一次修改还没完成，请稍后重试".into()));
            return;
        }
        let mut ids = edit.track_ids.clone();
        let revision = edit.revision;
        if from >= ids.len() || to >= ids.len() || from == to {
            return;
        }
        let moved = ids.remove(from);
        ids.insert(to, moved);
        if let Some(edit) = self.playlist.as_mut() {
            edit.busy = true;
        }
        let client = library.client.clone();
        let tx = self.internal.clone();
        let generation = self.list_generation;
        tokio::spawn(async move {
            let result = client
                .reorder_playlist_items(playlist_id, &ids, revision)
                .await;
            let _ = tx.send(Internal::PlaylistChanged {
                generation,
                reload: Some(playlist_id),
                revision: result.as_ref().ok().map(|items| items.revision),
                appended: Vec::new(),
                removed: Vec::new(),
                order: result.is_ok().then(|| ids.clone()),
                exclusive: true,
                notice: None,
                result: result.map(|_| ()),
            });
        });
    }

    fn upload_playlist_artwork(&mut self, playlist_id: i64, path: PathBuf) {
        let Some(library) = &self.library else { return };
        let client = library.client.clone();
        let tx = self.internal.clone();
        let generation = self.list_generation;
        tokio::spawn(async move {
            let result = match std::fs::read(&path) {
                Ok(bytes) => {
                    let name = path.file_name().map_or_else(
                        || "artwork".to_owned(),
                        |n| n.to_string_lossy().into_owned(),
                    );
                    client
                        .upload_playlist_artwork(playlist_id, bytes, &name)
                        .await
                        .map(|artwork| artwork.revision)
                }
                Err(e) => Err(api::ApiError::Decode(format!("读取封面失败：{e}"))),
            };
            let _ = tx.send(Internal::PlaylistChanged {
                generation,
                reload: Some(playlist_id),
                revision: result.as_ref().ok().copied(),
                appended: Vec::new(),
                removed: Vec::new(),
                order: None,
                exclusive: false,
                notice: result.as_ref().ok().map(|_| "封面已更新".to_owned()),
                result: result.map(|_| ()),
            });
        });
    }

    fn delete_playlist_artwork(&mut self, playlist_id: i64) {
        let Some(library) = &self.library else { return };
        let client = library.client.clone();
        let tx = self.internal.clone();
        let generation = self.list_generation;
        tokio::spawn(async move {
            let result = client.delete_playlist_artwork(playlist_id).await;
            let _ = tx.send(Internal::PlaylistChanged {
                generation,
                reload: Some(playlist_id),
                revision: None,
                appended: Vec::new(),
                removed: Vec::new(),
                order: None,
                exclusive: false,
                notice: result.as_ref().ok().map(|()| "封面已删除".to_owned()),
                result,
            });
        });
    }

    /// A write to the open playlist is in flight.
    fn playlist_busy(&self) -> bool {
        self.playlist.as_ref().is_some_and(|p| p.busy)
    }

    /// Revision of the open playlist (`expectedRevision` of item writes).
    fn playlist_revision(&self, playlist_id: i64) -> Option<i64> {
        self.playlist
            .as_ref()
            .filter(|p| p.id == playlist_id)
            .map(|p| p.revision)
    }

    /// Whole-library random play (`/browse/tracks?sort=random` on a browse
    /// server, a sampled window otherwise).
    fn shuffle_all(&mut self) {
        let Some(library) = &self.library else { return };
        let client = library.client.clone();
        let capabilities = library.capabilities.clone();
        let tx = self.internal.clone();
        let generation = self.list_generation;
        tokio::spawn(async move {
            let result = library::random_tracks(&client, &capabilities, 200).await;
            let _ = tx.send(Internal::RandomTracks {
                generation,
                tracks: result,
            });
        });
    }

    // ------------------------------------------------------------ playback

    fn seek(&mut self, position_ms: i64) {
        let position_ms = position_ms.max(0);
        if self.idle
            && let Some((uid, _)) = self.resume_at
        {
            // Restored, not started yet: move where playing will start.
            self.resume_at = Some((uid, position_ms));
            self.position_ms = position_ms;
            self.emit_position();
            self.update_active_line();
            return;
        }
        if self.idle || self.queue.current().is_none() {
            return;
        }
        #[allow(clippy::cast_precision_loss)]
        let seconds = position_ms as f64 / 1000.0;
        if let Err(e) = self.engine.seek(seconds) {
            self.emit(Event::Notice(e));
            return;
        }
        self.position_ms = position_ms;
        if let Some(r) = self.reporter.as_mut() {
            r.act(|a| a.on_seek(seconds));
        }
        self.emit_position();
        self.update_active_line();
    }

    fn adapter_track(&self) -> Option<AdapterTrack> {
        self.queue.current().map(|item| AdapterTrack {
            context: None,
            duration_millis: u64::try_from(item.track.duration_millis).unwrap_or(0),
            track_id: item.track.id,
            unique_id: item.uid.to_string(),
        })
    }

    /// Applies a queue move: play the target, restart it when it is the
    /// entry already playing, or stop at the start of the queue.
    fn step(&mut self, step: Option<Step>, user_navigated: bool) {
        match step {
            None => {}
            Some(Step::Play(pos)) if Some(pos) == self.queue.position() && !self.idle => {
                self.seek(0);
            }
            Some(Step::Play(pos)) if self.stops_at_track_change() => {
                // "End of track" stops at the next change of track, skips included.
                self.stop_playback();
                self.queue.go(pos);
                self.show_current_stopped();
                self.emit_queue_position();
                self.set_sleep_timer(None);
            }
            Some(Step::Play(pos)) => {
                self.queue.go(pos);
                self.play_current(user_navigated);
                self.emit_queue_position();
            }
            Some(Step::Stop(pos)) => {
                self.stop_playback();
                self.queue.go(pos);
                self.show_current_stopped();
                self.emit_queue_position();
            }
        }
    }

    /// Stops the engine and ends the reported play; the queue is untouched.
    fn stop_playback(&mut self) {
        if !self.idle {
            let _ = self.engine.stop();
            if let Some(r) = self.reporter.as_mut() {
                r.act(|a| a.on_status(AdapterStatus::Stopped));
            }
        }
        self.idle = true;
        self.loading = false;
        self.paused = true;
        self.preloaded = None;
        self.expected_path = None;
        self.position_ms = 0;
        self.pending_seek_ms = None;
        self.save_position();
        self.emit(Event::Paused(true));
        self.emit_position();
    }

    /// Shows the current entry (if any) without playing it.
    fn show_current_stopped(&mut self) {
        let adapter_track = self.adapter_track();
        if let Some(r) = self.reporter.as_mut() {
            r.act(|a| a.on_track_change(adapter_track));
        }
        self.started_uid = None;
        let now = self
            .queue
            .current()
            .map(|item| self.now_playing(&item.track));
        self.emit(Event::NowPlaying(now));
        self.emit_position();
        self.lyrics = None;
        self.active_line = None;
        self.emit(Event::Lyrics {
            lines: Vec::new(),
            synced: false,
            source: None,
        });
    }

    fn fetch_album(&mut self, album_id: i64, intent: Enqueue) {
        let Some(library) = &self.library else { return };
        let client = library.client.clone();
        let browse = library.capabilities.has(Feature::Browse);
        let tx = self.internal.clone();
        tokio::spawn(async move {
            let query = api::TrackListQuery {
                album_id: Some(album_id),
                ..api::TrackListQuery::default()
            };
            let result = fetch_all_tracks(&client, browse, &query).await;
            let _ = tx.send(Internal::AlbumTracks(result, intent));
        });
    }

    fn enqueue(&mut self, tracks: Vec<Track>, at: QueuePosition) {
        if tracks.is_empty() {
            return;
        }
        let n = tracks.len();
        match at {
            QueuePosition::Next => self.queue.play_next(tracks),
            QueuePosition::Last => self.queue.add_last(tracks),
        }
        self.queue_changed();
        let what = if n == 1 {
            "1 首".to_owned()
        } else {
            format!("{n} 首")
        };
        self.emit(Event::Notice(match at {
            QueuePosition::Next => format!("已添加 {what}到下一首播放"),
            QueuePosition::Last => format!("已添加 {what}到队列末尾"),
        }));
    }

    fn remove_from_queue(&mut self, uid: u64) {
        let Some(was_current) = self.queue.remove(uid) else {
            return;
        };
        if was_current {
            let playing = !self.idle && !self.paused;
            self.stop_playback();
            if playing && self.queue.current().is_some() {
                self.play_current(true);
            } else {
                self.show_current_stopped();
            }
        }
        self.queue_changed();
    }

    /// The queue's entries or order changed: tell the UI, refresh the preload.
    fn queue_changed(&mut self) {
        let items = self
            .queue
            .in_play_order()
            .map(|item| QueueEntry {
                uid: item.uid,
                title: item.track.title.clone(),
                artist: item.track.artist.clone(),
                duration: format::duration(item.track.duration_millis),
            })
            .collect();
        self.emit(Event::Queue {
            items,
            current: self.queue.position(),
        });
        self.sync_preload();
        self.schedule_queue_save();
    }

    fn emit_queue_position(&mut self) {
        self.emit(Event::QueuePosition(self.queue.position()));
        self.schedule_queue_save();
    }

    fn set_sleep_timer(&mut self, timer: Option<SleepTimer>) {
        self.sleep_generation += 1;
        self.sleep = timer.map(|timer| Sleep {
            timer,
            remaining_ms: match timer {
                SleepTimer::Minutes(m) => i64::from(m) * 60_000,
                _ => 0,
            },
            album_id: self.queue.current().map(|i| i.track.album_id),
        });
        if let Some(SleepTimer::Minutes(_)) = timer {
            let generation = self.sleep_generation;
            let tx = self.internal.clone();
            tokio::spawn(async move {
                let mut tick =
                    tokio::time::interval(Duration::from_millis(SLEEP_TICK_MS.unsigned_abs()));
                tick.tick().await;
                loop {
                    tick.tick().await;
                    if tx.send(Internal::SleepTick(generation)).is_err() {
                        break;
                    }
                }
            });
        }
        self.emit_sleep();
        // Whether the engine may continue into the next file changed.
        self.sync_preload();
    }

    fn emit_sleep(&self) {
        let status = self.sleep.as_ref().map(|s| SleepStatus {
            timer: s.timer,
            remaining_ms: matches!(s.timer, SleepTimer::Minutes(_)).then_some(s.remaining_ms),
        });
        self.emit(Event::SleepTimer(status));
    }

    fn stops_at_track_change(&self) -> bool {
        self.sleep
            .as_ref()
            .is_some_and(|s| s.timer == SleepTimer::EndOfTrack)
    }

    /// Playback stops when the current file ends (sleep timer).
    fn pause_at_end(&self) -> bool {
        let Some(sleep) = &self.sleep else {
            return false;
        };
        match sleep.timer {
            SleepTimer::EndOfTrack => true,
            SleepTimer::EndOfAlbum => {
                let next_album = match self.queue.auto_next(self.settings.repeat) {
                    Some(Step::Play(pos)) => self.queue.at(pos).map(|i| i.track.album_id),
                    _ => None,
                };
                // Last track of the album: the next one (if any) is elsewhere.
                // Repeat-one replays the same track and so never ends the album.
                self.settings.repeat != Repeat::One && next_album != sleep.album_id
            }
            SleepTimer::Minutes(_) => false,
        }
    }

    fn schedule_queue_save(&mut self) {
        self.queue_revision += 1;
        let revision = self.queue_revision;
        let tx = self.internal.clone();
        tokio::spawn(async move {
            tokio::time::sleep(QUEUE_SAVE_DELAY).await;
            let _ = tx.send(Internal::SaveQueue(revision));
        });
    }

    /// `queue.json` path and contents for the signed-in server.
    fn queue_file(&self) -> Option<(PathBuf, Vec<u8>)> {
        let library = self.library.as_ref()?;
        let saved = SavedQueue {
            server: library.client.base_url().to_owned(),
            queue: self.queue.snapshot(),
        };
        let bytes = serde_json::to_vec(&saved).ok()?;
        Some((self.config.data_dir.join(QUEUE_FILE), bytes))
    }

    fn save_position(&mut self) {
        if self.library.is_some() {
            self.session_file
                .set(POSITION_KEY, &self.position_ms.max(0).to_string());
        }
    }

    /// Brings back the queue saved for this server, stopped at the saved
    /// position; playing it resumes the persisted reporting chain.
    fn restore_queue(&mut self) {
        let Some(library) = &self.library else { return };
        let path = self.config.data_dir.join(QUEUE_FILE);
        let Some(saved) = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<SavedQueue>(&b).ok())
        else {
            return;
        };
        if saved.server != library.client.base_url() || saved.queue.tracks.is_empty() {
            return;
        }
        self.queue.restore(saved.queue);
        self.queue_changed();
        let Some(item) = self.queue.current().cloned() else {
            return;
        };
        let position_ms = self
            .session_file
            .get(POSITION_KEY)
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0)
            .clamp(0, item.track.duration_millis.max(0));
        self.resume_at = Some((item.uid, position_ms));
        self.position_ms = position_ms;
        let adapter_track = self.adapter_track();
        if let Some(r) = self.reporter.as_mut() {
            r.act(|a| {
                a.on_queue_restored();
                a.on_track_change(adapter_track);
            });
            #[allow(clippy::cast_precision_loss)]
            r.act(|a| a.on_progress(position_ms as f64 / 1000.0));
        }
        self.emit(Event::NowPlaying(Some(self.now_playing(&item.track))));
        self.emit(Event::Paused(true));
        self.emit_position();
        self.load_lyrics(item.track.id);
    }

    /// Keeps the engine's next file equal to the queue's automatic next
    /// entry, so the engine continues into it without a gap.
    fn sync_preload(&mut self) {
        if self.idle || self.loading {
            // Done once the current file has loaded.
            return;
        }
        let Some(library) = &self.library else { return };
        let next = match self.queue.auto_next(self.settings.repeat) {
            // The sleep timer stops after this file: the engine must not go on.
            _ if self.pause_at_end() => None,
            Some(Step::Play(pos)) => self.queue.at(pos),
            _ => None,
        };
        let want = next.map(|item| (item.uid, library.client.stream_url(item.track.id)));
        if want.as_ref().map(|(uid, _)| *uid) == self.preloaded {
            return;
        }
        self.preloaded = None;
        if let Err(e) = self.engine.clear_upcoming() {
            tracing::warn!("clearing the preloaded file: {e}");
            return;
        }
        if let Some((uid, url)) = want {
            match self.engine.append(url.as_str()) {
                Ok(()) => self.preloaded = Some(uid),
                Err(e) => tracing::warn!("preloading the next file: {e}"),
            }
        }
    }

    /// Loads and starts the current queue entry.
    fn play_current(&mut self, user_navigated: bool) {
        self.start_current(user_navigated, true);
    }

    /// Starts the current queue entry: `load` it, or (gapless) it is the
    /// preloaded file the engine has just moved on to.
    fn start_current(&mut self, user_navigated: bool, load: bool) {
        let Some(item) = self.queue.current().cloned() else {
            return;
        };
        let Some(library) = &self.library else { return };
        let url = library.client.stream_url(item.track.id);
        if load
            && let Err(e) = self
                .engine
                .load(url.as_str())
                .and_then(|()| self.engine.set_paused(false))
        {
            self.emit(Event::Notice(e));
            return;
        }
        self.idle = false;
        self.loading = true;
        self.paused = false;
        self.preloaded = None;
        self.expected_path = Some(url.to_string());
        // A restored entry continues where it was left.
        let resume_ms = match self.resume_at.take() {
            Some((uid, ms)) if uid == item.uid && ms > 0 && load => Some(ms),
            _ => None,
        };
        self.pending_seek_ms = resume_ms;
        self.position_ms = resume_ms.unwrap_or(0);
        self.reported_step = self.position_ms / POSITION_STEP_MS;
        self.engine_duration_ms = 0;
        let position_ms = self.position_ms;

        if let Some(sleep) = &self.sleep
            && sleep.timer == SleepTimer::EndOfAlbum
            && sleep.album_id != Some(item.track.album_id)
        {
            // Moved on to another album: the timer no longer applies.
            self.set_sleep_timer(None);
        }

        // The same entry again without a user action: a repeat-one pass.
        let repeated = !user_navigated && self.started_uid == Some(item.uid);
        self.started_uid = Some(item.uid);
        let adapter_track = self.adapter_track();
        if let Some(r) = self.reporter.as_mut() {
            r.act(|a| {
                if repeated {
                    a.on_repeated();
                }
                #[allow(clippy::cast_precision_loss)]
                a.on_progress(position_ms as f64 / 1000.0);
                a.on_track_change(adapter_track);
                if user_navigated {
                    a.on_user_navigate();
                }
                a.on_status(AdapterStatus::Playing);
            });
        }

        let track = &item.track;
        self.emit(Event::NowPlaying(Some(self.now_playing(track))));
        self.emit(Event::Paused(false));
        self.emit_position();
        self.save_position();
        self.load_lyrics(track.id);
    }

    /// Clears the lyrics and fetches those of `track_id`.
    fn load_lyrics(&mut self, track_id: i64) {
        let Some(library) = &self.library else { return };
        let client = library.client.clone();
        self.lyrics = None;
        self.active_line = None;
        self.emit(Event::Lyrics {
            lines: Vec::new(),
            synced: false,
            source: None,
        });
        let tx = self.internal.clone();
        tokio::spawn(async move {
            let lyrics = match client.lyrics(track_id).await {
                Ok(lyrics) => lyrics,
                Err(e) => {
                    tracing::debug!("lyrics {track_id}: {e}");
                    if let Some(msg) = unauthorized(&client, &e) {
                        let _ = tx.send(msg);
                    }
                    None
                }
            };
            let _ = tx.send(Internal::Lyrics { track_id, lyrics });
        });
    }

    fn duration_ms(&self) -> i64 {
        self.queue
            .current()
            .map(|i| i.track.duration_millis)
            .filter(|d| *d > 0)
            .unwrap_or(self.engine_duration_ms)
    }

    fn emit_position(&self) {
        self.emit(Event::Position {
            position_ms: self.position_ms,
            duration_ms: self.duration_ms(),
        });
    }

    fn update_active_line(&mut self) {
        let active = self
            .lyrics
            .as_ref()
            .and_then(|l| l.active_index(self.position_ms, 0));
        if active != self.active_line {
            self.active_line = active;
            self.emit(Event::LyricsActive(active));
        }
    }

    fn engine_event(&mut self, event: EngineEvent) {
        match event {
            EngineEvent::TimePos(seconds) => {
                if self.idle || self.loading {
                    return;
                }
                #[allow(clippy::cast_possible_truncation)]
                let ms = (seconds * 1000.0).round() as i64;
                self.position_ms = ms.max(0);
                let save_step = self.position_ms / POSITION_SAVE_STEP_MS;
                if save_step != self.saved_position_step {
                    self.saved_position_step = save_step;
                    self.save_position();
                }
                let step = self.position_ms / POSITION_STEP_MS;
                if step != self.reported_step {
                    self.reported_step = step;
                    if let Some(r) = self.reporter.as_mut() {
                        r.act(|a| a.on_progress(seconds));
                    }
                    self.emit_position();
                }
                self.update_active_line();
            }
            EngineEvent::Duration(seconds) => {
                if self.loading {
                    return;
                }
                #[allow(clippy::cast_possible_truncation)]
                let ms = (seconds * 1000.0).round() as i64;
                self.engine_duration_ms = ms;
                self.emit_position();
            }
            EngineEvent::Paused(paused) => {
                if self.idle || paused == self.paused {
                    return;
                }
                self.paused = paused;
                if let Some(r) = self.reporter.as_mut() {
                    let status = if paused {
                        AdapterStatus::Paused
                    } else {
                        AdapterStatus::Playing
                    };
                    r.act(|a| a.on_status(status));
                }
                if paused {
                    self.save_position();
                }
                self.emit(Event::Paused(paused));
            }
            EngineEvent::Buffering(buffering) => {
                if let Some(r) = self.reporter.as_mut() {
                    r.act(|a| a.on_buffering(buffering));
                }
                self.emit(Event::Buffering(buffering));
            }
            EngineEvent::Volume(volume) => {
                self.update_settings(|s| s.volume = volume.clamp(0.0, 100.0));
                self.emit(Event::Volume(volume));
            }
            EngineEvent::FileLoaded { path } => {
                if !self.loading {
                    return;
                }
                if let (Some(path), Some(expected)) = (&path, &self.expected_path)
                    && path != expected
                {
                    // The engine moved on to a stale preload (the queue
                    // changed right at the track boundary): load the right one.
                    tracing::debug!(%path, %expected, "unexpected file, reloading");
                    if let Err(e) = self.engine.load(expected) {
                        self.emit(Event::Notice(e));
                    }
                    return;
                }
                self.loading = false;
                if let Some(ms) = self.pending_seek_ms.take() {
                    #[allow(clippy::cast_precision_loss)]
                    if let Err(e) = self.engine.seek(ms as f64 / 1000.0) {
                        tracing::warn!("seeking to the resumed position: {e}");
                    }
                }
                self.sync_preload();
            }
            EngineEvent::EndFile { reason, error } => match reason {
                EngineEnd::Stop => {}
                EngineEnd::Eof => {
                    if self.idle || self.loading {
                        return;
                    }
                    match self.queue.auto_next(self.settings.repeat) {
                        Some(Step::Play(pos)) if self.pause_at_end() => {
                            // Sleep timer: on to the next entry, stopped.
                            self.stop_playback();
                            self.queue.go(pos);
                            self.show_current_stopped();
                            self.emit_queue_position();
                            self.set_sleep_timer(None);
                        }
                        Some(Step::Play(pos)) => {
                            let gapless = self.preloaded.is_some()
                                && self.queue.at(pos).map(|i| i.uid) == self.preloaded;
                            self.queue.go(pos);
                            // Gapless: the engine already moved on to the preload.
                            self.start_current(false, !gapless);
                            self.emit_queue_position();
                        }
                        step @ Some(Step::Stop(_)) => self.step(step, false),
                        None => {}
                    }
                }
                EngineEnd::Error => {
                    if self.idle {
                        return;
                    }
                    // Keeps the engine from moving on into the preload.
                    let _ = self.engine.stop();
                    self.preloaded = None;
                    self.idle = true;
                    self.loading = false;
                    self.paused = true;
                    if let Some(r) = self.reporter.as_mut() {
                        r.act(|a| {
                            a.on_error();
                            a.on_status(AdapterStatus::Paused);
                        });
                    }
                    self.emit(Event::Paused(true));
                    self.emit(Event::Notice(format!(
                        "播放失败：{}",
                        error.unwrap_or_else(|| "未知错误".into())
                    )));
                }
            },
        }
    }

    // ------------------------------------------------------------ tasks

    fn internal(&mut self, msg: Internal) {
        match msg {
            Internal::Reporter(generation, msg) => {
                if generation != self.list_generation {
                    return;
                }
                if let ReporterMsg::Unauthorized(api_token) = msg {
                    self.internal(Internal::Unauthorized { api_token });
                } else if let Some(r) = self.reporter.as_mut() {
                    r.handle(msg);
                }
            }
            Internal::LoginFailed(failure) => self.emit(Event::LoginFailed(failure)),
            Internal::LoggedIn(ok) => {
                let (client, capabilities) = *ok;
                if self.library.is_none() {
                    self.logged_in(client, capabilities);
                }
            }
            Internal::Unauthorized { api_token } => {
                if self
                    .library
                    .as_ref()
                    .is_some_and(|l| l.client.api_token() == api_token)
                {
                    self.sign_out(SignOutReason::Expired);
                }
            }
            Internal::ViewPage {
                generation,
                revision,
                view,
                query,
                page,
            } => self.view_page(generation, revision, view, *query, *page),
            Internal::RandomTracks {
                generation,
                tracks: result,
            } => match result {
                _ if generation != self.list_generation => {}
                Ok(tracks) if !tracks.is_empty() => {
                    if self.queue.replace(tracks, 0).is_some() {
                        self.play_current(true);
                    }
                    self.queue_changed();
                }
                Ok(_) => self.emit(Event::Notice("曲库是空的".into())),
                Err(e) => self.notice_for("随机播放失败", &e),
            },
            Internal::PlaylistChanged {
                generation,
                reload,
                revision,
                appended,
                removed,
                order,
                exclusive,
                notice,
                result,
            } => self.playlist_write(PlaylistChanged {
                generation,
                reload,
                revision,
                appended,
                removed,
                order,
                exclusive,
                notice,
                result,
            }),
            Internal::Playlists { generation, items } => {
                if generation == self.list_generation {
                    match items {
                        Ok(items) => {
                            self.playlists.clone_from(&items);
                            self.emit(Event::Playlists(items));
                        }
                        Err(e) => self.notice_for("加载歌单失败", &e),
                    }
                }
            }
            Internal::FavoriteSet {
                generation,
                kind,
                id,
                favorite,
                result,
            } => match result {
                _ if generation != self.list_generation => {}
                Ok(()) => {
                    self.apply_favorite(kind, id, favorite);
                    self.emit(Event::Favorite { kind, id, favorite });
                }
                Err(e) => self.notice_for("收藏失败", &e),
            },
            Internal::AlbumTracks(Ok(tracks), Enqueue::Play) => {
                if self.queue.replace(tracks, 0).is_some() {
                    self.play_current(true);
                }
                self.queue_changed();
            }
            Internal::AlbumTracks(Ok(tracks), Enqueue::At(at)) => self.enqueue(tracks, at),
            Internal::AlbumTracks(Err(e), _) => {
                if e.is_unauthorized() {
                    self.sign_out(SignOutReason::Expired);
                } else {
                    self.emit(Event::Notice(format!("加载专辑失败：{e}")));
                }
            }
            Internal::Lyrics { track_id, lyrics } => {
                if self.queue.current().map(|i| i.track.id) != Some(track_id) {
                    return;
                }
                let Some(lyrics) = lyrics.filter(|l| !l.lines.is_empty()) else {
                    return;
                };
                let lines = lyrics.lines.iter().map(|l| l.text.clone()).collect();
                let source = lyrics.source_name.clone().or(lyrics.source.clone());
                self.emit(Event::Lyrics {
                    lines,
                    synced: lyrics.synced,
                    source,
                });
                if lyrics.synced {
                    self.lyrics = Some(lyrics::SyncedLyrics::new(
                        lyrics
                            .lines
                            .into_iter()
                            .map(|l| lyrics::Line {
                                time_ms: l.time_ms,
                                text: l.text,
                            })
                            .collect(),
                    ));
                    self.active_line = None;
                    self.update_active_line();
                }
            }
            Internal::AudioDeviceSet(device, result) => {
                self.output_busy = false;
                match result {
                    Ok(()) => self.update_settings(|s| s.audio_device.clone_from(&device)),
                    Err(e) => self.emit(Event::Notice(format!("切换输出设备失败：{e}"))),
                }
                self.emit(Event::AudioDevice(self.settings.audio_device.clone()));
            }
            Internal::ExclusiveSet(enabled, result) => {
                self.output_busy = false;
                match result {
                    Ok(()) => self.update_settings(|s| s.exclusive = enabled),
                    Err(e) => self.emit(Event::Notice(format!("切换独占模式失败：{e}"))),
                }
                self.emit(Event::Exclusive(self.settings.exclusive));
            }
            Internal::SleepTick(generation) => {
                if generation != self.sleep_generation {
                    return;
                }
                let playing = !self.idle && !self.paused;
                let Some(sleep) = self.sleep.as_mut() else {
                    return;
                };
                if !playing {
                    return;
                }
                sleep.remaining_ms -= SLEEP_TICK_MS;
                if sleep.remaining_ms > 0 {
                    self.emit_sleep();
                    return;
                }
                self.set_sleep_timer(None);
                if let Err(e) = self.engine.set_paused(true) {
                    self.emit(Event::Notice(e));
                }
            }
            Internal::SaveQueue(revision) => {
                if revision != self.queue_revision {
                    return;
                }
                if let Some((path, bytes)) = self.queue_file() {
                    self.saved_queue_revision = revision;
                    tokio::task::spawn_blocking(move || write_atomic(&path, &bytes));
                }
            }
            Internal::SaveSettings(revision) => {
                if revision == self.settings_revision {
                    self.save_settings();
                }
            }
        }
    }

    /// Stops playback, drops the signed-in state and forgets the rejected
    /// (expired) or all (user) saved tokens.
    fn sign_out(&mut self, reason: SignOutReason) {
        let Some(library) = self.library.take() else {
            return;
        };
        self.list_generation += 1;
        if let Some(mut reporter) = self.reporter.take()
            && !self.idle
        {
            // Ends the server-side session; with an expired token this
            // is rejected and the server times the session out.
            reporter.act(|a| a.on_status(AdapterStatus::Stopped));
        }
        let _ = self.engine.stop();
        self.queue.clear();
        self.idle = true;
        self.loading = false;
        self.paused = true;
        self.preloaded = None;
        self.expected_path = None;
        self.started_uid = None;
        self.resume_at = None;
        self.pending_seek_ms = None;
        self.sleep = None;
        self.sleep_generation += 1;
        self.position_ms = 0;
        self.engine_duration_ms = 0;
        self.lyrics = None;
        self.active_line = None;

        let server = library.client.base_url().to_owned();
        let secrets = self.config.secrets.clone();
        let has_media_token = reason == SignOutReason::Expired;
        {
            let server = server.clone();
            tokio::spawn(async move {
                write_token(&secrets, &server, TokenKind::Api, None).await;
                if !has_media_token {
                    write_token(&secrets, &server, TokenKind::Media, None).await;
                }
            });
        }

        self.emit(Event::NowPlaying(None));
        self.emit(Event::Paused(true));
        self.emit_position();
        self.emit(Event::Lyrics {
            lines: Vec::new(),
            synced: false,
            source: None,
        });
        self.view.reset(View::Albums);
        // Nothing of the old server may survive: a late answer of its tasks
        // is dropped by the generation, but the cached state is cleared too.
        self.playlist = None;
        self.playlists.clear();
        self.seeds = Seeds::default();
        self.emit(Event::Playlists(Vec::new()));
        self.emit(Event::ViewReset {
            view: self.view.view.clone(),
            header: self.view.header.clone(),
        });
        self.emit(Event::Queue {
            items: Vec::new(),
            current: None,
        });
        self.emit(Event::SleepTimer(None));
        self.emit(Event::SignedOut(reason));
        self.emit(Event::SavedLogin(SavedLogin {
            server_url: server,
            has_api_token: false,
            has_media_token,
            signing_in: false,
        }));
    }

    fn logged_in(&mut self, client: Client, capabilities: Capabilities) {
        self.session_file.set(SERVER_KEY, client.base_url());
        self.list_generation += 1;
        let generation = self.list_generation;
        let storage = FileStorage::open(self.config.data_dir.join("playback.json"));
        let reporter_tx = reporter_channel(&self.internal, generation);
        self.reporter = Some(Reporter::new(
            client.clone(),
            &capabilities,
            Box::new(storage),
            reporter_tx,
        ));
        self.library = Some(Library {
            images: Arc::new(Images::new(client.clone())),
            client: client.clone(),
            capabilities,
        });
        self.emit(Event::LoggedIn {
            server_url: client.base_url().to_owned(),
        });
        self.restore_queue();
        // The sidebar shows the album grid after signing in.
        self.navigate(View::Albums);
        self.refresh_playlists();
    }
}

/// Every page of a track list.
async fn fetch_all_tracks(
    client: &Client,
    browse: bool,
    query: &api::TrackListQuery,
) -> Result<Vec<Track>, api::ApiError> {
    let mut out = Vec::new();
    loop {
        let offset = u32::try_from(out.len()).unwrap_or(u32::MAX);
        let page = client.tracks(browse, query, offset, library::PAGE).await?;
        let n = page.items.len();
        out.extend(page.items);
        if n == 0 || i64::try_from(out.len()).unwrap_or(i64::MAX) >= page.total {
            return Ok(out);
        }
    }
}

/// Writes via a temp file and rename, so a crash never leaves half a file.
fn write_atomic(path: &std::path::Path, bytes: &[u8]) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("tmp");
    let result = std::fs::write(&tmp, bytes).and_then(|()| std::fs::rename(&tmp, path));
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        tracing::warn!("writing {}: {e}", path.display());
    }
}

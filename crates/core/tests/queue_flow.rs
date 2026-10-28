//! Queue editing, repeat modes and gapless preloading against a fake engine.

use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use client_core::{
    AudioDevice, Command, CoreConfig, CoreHandle, Engine, EngineEnd, EngineEvent, Event,
    MemorySecrets, QueuePosition, Repeat, Settings, SleepTimer, TrackSource, View,
};
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[derive(Default)]
struct FakeEngine {
    calls: Mutex<Vec<String>>,
}

impl FakeEngine {
    /// Calls since the last `take`, with the server URL stripped.
    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.calls.lock().unwrap())
    }

    fn push(&self, call: String) -> Result<(), String> {
        self.calls.lock().unwrap().push(call);
        Ok(())
    }
}

/// `http://127.0.0.1:1234/api/v1/tracks/2/stream?mediaToken=media` -> `2`.
fn short(url: &str) -> String {
    url.split("/tracks/")
        .nth(1)
        .and_then(|s| s.split('/').next())
        .unwrap_or(url)
        .to_owned()
}

impl Engine for FakeEngine {
    fn load(&self, url: &str) -> Result<(), String> {
        self.push(format!("load {}", short(url)))
    }
    fn append(&self, url: &str) -> Result<(), String> {
        self.push(format!("append {}", short(url)))
    }
    fn clear_upcoming(&self) -> Result<(), String> {
        self.push("clear".into())
    }
    fn stop(&self) -> Result<(), String> {
        self.push("stop".into())
    }
    fn set_paused(&self, _: bool) -> Result<(), String> {
        Ok(())
    }
    fn seek(&self, seconds: f64) -> Result<(), String> {
        self.push(format!("seek {seconds}"))
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

fn track(id: i64) -> Value {
    json!({
        "id": id, "title": format!("曲{id}"), "artist": "歌手", "album": "专辑",
        "albumId": if id <= 2 { 10 } else { 20 },
        "durationMillis": 240000, "streamUrl": format!("/api/v1/tracks/{id}/stream"),
    })
}

async fn server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(path("/api/v1/capabilities"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "apiVersion": "v1", "apiRevision": 3, "features": { "browse": true },
        })))
        .mount(&server)
        .await;
    // Empty for the media token probe; the library comes from /browse.
    Mock::given(path("/api/v1/tracks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [], "limit": 1, "offset": 0, "total": 0
        })))
        .mount(&server)
        .await;
    Mock::given(path("/api/v1/browse/albums"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [], "limit": 500, "offset": 0, "total": 0
        })))
        .mount(&server)
        .await;
    Mock::given(path("/api/v1/browse/tracks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [track(1), track(2), track(3)], "limit": 500, "offset": 0, "total": 3
        })))
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
    server
}

struct Harness {
    core: Option<CoreHandle>,
    events: mpsc::Receiver<Event>,
    engine: Arc<FakeEngine>,
    base: String,
}

impl Harness {
    async fn start(server: &MockServer) -> Self {
        // Tests run in parallel: one directory each.
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let data_dir =
            std::env::temp_dir().join(format!("032player-queue-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data_dir);
        let h = Self::start_in(server, data_dir).await;
        h.wait_tracks();
        h
    }

    /// Switches to the track list and waits for it.
    fn wait_tracks(&self) {
        self.send(Command::Navigate(View::Tracks));
        self.wait("tracks", |e| {
            matches!(
                e,
                Event::ViewRows {
                    view: View::Tracks,
                    total: 3,
                    ..
                }
            )
        });
    }

    /// Starts in `data_dir` as is (a restart keeps it).
    async fn start_in(server: &MockServer, data_dir: std::path::PathBuf) -> Self {
        let engine = Arc::new(FakeEngine::default());
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let config = CoreConfig {
            settings: Settings::load(&data_dir),
            data_dir,
            secrets: Arc::new(MemorySecrets::default()),
        };
        let core = client_core::start(engine.clone(), config, move |e| {
            let _ = tx.lock().unwrap().send(e);
        });
        let h = Self {
            core: Some(core),
            events: rx,
            engine,
            base: server.uri(),
        };
        h.send(Command::Login {
            server_url: server.uri(),
            api_token: "api".into(),
            media_token: "media".into(),
        });
        h.wait("login", |e| matches!(e, Event::LoggedIn { .. }));
        h
    }

    fn send(&self, command: Command) {
        self.core.as_ref().unwrap().send(command);
    }

    fn engine(&self, event: EngineEvent) {
        self.core.as_ref().unwrap().engine_event(event);
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

    fn wait_playing(&self, id: i64) {
        self.wait(
            &format!("now playing {id}"),
            |e| matches!(e, Event::NowPlaying(Some(n)) if n.track_id == id),
        );
    }

    /// The queue as track titles, from the next `Event::Queue`.
    fn wait_queue(&self) -> (Vec<String>, Option<usize>) {
        match self.wait("queue", |e| matches!(e, Event::Queue { .. })) {
            Event::Queue { items, current } => {
                (items.into_iter().map(|i| i.title).collect(), current)
            }
            _ => unreachable!(),
        }
    }

    fn loaded(&self, id: i64) {
        self.engine(EngineEvent::FileLoaded {
            path: Some(format!(
                "{}/api/v1/tracks/{id}/stream?mediaToken=media",
                self.base
            )),
        });
    }

    fn eof(&self) {
        self.engine(EngineEvent::EndFile {
            reason: EngineEnd::Eof,
            error: None,
        });
    }

    /// Lets the core drain its inbox, then returns the engine calls.
    async fn calls(&self) -> Vec<String> {
        tokio::time::sleep(Duration::from_millis(150)).await;
        self.engine.take()
    }

    async fn shutdown(mut self) {
        let core = self.core.take().unwrap();
        tokio::task::spawn_blocking(move || core.shutdown())
            .await
            .unwrap();
    }
}

fn titles(ids: &[i64]) -> Vec<String> {
    ids.iter().map(|id| format!("曲{id}")).collect()
}

async fn playback_bodies(server: &MockServer) -> Vec<Value> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == "/api/v1/playback/events")
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .collect()
}

async fn playback_events(server: &MockServer) -> Vec<String> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == "/api/v1/playback/events")
        .map(|r| {
            let e: Value = serde_json::from_slice(&r.body).unwrap();
            format!("{}:{}", e["type"].as_str().unwrap(), e["trackId"])
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn gapless_queue_editing_and_repeat() {
    let server = server().await;
    let h = Harness::start(&server).await;

    h.send(Command::PlayRow(0));
    h.wait_playing(1);
    assert_eq!(h.wait_queue(), (titles(&[1, 2, 3]), Some(0)));
    // The next file is preloaded once the current one has loaded.
    assert_eq!(h.calls().await, ["load 1"]);
    h.loaded(1);
    assert_eq!(h.calls().await, ["clear", "append 2"]);

    // Gapless: the engine moved on by itself, nothing is loaded.
    h.eof();
    h.wait_playing(2);
    h.loaded(2);
    assert_eq!(h.calls().await, ["clear", "append 3"]);

    // "Play next" changes what comes next: the preload follows.
    h.send(Command::Enqueue {
        source: TrackSource::Row(0),
        at: QueuePosition::Next,
    });
    assert_eq!(h.wait_queue(), (titles(&[1, 2, 1, 3]), Some(1)));
    assert_eq!(h.calls().await, ["clear", "append 1"]);

    // Repeat-one preloads the same entry and counts each pass.
    h.send(Command::SetRepeat(Repeat::One));
    h.wait("repeat", |e| *e == Event::Repeat(Repeat::One));
    assert_eq!(h.calls().await, ["clear", "append 2"]);
    h.engine(EngineEvent::TimePos(239.0));
    h.eof();
    h.wait_playing(2);
    // The engine reports a stale preload: the right file is loaded instead.
    h.send(Command::SetRepeat(Repeat::Off));
    h.loaded(3);
    assert_eq!(h.calls().await, ["load 2"]);
    h.loaded(2);
    assert_eq!(h.calls().await, ["clear", "append 1"]);

    // Removing the playing entry plays the one that took its place
    // (uids: 1-3 for tracks 1-3, 4 for the queued copy of track 1).
    h.send(Command::RemoveFromQueue(2));
    h.wait_playing(1);
    assert_eq!(h.wait_queue(), (titles(&[1, 1, 3]), Some(1)));
    assert_eq!(h.calls().await, ["stop", "load 1"]);

    // Past the end without repeat: back to the first entry, stopped.
    h.send(Command::PlayQueueItem(3));
    h.wait_playing(3);
    h.loaded(3);
    // Nothing after the last entry; `load` already dropped the old preload.
    assert_eq!(h.calls().await, ["load 3"]);
    h.eof();
    h.wait("stopped", |e| *e == Event::Paused(true));
    h.wait_playing(1);
    assert_eq!(h.calls().await, ["stop"]);

    h.send(Command::ClearQueue);
    h.wait("cleared", |e| *e == Event::NowPlaying(None));
    assert_eq!(h.wait_queue(), (Vec::new(), None));

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut sent = playback_events(&server).await;
    while sent.len() < 5 && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
        sent = playback_events(&server).await;
    }
    // start 1, end 1 (completed), start 2, end 2 (repeat pass), start 2, ...
    assert_eq!(
        &sent[..5],
        ["start:1", "end:1", "start:2", "end:2", "start:2"]
    );
    h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn restart_restores_the_queue_and_resumes_the_chain() {
    let server = server().await;
    let dir = std::env::temp_dir().join(format!("032player-restore-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    let h = Harness::start_in(&server, dir.clone()).await;
    h.wait_tracks();
    h.send(Command::SetShuffle(true));
    h.wait("shuffle", |e| *e == Event::Shuffle(true));
    assert_eq!(h.wait_queue(), (Vec::new(), None));
    h.send(Command::PlayRow(1));
    h.wait_playing(2);
    let (order, current) = h.wait_queue();
    assert_eq!((order[0].as_str(), current), ("曲2", Some(0)));
    h.loaded(2);
    h.engine(EngineEvent::TimePos(30.0));
    h.engine(EngineEvent::Paused(true));
    h.wait("paused", |e| *e == Event::Paused(true));
    // Events go out one at a time: once the pause arrived, the start was
    // acknowledged, which is what makes the chain resumable.
    let deadline = Instant::now() + Duration::from_secs(5);
    while !playback_events(&server)
        .await
        .contains(&"pause:2".to_owned())
    {
        assert!(Instant::now() < deadline, "pause not reported");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    h.engine.take();
    h.shutdown().await;
    let first = playback_bodies(&server).await;
    let session_id = first[0]["sessionId"].as_str().unwrap().to_owned();
    assert_eq!(first.last().unwrap()["endReason"], "client_closed");

    // Restart: same queue (same shuffled order), stopped at 0:30.
    let h = Harness::start_in(&server, dir.clone()).await;
    assert_eq!(h.wait_queue(), (order, Some(0)));
    h.wait_playing(2);
    h.wait("position", |e| {
        matches!(
            e,
            Event::Position {
                position_ms: 30_000,
                ..
            }
        )
    });
    assert!(h.calls().await.is_empty(), "restoring does not play");

    // Play continues at the saved position and resumes the reported chain.
    h.send(Command::TogglePause);
    h.wait("playing", |e| *e == Event::Paused(false));
    assert_eq!(h.calls().await, ["load 2"]);
    h.loaded(2);
    let calls = h.calls().await;
    assert_eq!(calls[0], "seek 30");
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut all = playback_bodies(&server).await;
    while all.len() <= first.len() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
        all = playback_bodies(&server).await;
    }
    let resumed = &all[first.len()];

    assert_eq!(resumed["type"], "start");
    assert_eq!(resumed["trackId"], 2);
    assert_eq!(resumed["resumedFromSessionId"], session_id.as_str());
    assert_eq!(resumed["positionMillis"], 30_000);
    h.shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn sleep_timer_stops_at_the_end_of_the_track_or_album() {
    let server = server().await;
    let h = Harness::start(&server).await;

    h.send(Command::PlayRow(0));
    h.wait_playing(1);
    h.loaded(1);
    assert_eq!(h.calls().await, ["load 1", "clear", "append 2"]);

    // End of track: the preload goes, so the engine stops after this file.
    h.send(Command::SetSleepTimer(Some(SleepTimer::EndOfTrack)));
    h.wait("timer", |e| matches!(e, Event::SleepTimer(Some(_))));
    assert_eq!(h.calls().await, ["clear"]);
    h.eof();
    h.wait("stopped", |e| *e == Event::Paused(true));
    h.wait_playing(2);
    h.wait("timer done", |e| *e == Event::SleepTimer(None));
    assert_eq!(h.calls().await, ["stop"]);

    // End of album: track 2 is the last of album 10, track 3 is album 20.
    h.send(Command::TogglePause);
    h.wait("playing", |e| *e == Event::Paused(false));
    h.loaded(2);
    assert_eq!(h.calls().await, ["load 2", "clear", "append 3"]);
    h.send(Command::SetSleepTimer(Some(SleepTimer::EndOfAlbum)));
    h.wait("timer", |e| matches!(e, Event::SleepTimer(Some(_))));
    assert_eq!(h.calls().await, ["clear"]);
    h.eof();
    h.wait("stopped", |e| *e == Event::Paused(true));
    h.wait_playing(3);
    h.wait("timer done", |e| *e == Event::SleepTimer(None));

    // Cancelling brings the preload back.
    h.send(Command::TogglePause);
    h.wait("playing", |e| *e == Event::Paused(false));
    h.loaded(3);
    h.calls().await;
    h.send(Command::SetSleepTimer(Some(SleepTimer::Minutes(30))));
    h.wait(
        "timed",
        |e| matches!(e, Event::SleepTimer(Some(s)) if s.remaining_ms == Some(1_800_000)),
    );
    h.send(Command::SetSleepTimer(None));
    h.wait("cancelled", |e| *e == Event::SleepTimer(None));
    h.shutdown().await;
}

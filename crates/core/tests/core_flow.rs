//! Core end to end against a mock server and a fake engine.

use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use client_core::{
    AudioDevice, Command, CoreConfig, Engine, EngineEnd, EngineEvent, Event, LoginFailure,
    MemorySecrets, Settings, View,
};
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[derive(Default)]
struct FakeEngine {
    calls: Mutex<Vec<String>>,
}

impl FakeEngine {
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn push(&self, call: String) -> Result<(), String> {
        self.calls.lock().unwrap().push(call);
        Ok(())
    }
}

impl Engine for FakeEngine {
    fn load(&self, url: &str) -> Result<(), String> {
        self.push(format!("load {url}"))
    }
    fn append(&self, url: &str) -> Result<(), String> {
        self.push(format!("append {url}"))
    }
    fn clear_upcoming(&self) -> Result<(), String> {
        self.push("clear_upcoming".into())
    }
    fn stop(&self) -> Result<(), String> {
        self.push("stop".into())
    }
    fn set_paused(&self, paused: bool) -> Result<(), String> {
        self.push(format!("pause {paused}"))
    }
    fn seek(&self, seconds: f64) -> Result<(), String> {
        self.push(format!("seek {seconds}"))
    }
    fn set_volume(&self, volume: f64) -> Result<(), String> {
        self.push(format!("volume {volume}"))
    }
    fn audio_devices(&self) -> Result<Vec<AudioDevice>, String> {
        Ok(Vec::new())
    }
    fn set_audio_device(&self, name: &str) -> Result<(), String> {
        self.push(format!("device {name}"))
    }
    fn set_exclusive(&self, enabled: bool) -> Result<(), String> {
        self.push(format!("exclusive {enabled}"))
    }
}

fn track(id: i64) -> Value {
    json!({
        "id": id, "title": format!("曲{id}"), "artist": "歌手", "album": "专辑",
        "durationMillis": 240000, "streamUrl": format!("/api/v1/tracks/{id}/stream"),
        "codec": "flac", "bitDepth": 16, "sampleRate": 44100,
    })
}

async fn server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(path("/api/v1/capabilities"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "apiVersion": "v1", "apiRevision": 3,
            "features": { "browse": true },
            "playback": { "clientKinds": ["web", "desktop"] },
        })))
        .mount(&server)
        .await;
    Mock::given(path("/api/v1/tracks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [track(1)], "limit": 1, "offset": 0, "total": 2
        })))
        .mount(&server)
        .await;
    Mock::given(path("/api/v1/tracks/1/stream"))
        .respond_with(ResponseTemplate::new(206))
        .mount(&server)
        .await;
    Mock::given(path("/api/v1/browse/albums"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [{ "id": 9, "title": "A", "artist": "B", "artworkUrl": "/api/v1/artwork/1" }],
            "limit": 500, "offset": 0, "total": 1
        })))
        .mount(&server)
        .await;
    Mock::given(path("/api/v1/browse/tracks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [track(1), track(2)], "limit": 500, "offset": 0, "total": 2
        })))
        .mount(&server)
        .await;
    Mock::given(path("/api/v1/tracks/1/lyrics"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "lines": [{ "text": "一", "timeMs": 0 }, { "text": "二", "timeMs": 5000 }],
            "synced": true, "sourceName": "LRCLIB"
        })))
        .mount(&server)
        .await;
    Mock::given(path("/api/v1/tracks/2/lyrics"))
        .respond_with(ResponseTemplate::new(404))
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

struct Events(mpsc::Receiver<Event>);

impl Events {
    /// Waits for the first event matching `pred`, skipping others.
    fn wait(&self, what: &str, pred: impl Fn(&Event) -> bool) -> Event {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.0.recv_timeout(left) {
                Ok(e) if pred(&e) => return e,
                Ok(_) => {}
                Err(_) => panic!("timed out waiting for {what}"),
            }
        }
    }
}

async fn playback_events(server: &MockServer) -> Vec<Value> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == "/api/v1/playback/events")
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .collect()
}

async fn wait_for_events(server: &MockServer, n: usize) -> Vec<Value> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let events = playback_events(server).await;
        if events.len() >= n || Instant::now() > deadline {
            return events;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn login_play_auto_advance_and_report() {
    let server = server().await;
    let engine = Arc::new(FakeEngine::default());
    let data_dir = std::env::temp_dir().join(format!("032player-core-{}", std::process::id()));
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let config = CoreConfig {
        data_dir,
        secrets: Arc::new(MemorySecrets::default()),
        settings: Settings::default(),
    };
    let core = client_core::start(engine.clone(), config, move |e| {
        let _ = tx.lock().unwrap().send(e);
    });
    let events = Events(rx);

    core.send(Command::Login {
        server_url: server.uri(),
        api_token: "api".into(),
        media_token: "media".into(),
    });
    events.wait("login", |e| matches!(e, Event::LoggedIn { .. }));
    // Signing in opens the album grid.
    let first = events.wait("albums", |e| matches!(e, Event::ViewRows { .. }));
    let Event::ViewRows {
        view, rows, total, ..
    } = first
    else {
        unreachable!()
    };
    assert_eq!(view, View::Albums);
    assert_eq!((rows[0].title.as_str(), rows.len(), total), ("A", 1, 1));

    // The track list is its own view.
    core.send(Command::Navigate(View::Tracks));
    let tracks = events.wait("tracks", |e| {
        matches!(
            e,
            Event::ViewRows {
                view: View::Tracks,
                ..
            }
        )
    });
    let Event::ViewRows { rows, total, .. } = tracks else {
        unreachable!()
    };
    assert_eq!((rows.len(), total), (2, 2));

    core.send(Command::PlayRow(0));
    let now = events.wait("now playing", |e| matches!(e, Event::NowPlaying(Some(_))));
    let Event::NowPlaying(Some(now)) = now else {
        unreachable!()
    };
    assert_eq!(now.title, "曲1");
    assert_eq!(now.quality.as_deref(), Some("FLAC / 16bit / 44.1kHz"));
    events.wait(
        "lyrics",
        |e| matches!(e, Event::Lyrics { lines, synced: true, .. } if lines.len() == 2),
    );
    events.wait("first line", |e| *e == Event::LyricsActive(Some(0)));

    let calls = engine.calls();
    assert!(
        calls[0].starts_with("load ")
            && calls[0].ends_with("/api/v1/tracks/1/stream?mediaToken=media"),
        "{calls:?}"
    );

    // Stale signals of the previous file before the new one is loaded.
    core.engine_event(EngineEvent::TimePos(200.0));
    core.engine_event(EngineEvent::EndFile {
        reason: EngineEnd::Eof,
        error: None,
    });
    core.engine_event(EngineEvent::FileLoaded { path: None });
    core.engine_event(EngineEvent::TimePos(6.0));
    events.wait("second line", |e| *e == Event::LyricsActive(Some(1)));
    core.engine_event(EngineEvent::TimePos(239.0));
    events.wait("position", |e| {
        matches!(
            e,
            Event::Position {
                position_ms: 239_000,
                duration_ms: 240_000
            }
        )
    });
    core.engine_event(EngineEvent::EndFile {
        reason: EngineEnd::Eof,
        error: None,
    });
    events.wait(
        "next track",
        |e| matches!(e, Event::NowPlaying(Some(n)) if n.track_id == 2),
    );

    let sent = wait_for_events(&server, 3).await;
    let kinds: Vec<String> = sent
        .iter()
        .map(|e| format!("{}:{}", e["type"].as_str().unwrap(), e["trackId"]))
        .collect();
    assert_eq!(kinds, ["start:1", "end:1", "start:2"]);
    assert_eq!(sent[0]["clientKind"], "desktop");
    assert_eq!(sent[1]["endReason"], "completed");
    assert_eq!(sent[1]["positionMillis"], 239_000);

    // Pause comes back from the engine as a property change.
    core.send(Command::TogglePause);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !engine.calls().contains(&"pause true".to_owned()) {
        assert!(Instant::now() < deadline, "pause not sent to the engine");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    core.engine_event(EngineEvent::Paused(true));
    events.wait("paused", |e| *e == Event::Paused(true));
    let sent = wait_for_events(&server, 4).await;
    assert_eq!(sent[3]["type"], "pause");

    // Close: client_closed over the keepalive path (media token in the URL).
    tokio::task::spawn_blocking(move || core.shutdown())
        .await
        .unwrap();
    let requests = server.received_requests().await.unwrap();
    let closed = requests
        .iter()
        .rfind(|r| r.url.path() == "/api/v1/playback/events")
        .unwrap();
    assert_eq!(closed.url.query(), Some("mediaToken=media"));
    let body: Value = serde_json::from_slice(&closed.body).unwrap();
    assert_eq!(body["endReason"], "client_closed");
    assert!(!closed.headers.contains_key("authorization"));
}

#[tokio::test(flavor = "multi_thread")]
async fn login_failures_are_reported() {
    let server = MockServer::start().await;
    Mock::given(path("/api/v1/capabilities"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let core = client_core::start(
        Arc::new(FakeEngine::default()),
        CoreConfig {
            data_dir: std::env::temp_dir(),
            secrets: Arc::new(MemorySecrets::default()),
            settings: Settings::default(),
        },
        move |e| {
            let _ = tx.lock().unwrap().send(e);
        },
    );
    let events = Events(rx);
    core.send(Command::Login {
        server_url: server.uri(),
        api_token: "api".into(),
        media_token: "media".into(),
    });
    events.wait("failure", |e| {
        *e == Event::LoginFailed(LoginFailure::ApiTokenInvalid)
    });
    core.send(Command::Login {
        server_url: server.uri(),
        api_token: "same".into(),
        media_token: "same".into(),
    });
    events.wait("failure", |e| {
        *e == Event::LoginFailed(LoginFailure::TokensMustDiffer)
    });
    tokio::task::spawn_blocking(move || core.shutdown())
        .await
        .unwrap();
}

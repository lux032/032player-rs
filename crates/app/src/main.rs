//! 032player: native desktop client for 032 Music Server.
//!
//! The console stays visible in every build during M0 so logs (startup
//! timing, errors) can be collected; packaged builds hide it in M4 with
//! `windows_subsystem = "windows"`.

mod artwork;
mod engine;
mod rows;
mod system;
mod ui;

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use client_core::{Command, CoreConfig, Engine, EngineEvent, SecretStore, Settings};
use slint::ComponentHandle;

/// Code generated from `ui/*.slint`.
#[allow(clippy::all, clippy::todo)]
mod generated {
    slint::include_modules!();
}
pub use generated::*;

type Forward = Box<dyn Fn(EngineEvent) + Send + Sync>;

fn data_dir() -> PathBuf {
    directories::ProjectDirs::from("", "", "032player").map_or_else(
        || std::env::temp_dir().join("032player"),
        |d| d.data_dir().to_path_buf(),
    )
}

/// Tokens in the system credential store. The store opens lazily on the
/// first call (off the UI thread); if it is missing, every call fails and
/// the core logs it, so tokens are simply not remembered.
struct SystemSecrets(os::secrets::Keyring);

impl SecretStore for SystemSecrets {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        self.0.get(key)
    }
    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        self.0.set(key, value)
    }
    fn delete(&self, key: &str) -> Result<(), String> {
        self.0.delete(key)
    }
}

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_default().trim().to_owned()
}

fn main() -> Result<(), slint::PlatformError> {
    let started = Instant::now();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,wgpu=warn,naga=warn".into()),
        )
        .init();

    let data_dir = data_dir();
    tracing::info!(data_dir = %data_dir.display(), "starting");
    // A second start shows the running instance's window and exits.
    let _instance = match os::instance::acquire(&data_dir, system::activate) {
        os::instance::Instance::Primary(guard) => guard,
        os::instance::Instance::Secondary { notified } => {
            tracing::info!(notified, "already running");
            return Ok(());
        }
    };

    let window = AppWindow::new()?;
    let settings = Settings::load(&data_dir);
    let player_options = player::PlayerOptions {
        audio_device: settings.audio_device.clone(),
        exclusive: settings.exclusive,
        volume: settings.volume,
        ..player::PlayerOptions::default()
    };

    // The player callback runs on mpv's event thread; it forwards into the
    // core once the core exists.
    let forward: Arc<OnceLock<Forward>> = Arc::new(OnceLock::new());
    let player_forward = forward.clone();
    let (engine, engine_error): (Arc<dyn Engine>, Option<String>) =
        match player::Player::new(&player_options, move |event| {
            if let (Some(f), Some(e)) = (player_forward.get(), engine::map_event(event)) {
                f(e);
            }
        }) {
            Ok(p) => (Arc::new(engine::MpvEngine(p)), None),
            Err(e) => {
                let msg = format!("mpv 初始化失败：{e}");
                tracing::error!("{msg}");
                (Arc::new(engine::NullEngine(msg.clone())), Some(msg))
            }
        };

    let core = client_core::start(
        engine.clone(),
        CoreConfig {
            data_dir: data_dir.clone(),
            secrets: Arc::new(SystemSecrets(os::secrets::Keyring)),
            settings: settings.clone(),
        },
        |event| {
            let _ = slint::invoke_from_event_loop(move || {
                system::on_event(&event);
                ui::apply(event);
            });
        },
    );
    let _ = forward.set(Box::new(core.engine_sender()));

    ui::install(&window, core.commander(), &settings);

    // Development: a complete set of M032_* variables signs in with them;
    // otherwise the saved sign-in is restored.
    let (server, api_token, media_token) = (
        env("M032_SERVER_URL"),
        env("M032_API_TOKEN"),
        env("M032_MEDIA_TOKEN"),
    );
    if server.is_empty() || api_token.is_empty() || media_token.is_empty() {
        core.send(Command::Restore);
    } else {
        window.set_login_server(server.into());
        window.invoke_login(
            window.get_login_server(),
            api_token.into(),
            media_token.into(),
        );
    }
    if let Some(msg) = engine_error {
        window.set_notice(msg.into());
    }

    let _ = slint::invoke_from_event_loop(move || {
        tracing::info!(
            elapsed_ms = started.elapsed().as_millis(),
            "window shown (first event loop turn)"
        );
    });
    window.show()?;
    system::install(&window, core.commander());
    // Runs until quit (closing may only hide the window to the tray).
    slint::run_event_loop_until_quit()?;
    let _ = window.hide();

    tracing::info!("shutting down");
    core.shutdown();
    drop(engine);
    Ok(())
}

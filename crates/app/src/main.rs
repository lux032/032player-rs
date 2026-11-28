//! 032player: native desktop client for 032 Music Server.
//!
//! Release builds on Windows have no console window; application logs go to
//! `<data dir>/logs/032player.log` and native mpv warnings to `mpv.log`
//! (settings page: "打开日志文件夹"). Debug builds also log to the console.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod cards;
mod engine;
mod fonts;
mod system;
mod ui;
mod window_chrome;

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use client_core::{Command, CoreConfig, Engine, EngineEvent, SecretStore, Settings};
use slint::{ComponentHandle, Model};

/// Code generated from `ui/*.slint` (compiled in the separate `app-ui` crate).
pub use app_ui::*;

type Forward = Box<dyn Fn(EngineEvent) + Send + Sync>;

fn credit_widths(artists: &slint::ModelRc<CreditPart>, size: f32, weight: i32) -> Vec<f32> {
    (0..artists.row_count())
        .filter_map(|i| {
            let artist = artists.row_data(i)?;
            let name_width = fonts::text_width(&artist.text, size, weight);
            let separator_width = if i == 0 {
                0.0
            } else {
                fonts::text_width(&artist.join_phrase, size, 400)
            };
            Some(name_width + separator_width)
        })
        .collect()
}

/// Log directory (also opened from the settings page).
pub fn log_dir() -> PathBuf {
    data_dir().join("logs")
}

/// Log to `logs/032player.log` (appended; moved to `032player.old.log`
/// past 2 MB) and to stderr; panics are logged too.
fn init_logging(data_dir: &std::path::Path) {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info,wgpu=warn,naga=warn".into());
    let dir = data_dir.join("logs");
    let path = dir.join("032player.log");
    let file = std::fs::create_dir_all(&dir).ok().and_then(|()| {
        if std::fs::metadata(&path).is_ok_and(|m| m.len() > 2 * 1024 * 1024) {
            // Fails while another instance writes it; then keep appending.
            let _ = std::fs::rename(&path, dir.join("032player.old.log"));
        }
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok()
    });
    let file_layer = file.map(|f| {
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_writer(std::sync::Mutex::new(f))
    });
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(file_layer)
        .init();
    std::panic::set_hook(Box::new(|info| {
        tracing::error!("panic: {info}");
    }));
}

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
    let data_dir = data_dir();
    init_logging(&data_dir);
    tracing::info!(data_dir = %data_dir.display(), "starting");
    // A second start shows the running instance's window and exits.
    let _instance = match os::instance::acquire(&data_dir, system::activate) {
        os::instance::Instance::Primary(guard) => guard,
        os::instance::Instance::Secondary { notified } => {
            tracing::info!(notified, "already running");
            return Ok(());
        }
    };

    let settings = Settings::load(&data_dir);
    // The font collection belongs to the platform: select it first, then
    // register the bundled fonts before any text is measured.
    window_chrome::backend_selector(settings.theme, settings.window.maximized).select()?;
    fonts::register();
    let window = AppWindow::new()?;
    window
        .global::<Fonts>()
        .on_family(|text| fonts::family_for(&text).into());
    window
        .global::<Fonts>()
        .on_display(|text| fonts::display_family(&text).into());
    window
        .global::<ArtistLayout>()
        .on_fit_count(|artists, avail, size, weight| {
            let widths = credit_widths(&artists, size, weight);
            fonts::fit_artist_count(&widths, avail, size) as i32
        });
    window
        .global::<ArtistLayout>()
        .on_full_width(|artists, size, weight| {
            fonts::artists_full_width(&credit_widths(&artists, size, weight))
        });
    window
        .global::<ArtistLayout>()
        .on_chip_width(|hidden, size| {
            fonts::artist_chip_width(usize::try_from(hidden).unwrap_or(0), size)
        });
    window
        .global::<ArtistLayout>()
        .on_cap_width(|text, size, max_w| fonts::text_width(&text, size, 400).min(max_w));
    let player_options = player::PlayerOptions {
        audio_device: settings.audio_device.clone(),
        exclusive: settings.exclusive,
        volume: settings.volume,
        // Keep native AO / network warnings: `paused-for-cache` alone cannot
        // distinguish a stream underrun from a Windows audio device underrun.
        // Captured via libmpv log events so query credentials (`mediaToken`)
        // are redacted before writing `mpv.log`.
        log_file: Some(log_dir().join("mpv.log")),
        log_level: Some("warn".into()),
        extra: Vec::new(),
    };

    // The player callback runs on mpv's event thread; it forwards into the
    // core once the core exists and mpv is the active backend.
    let forward: Arc<OnceLock<Forward>> = Arc::new(OnceLock::new());
    let active_backend = Arc::new(std::sync::atomic::AtomicU8::new(engine::ACTIVE_NONE));
    let player_forward = forward.clone();
    let mpv_active = active_backend.clone();
    let (engine, engine_error): (Arc<dyn Engine>, Option<String>) =
        match player::Player::new(&player_options, move |event| {
            if mpv_active.load(std::sync::atomic::Ordering::Acquire) != engine::ACTIVE_MPV
                && !matches!(event, player::PlayerEvent::Volume(_))
            {
                return;
            }
            if let (Some(f), Some(e)) = (player_forward.get(), engine::map_event(event)) {
                f(e);
            }
        }) {
            Ok(p) => (
                Arc::new(engine::HybridEngine::new(
                    p,
                    settings.volume,
                    settings.exclusive,
                    forward.clone(),
                    active_backend,
                )),
                None,
            ),
            Err(e) => {
                let msg = format!("mpv 初始化失败：{e}");
                tracing::error!("{msg}");
                (Arc::new(engine::NullEngine(msg.clone())), Some(msg))
            }
        };

    if let Some(clsid) = settings.asio_driver.as_deref() {
        let (left, right) = settings.asio_channel_pair(clsid);
        let _ = engine.set_asio_target(Some(client_core::AsioTarget {
            clsid: clsid.to_owned(),
            left_channel: left,
            right_channel: right,
        }));
    }

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
    core.send(Command::CheckUpdate);
    ui::begin_restore();
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
    // Last size and position; maximized again after the window exists.
    window.window().set_size(slint::LogicalSize::new(
        settings.window.width,
        settings.window.height,
    ));
    let restored = settings.window.x.zip(settings.window.y);
    if let Some((x, y)) = restored {
        window
            .window()
            .set_position(slint::PhysicalPosition::new(x, y));
    }
    window.show()?;
    window_chrome::install(&window);
    if restored.is_some() {
        keep_on_screen(&window, settings.window.maximized);
    } else if settings.window.maximized {
        window.window().set_maximized(true);
        window_chrome::sync(&window);
    }
    system::install(&window, core.commander());
    // Runs until quit (closing may only hide the window to the tray).
    slint::run_event_loop_until_quit()?;
    if let Some(state) = ui::window_state() {
        core.send(Command::SetWindow(state));
    }
    let _ = window.hide();

    tracing::info!("shutting down");
    core.shutdown();
    drop(engine);
    Ok(())
}

/// Moves the normal (unmaximized) window back onto a monitor when its restored
/// position is off every screen (e.g. it was last closed on a monitor that is
/// now gone), then applies `start_maximized` if saved.
fn keep_on_screen(window: &AppWindow, start_maximized: bool) {
    use slint::winit_030::{WinitWindowAccessor, winit};
    let weak = window.as_weak();
    let task = slint::spawn_local(async move {
        let Some(w) = weak.upgrade() else { return };
        let Ok(winit) = w.window().winit_window().await else {
            return;
        };
        if let Ok(pos) = winit.outer_position() {
            let scale = winit.scale_factor().max(1.0);
            let width = i32::try_from(winit.outer_size().width).unwrap_or(i32::MAX);
            #[allow(clippy::cast_possible_truncation)]
            let title_bar_px = (46.0 * scale).round() as i32;
            #[allow(clippy::cast_possible_truncation)]
            let min_overlap_px = (100.0 * scale).round() as i32;
            // The 38px logical title bar strip must overlap a monitor with
            // enough room to drag (leaving headroom for a top taskbar/panel).
            let reachable = winit.available_monitors().any(|m| {
                let (mp, ms) = (m.position(), m.size());
                let (mw, mh) = (
                    i32::try_from(ms.width).unwrap_or(i32::MAX),
                    i32::try_from(ms.height).unwrap_or(i32::MAX),
                );
                let top_visible = pos.y + title_bar_px > mp.y && pos.y < mp.y + mh;
                let overlap = (pos.x + width).min(mp.x + mw) - pos.x.max(mp.x);
                top_visible && overlap >= min_overlap_px
            });
            if !reachable
                && let Some(monitor) = winit
                    .primary_monitor()
                    .or_else(|| winit.available_monitors().next())
            {
                let (mp, ms, size) = (monitor.position(), monitor.size(), winit.outer_size());
                let center = |origin: i32, total: u32, own: u32| {
                    origin + i32::try_from(total.saturating_sub(own) / 2).unwrap_or(0)
                };
                tracing::info!(
                    x = pos.x,
                    y = pos.y,
                    "window was off screen, moving it back"
                );
                winit.set_outer_position(winit::dpi::PhysicalPosition::new(
                    center(mp.x, ms.width, size.width),
                    center(mp.y, ms.height, size.height),
                ));
            }
        }
        if start_maximized {
            winit.set_maximized(true);
        }
        window_chrome::sync(&w);
    });
    if let Err(e) = task {
        tracing::warn!("cannot check the window position: {e}");
        if start_maximized {
            window.window().set_maximized(true);
            window_chrome::sync(window);
        }
    }
}

//! System integration on the UI thread: media controls (SMTC / MPRIS),
//! the tray icon, showing / hiding the window and quitting.

use std::cell::{Cell, RefCell};
use std::time::Duration;

use client_core::{Command, Commander, Event};
use os::media::{MediaCommand, MediaControls, Metadata, Playback};
use slint::ComponentHandle;

use crate::AppWindow;

/// The system's position is refreshed at least this often while playing.
const MEDIA_POSITION_STEP_MS: i64 = 5_000;

struct System {
    window: slint::Weak<AppWindow>,
    commander: Commander,
    media: RefCell<Option<MediaControls>>,
    metadata: RefCell<Option<Metadata>>,
    #[cfg(windows)]
    tray: RefCell<Option<os::tray::Tray>>,
    has_track: Cell<bool>,
    track_id: Cell<Option<i64>>,
    paused: Cell<bool>,
    position_ms: Cell<i64>,
    sent_position_ms: Cell<i64>,
}

thread_local! {
    static SYSTEM: RefCell<Option<std::rc::Rc<System>>> = const { RefCell::new(None) };
}

fn with_system(f: impl FnOnce(&System)) {
    let system = SYSTEM.with(|cell| cell.borrow().clone());
    if let Some(system) = system {
        f(&system);
    }
}

/// Runs `f` on the UI thread (callbacks of the OS come from other threads).
fn on_ui(f: impl FnOnce(&System) + Send + 'static) {
    let _ = slint::invoke_from_event_loop(move || with_system(f));
}

/// Native window handle (HWND on Windows).
fn native_handle(window: &AppWindow) -> Option<isize> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = window.window().window_handle();
    match handle.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
        _ => None,
    }
}

/// Call after the window is shown (the media controls need its handle).
pub fn install(window: &AppWindow, commander: Commander) {
    let media = match MediaControls::new(native_handle(window), |command| {
        on_ui(move |s| s.media_command(command));
    }) {
        Ok(media) => Some(media),
        Err(e) => {
            tracing::warn!("system media controls unavailable: {e}");
            None
        }
    };

    let system = std::rc::Rc::new(System {
        window: window.as_weak(),
        commander,
        media: RefCell::new(media),
        metadata: RefCell::new(None),
        #[cfg(windows)]
        tray: RefCell::new(tray()),
        has_track: Cell::new(false),
        track_id: Cell::new(None),
        paused: Cell::new(true),
        position_ms: Cell::new(0),
        sent_position_ms: Cell::new(0),
    });
    #[cfg(windows)]
    window.set_tray_available(system.tray.borrow().is_some());
    SYSTEM.with(|cell| *cell.borrow_mut() = Some(system));

    // Closing hides to the tray when enabled (and there is a tray), else quits.
    let weak = window.as_weak();
    window.window().on_close_requested(move || {
        if let Some(w) = weak.upgrade() {
            handle_close_request(&w);
        } else {
            crate::ui::record_window_now();
            let _ = slint::quit_event_loop();
        }
        slint::CloseRequestResponse::HideWindow
    });
}

#[cfg(windows)]
fn tray() -> Option<os::tray::Tray> {
    use os::tray::{Tray, TrayAction};
    let icon = match client_core::decode(include_bytes!("../assets/tray.png"), 32, 32) {
        Ok(icon) => icon,
        Err(e) => {
            tracing::warn!("tray icon: {e}");
            return None;
        }
    };
    let result = Tray::new(icon.pixels.to_vec(), icon.width, icon.height, |action| {
        on_ui(move |s| match action {
            TrayAction::Show => s.show_window(),
            TrayAction::TogglePause => s.commander.send(Command::TogglePause),
            TrayAction::Next => s.commander.send(Command::Next),
            TrayAction::Previous => s.commander.send(Command::Previous),
            TrayAction::Stop => s.commander.send(Command::Stop),
            TrayAction::Quit => {
                crate::ui::record_window_now();
                let _ = slint::quit_event_loop();
            }
        });
    });
    match result {
        Ok(tray) => Some(tray),
        Err(e) => {
            tracing::warn!("tray unavailable: {e}");
            None
        }
    }
}

/// Shared close logic for both OS `on_close_requested` and custom title bar close.
fn handle_close_request(w: &AppWindow) {
    // Record geometry before hiding: once hidden, `is_visible()` becomes
    // false and shutdown's `window_state()` would skip the final resize.
    crate::ui::record_window_now();
    let to_tray = w.get_close_to_tray() && w.get_tray_available();
    if !to_tray {
        let _ = slint::quit_event_loop();
    }
}

/// Records window geometry, hides the window immediately (matching
/// `CloseRequestResponse::HideWindow`), and quits unless closing to the tray.
pub fn close_or_hide(w: &AppWindow) {
    handle_close_request(w);
    let _ = w.hide();
}

/// A second start of the app asks this one to show itself.
pub fn activate() {
    on_ui(System::show_window);
}

/// Mirrors core events into the media controls and the tray.
pub fn on_event(event: &Event) {
    with_system(|s| s.event(event));
}

fn ms(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

fn duration(ms: i64) -> Duration {
    Duration::from_millis(u64::try_from(ms).unwrap_or(0))
}

impl System {
    fn show_window(&self) {
        if let Some(w) = self.window.upgrade() {
            let _ = w.show();
            w.window().set_minimized(false);
            crate::window_chrome::focus_and_sync(&w);
        }
    }

    fn media_command(&self, command: MediaCommand) {
        let c = &self.commander;
        let paused = self.paused.get();
        match command {
            MediaCommand::Play if paused => c.send(Command::TogglePause),
            MediaCommand::Pause if !paused && self.has_track.get() => {
                c.send(Command::TogglePause);
            }
            MediaCommand::Play | MediaCommand::Pause => {}
            MediaCommand::Toggle => c.send(Command::TogglePause),
            MediaCommand::Next => c.send(Command::Next),
            MediaCommand::Previous => c.send(Command::Previous),
            MediaCommand::Stop => c.send(Command::Stop),
            MediaCommand::SeekTo(at) => c.send(Command::Seek {
                position_ms: ms(at),
            }),
            MediaCommand::SeekBy { forward, by } => {
                let delta = if forward { ms(by) } else { -ms(by) };
                c.send(Command::Seek {
                    position_ms: (self.position_ms.get() + delta).max(0),
                });
            }
            MediaCommand::SetVolume(volume) => {
                c.send(Command::SetVolume((volume * 100.0).clamp(0.0, 100.0)));
            }
            MediaCommand::Raise => self.show_window(),
            MediaCommand::Quit => {
                crate::ui::record_window_now();
                let _ = slint::quit_event_loop();
            }
        }
    }

    fn send_playback(&self) {
        let playback = if !self.has_track.get() {
            Playback::Stopped
        } else if self.paused.get() {
            Playback::Paused(duration(self.position_ms.get()))
        } else {
            Playback::Playing(duration(self.position_ms.get()))
        };
        self.sent_position_ms.set(self.position_ms.get());
        if let Some(media) = self.media.borrow_mut().as_mut() {
            media.set_playback(playback);
        }
    }

    fn event(&self, event: &Event) {
        match event {
            Event::NowPlaying(now) => {
                let next_id = now.as_ref().map(|n| n.track_id);
                if self.track_id.replace(next_id) != next_id {
                    self.position_ms.set(0);
                }
                self.has_track.set(now.is_some());
                #[cfg(windows)]
                if let Some(tray) = self.tray.borrow().as_ref() {
                    let tip = now.as_ref().map_or_else(
                        || "032player".to_owned(),
                        |n| format!("{} - {}", n.title, n.artist),
                    );
                    tray.set_tooltip(&tip);
                }
                let next_meta = now.as_ref().map(|now| Metadata {
                    title: now.title.clone(),
                    artist: now.artist.clone(),
                    album: now.album.clone(),
                    cover_path: now.artwork_path.clone(),
                    duration: (now.duration_ms > 0).then(|| duration(now.duration_ms)),
                });
                if let (Some(media), Some(meta)) = (self.media.borrow_mut().as_mut(), &next_meta) {
                    media.set_metadata(meta);
                }
                *self.metadata.borrow_mut() = next_meta;
                self.send_playback();
            }
            Event::MediaCover(path) => {
                let mut meta_ref = self.metadata.borrow_mut();
                if let Some(meta) = meta_ref.as_mut() {
                    meta.cover_path = path.clone();
                    if let Some(media) = self.media.borrow_mut().as_mut() {
                        media.set_metadata(meta);
                    }
                }
            }
            Event::Paused(paused) => {
                self.paused.set(*paused);
                self.send_playback();
            }
            Event::Position { position_ms, .. } => {
                self.position_ms.set(*position_ms);
                // Seeks jump; playback drifts slowly: refresh on either.
                let expected = self.sent_position_ms.get();
                let drift = position_ms - expected;
                if !(0..MEDIA_POSITION_STEP_MS).contains(&drift) {
                    self.send_playback();
                }
            }
            _ => {}
        }
    }
}

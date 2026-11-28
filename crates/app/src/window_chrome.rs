//! Frameless window chrome controls, focus/maximize tracking, and safe
//! Windows 11 DWM border/corner synchronization via `winit` (zero `unsafe`).

use client_core::ThemeMode;
use slint::ComponentHandle;
use slint::winit_030::{EventResult, WinitWindowAccessor, winit};

use crate::AppWindow;

/// Builds the Slint backend selector with pre-creation window attributes so
/// Windows 11 applies rounded corners, shadow, and theme-matched border color
/// before the first frame paints.
pub fn backend_selector(theme: ThemeMode, maximized: bool) -> slint::BackendSelector {
    #[cfg(windows)]
    {
        use winit::platform::windows::{CornerPreference, WindowAttributesExtWindows};
        let initial_border = match theme {
            ThemeMode::Dark if !maximized => Some(windows_border_color(true, true)),
            ThemeMode::Light if !maximized => Some(windows_border_color(false, true)),
            // For System theme (unknown before creation) or maximized windows,
            // defer coloring until `install` reads the live OS theme and state.
            _ => None,
        };
        let corners = if maximized {
            CornerPreference::DoNotRound
        } else {
            CornerPreference::Round
        };
        slint::BackendSelector::new().with_winit_window_attributes_hook(move |attrs| {
            let attrs = attrs
                .with_corner_preference(corners)
                .with_undecorated_shadow(!maximized);
            match initial_border {
                Some(color) => attrs.with_border_color(Some(color)),
                None => attrs,
            }
        })
    }
    #[cfg(not(windows))]
    {
        let _ = (theme, maximized);
        slint::BackendSelector::new()
    }
}

/// Wires custom title bar actions and observes native winit window events.
/// Call after `window.show()`.
pub fn install(window: &AppWindow) {
    #[cfg(target_os = "macos")]
    window.set_disable_client_resize(true);

    let weak = window.as_weak();
    window.on_window_drag(move || {
        if let Some(w) = weak.upgrade() {
            w.window().with_winit_window(|win| {
                let _ = win.drag_window();
            });
        }
    });

    let weak = window.as_weak();
    window.on_window_minimize(move || {
        if let Some(w) = weak.upgrade() {
            w.window().set_minimized(true);
        }
    });

    let weak = window.as_weak();
    window.on_window_toggle_maximize(move || {
        if let Some(w) = weak.upgrade() {
            // Read the confirmed maximized state and request a toggle without
            // optimistically flipping the UI icon before the WM confirms it.
            let next = !w.get_window_maximized();
            w.window().set_maximized(next);
            sync(&w);
        }
    });

    let weak = window.as_weak();
    window.on_window_close(move || {
        if let Some(w) = weak.upgrade() {
            crate::system::close_or_hide(&w);
        }
    });

    let weak = window.as_weak();
    window
        .window()
        .on_winit_window_event(move |slint_win, event| {
            if let Some(w) = weak.upgrade() {
                match event {
                    winit::event::WindowEvent::Focused(focused) => {
                        if w.get_window_active() != *focused {
                            w.set_window_active(*focused);
                        }
                        slint_win.with_winit_window(|win| sync_from_winit(win, &w, None));
                    }
                    winit::event::WindowEvent::Resized(_)
                    | winit::event::WindowEvent::ScaleFactorChanged { .. } => {
                        slint_win.with_winit_window(|win| {
                            if sync_maximized_state(win, &w) {
                                apply_platform_chrome(win, &w, None);
                            }
                        });
                    }
                    winit::event::WindowEvent::ThemeChanged(theme) => {
                        slint_win
                            .with_winit_window(|win| apply_platform_chrome(win, &w, Some(*theme)));
                    }
                    _ => {}
                }
            }
            EventResult::Propagate
        });

    sync(window);
}

/// Synchronizes Slint maximize state and platform DWM frame attributes with
/// the current theme, focus, and confirmed window maximize state.
pub fn sync(window: &AppWindow) {
    let weak = window.as_weak();
    window.window().with_winit_window(|win| {
        if let Some(w) = weak.upgrade() {
            sync_from_winit(win, &w, None);
        }
    });
}

/// Requests OS focus for the window (used by tray / second-instance activation)
/// and refreshes synchronized chrome state.
pub fn focus_and_sync(window: &AppWindow) {
    let weak = window.as_weak();
    window.window().with_winit_window(|win| {
        win.focus_window();
        if let Some(w) = weak.upgrade() {
            sync_from_winit(win, &w, None);
        }
    });
}

/// Updates `window-maximized` from `winit` only while the window is not
/// minimized (`winit::Window::is_maximized` returns `false` while minimized).
/// Returns `true` when the maximized state changed.
fn sync_maximized_state(win: &winit::window::Window, app: &AppWindow) -> bool {
    if win.is_minimized().unwrap_or(false) {
        return false;
    }
    let maximized = win.is_maximized();
    if app.get_window_maximized() != maximized {
        app.set_window_maximized(maximized);
        if maximized {
            win.set_cursor(winit::window::CursorIcon::Default);
        }
        return true;
    }
    false
}

fn sync_from_winit(
    win: &winit::window::Window,
    app: &AppWindow,
    system_theme: Option<winit::window::Theme>,
) {
    let _ = sync_maximized_state(win, app);
    apply_platform_chrome(win, app, system_theme);
}

#[cfg(windows)]
fn windows_border_color(dark: bool, active: bool) -> winit::platform::windows::Color {
    use winit::platform::windows::Color;
    // Matches `Theme.border` (`win-border`) and `Theme.win-border-inactive` in `theme.slint`.
    match (dark, active) {
        (true, true) => Color::from_rgb(0x25, 0x25, 0x2e),
        (true, false) => Color::from_rgb(0x1d, 0x1d, 0x25),
        (false, true) => Color::from_rgb(0xde, 0xde, 0xe5),
        (false, false) => Color::from_rgb(0xe6, 0xe6, 0xed),
    }
}

#[cfg(windows)]
fn apply_platform_chrome(
    win: &winit::window::Window,
    app: &AppWindow,
    system_theme: Option<winit::window::Theme>,
) {
    use winit::platform::windows::{CornerPreference, WindowExtWindows};
    let dark = match app.get_theme_mode() {
        1 => true,
        2 => false,
        _ => system_theme.or_else(|| win.theme()) != Some(winit::window::Theme::Light),
    };
    let maximized = app.get_window_maximized();
    if maximized {
        win.set_corner_preference(CornerPreference::DoNotRound);
        win.set_undecorated_shadow(false);
        win.set_border_color(None);
    } else {
        win.set_corner_preference(CornerPreference::Round);
        win.set_undecorated_shadow(true);
        win.set_border_color(Some(windows_border_color(dark, app.get_window_active())));
    }
}

#[cfg(not(windows))]
fn apply_platform_chrome(
    _win: &winit::window::Window,
    _app: &AppWindow,
    _system_theme: Option<winit::window::Theme>,
) {
}

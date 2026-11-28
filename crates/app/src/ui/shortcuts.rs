//! Window-local shortcuts. Bubble handling lets focused editors consume typing first.
use super::*;
use client_core::Settings;
use client_core::shortcuts::{ACTIONS, Shortcuts};

fn key_name(e: &slint::language::KeyEvent) -> Option<String> {
    use slint::platform::Key;
    let text = e.text.as_str();
    let key = if text == " " {
        "Space".into()
    } else if text == slint::SharedString::from(Key::LeftArrow).as_str() {
        "Left".into()
    } else if text == slint::SharedString::from(Key::RightArrow).as_str() {
        "Right".into()
    } else if text == slint::SharedString::from(Key::UpArrow).as_str() {
        "Up".into()
    } else if text == slint::SharedString::from(Key::DownArrow).as_str() {
        "Down".into()
    } else if text.len() == 1 && text.chars().all(|c| c.is_ascii_alphanumeric()) {
        text.to_ascii_uppercase()
    } else {
        return None;
    };
    let mut result = String::new();
    if e.modifiers.control {
        result.push_str("Ctrl+");
    }
    if e.modifiers.alt {
        result.push_str("Alt+");
    }
    if e.modifiers.shift {
        result.push_str("Shift+");
    }
    if e.modifiers.meta {
        return None;
    }
    result.push_str(&key);
    Some(result)
}

fn refresh(w: &AppWindow, keys: &Shortcuts) {
    w.set_search_shortcut(keys.binding("search").into());
    w.set_shortcuts(ModelRc::new(VecModel::from(
        ACTIONS
            .iter()
            .map(|(id, title, _)| crate::ShortcutRow {
                title: (*title).into(),
                binding: keys.binding(id).into(),
            })
            .collect::<Vec<_>>(),
    )));
}

pub(super) fn install(w: &AppWindow, settings: &Settings) {
    let keys = Rc::new(RefCell::new(settings.shortcuts.clone()));
    refresh(w, &keys.borrow());
    let k = keys.clone();
    w.on_capture_shortcut(move |e| {
        with_ui(|ui, w| {
            if e.text == slint::SharedString::from(slint::platform::Key::Escape) {
                w.set_recording_shortcut(-1);
                return;
            }
            if e.repeat {
                return;
            }
            let Some(key) = key_name(&e) else {
                return;
            };
            let Ok(i) = usize::try_from(w.get_recording_shortcut()) else {
                return;
            };
            let Some((id, _, _)) = ACTIONS.get(i) else {
                return;
            };
            let mut keys = k.borrow_mut();
            if keys.set(id, key) {
                refresh(w, &keys);
                ui.send(Command::SetShortcuts(keys.clone()));
                w.set_recording_shortcut(-1);
            } else {
                ui.notice("此快捷键已被使用，请先清除原绑定或选择其他组合。".into());
            }
        });
        true
    });
    let k = keys.clone();
    w.on_clear_shortcut(move |i| {
        with_ui(|ui, w| {
            let Some((id, _, _)) = usize::try_from(i).ok().and_then(|i| ACTIONS.get(i)) else {
                return;
            };
            let mut keys = k.borrow_mut();
            keys.set(id, String::new());
            refresh(w, &keys);
            w.set_recording_shortcut(-1);
            ui.send(Command::SetShortcuts(keys.clone()));
        });
    });
    let k = keys.clone();
    w.on_reset_shortcuts(move || {
        with_ui(|ui, w| {
            *k.borrow_mut() = Shortcuts::default();
            refresh(w, &k.borrow());
            w.set_recording_shortcut(-1);
            ui.send(Command::SetShortcuts(k.borrow().clone()));
        });
    });
    let k = keys.clone();
    w.on_capture_global_shortcut(move |e| {
        // Only intercept modifier-based global search bindings (e.g. Ctrl+K)
        // ahead of focused editors so bare-letter typing is never disrupted.
        if !e.modifiers.control && !e.modifiers.alt {
            return false;
        }
        let Some(key) = key_name(&e) else {
            return false;
        };
        if k.borrow().action(&key) != Some("search") {
            return false;
        }
        if !e.repeat {
            with_ui(|_, w| {
                if w.get_full_open() {
                    w.invoke_close_full();
                }
                w.invoke_focus_top_search();
            });
        }
        true
    });
    w.on_shortcut_key(move |e| {
        let Some(key) = key_name(&e) else {
            return false;
        };
        let keys = keys.borrow();
        let Some(action) = keys.action(&key) else {
            return false;
        };
        if e.repeat {
            return true;
        }
        with_ui(|ui, w| match action {
            "playPause" => ui.send(Command::TogglePause),
            "previous" => ui.send(Command::Previous),
            "next" => ui.send(Command::Next),
            "seekBack" | "seekForward" => {
                let delta = if action == "seekBack" {
                    -10_000
                } else {
                    10_000
                };
                let position_ms = (i64::from(w.get_position_ms()) + delta)
                    .clamp(0, i64::from(w.get_duration_ms()).max(0));
                ui.send(Command::Seek { position_ms });
            }
            "volumeUp" | "volumeDown" => {
                let delta = if action == "volumeUp" { 5.0 } else { -5.0 };
                ui.send(Command::SetVolume(f64::from(
                    (w.get_volume() + delta).clamp(0.0, 100.0),
                )));
            }
            "shuffle" => {
                w.invoke_toggle_shuffle();
            }
            "repeat" => {
                w.invoke_cycle_repeat();
            }
            "queue" => w.invoke_toggle_panel(true),
            "search" => {
                if w.get_full_open() {
                    w.invoke_close_full();
                }
                w.invoke_focus_top_search();
            }
            _ => {}
        });
        true
    });
}

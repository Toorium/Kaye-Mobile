//! Typing on a phone. Tapping a text or number field opens Android's own text box, with the
//! field's text in it. That box has the phone's keyboard, and holding a finger on it pastes,
//! which egui can't do in Kaye's window. What's typed goes back into the field as if it had
//! been typed there: all of it selected, the new text pasted over it, then Enter.
//!
//! A field being typed in is told by egui asking for the keyboard (`PlatformOutput::ime`),
//! which it does for every text field with focus. Its text comes from the events egui sends
//! for screen readers.
//!
//! A devtools build on a computer runs this too with `KAYE_TOUCH=1`, answering with
//! `KAYE_DEV_TYPED` instead of a text box, so scripts can test it.

use std::sync::Mutex;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use egui::{Event, Key, Modifiers, Role};

struct State {
    /// The text box is open, and its answer comes back here. `None` is Cancel.
    asking: Option<Receiver<Option<String>>>,
    /// What to hand egui next frame.
    events: Vec<Event>,
    /// The field takes several lines, where Enter would add one, so Escape finishes it.
    multiline: bool,
    /// A field was being typed in last frame.
    focused: bool,
    /// The last text a text field reported, which is the focused one's once it's focused.
    text: Option<(String, bool)>,
}

static STATE: Mutex<State> = Mutex::new(State { asking: None, events: Vec::new(), multiline: false, focused: false, text: None });

fn key(key: Key, modifiers: Modifiers) -> [Event; 2] {
    let event = |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers };
    [event(true), event(false)]
}

/// Called at the end of every frame.
pub fn after_frame(ctx: &egui::Context) {
    if !super::touch::on() {
        return;
    }
    let focused = ctx.output(|o| o.ime.is_some());
    // The phone's own keyboard would only type into the field blind, so it stays away.
    ctx.output_mut(|o| o.ime = None);
    #[cfg(target_os = "android")]
    {
        // What Kaye copies goes on the phone's own clipboard too.
        let copied: Vec<String> = ctx.output(|o| o.commands.iter().filter_map(|c| if let egui::OutputCommand::CopyText(text) = c { Some(text.clone()) } else { None }).collect());
        for text in copied {
            crate::android::set_clipboard(&text);
        }
        // Links open in the phone's browser through Kaye's own activity, which is sure to
        // reach it, rather than eframe's way.
        let links: Vec<String> = ctx.output_mut(|o| {
            let mut links = Vec::new();
            o.commands.retain(|c| match c {
                egui::OutputCommand::OpenUrl(open) => {
                    links.push(open.url.clone());
                    false
                }
                _ => true,
            });
            links
        });
        for url in links {
            crate::android::open_url(&url);
        }
    }
    let reported = ctx.output(|o| {
        o.events.iter().rev().find_map(|event| {
            let info = event.widget_info();
            matches!(info.role, Role::TextInput | Role::MultilineTextInput).then(|| (info.current_text_value.clone().unwrap_or_default(), info.role == Role::MultilineTextInput))
        })
    });

    let mut state = STATE.lock().unwrap();
    if reported.is_some() {
        state.text = reported;
    }
    let was_focused = std::mem::replace(&mut state.focused, focused);
    if let Some(asking) = &state.asking {
        match asking.try_recv() {
            Ok(answer) => {
                state.asking = None;
                match answer {
                    Some(text) => {
                        state.events.extend(key(Key::A, Modifiers::COMMAND));
                        state.events.push(Event::Paste(text));
                        let finish = if state.multiline { Key::Escape } else { Key::Enter };
                        state.events.extend(key(finish, Modifiers::NONE));
                    }
                    None => state.events.extend(key(Key::Escape, Modifiers::NONE)),
                }
                ctx.request_repaint();
            }
            Err(TryRecvError::Disconnected) => state.asking = None,
            Err(TryRecvError::Empty) => {}
        }
        return;
    }
    // Only a field that's just been tapped opens the box.
    if !focused || was_focused {
        return;
    }
    let (text, multiline) = state.text.clone().unwrap_or_default();
    // A hidden field, like the API key's, only reports dots, so the box starts empty.
    let text = if !text.is_empty() && text.chars().all(|c| c == '\u{2022}' || c == '*') { String::new() } else { text };
    let numeric = !text.trim().is_empty() && text.trim().trim_end_matches(['\u{b0}', '%']).trim().parse::<f64>().is_ok();
    let (tx, rx) = channel();
    let repaint = ctx.clone();
    std::thread::spawn(move || {
        let _ = tx.send(ask(&text, numeric));
        repaint.request_repaint();
    });
    state.asking = Some(rx);
    state.multiline = multiline;
    ctx.request_repaint();
}

#[cfg(target_os = "android")]
fn ask(text: &str, numeric: bool) -> Option<String> {
    let title = if numeric { "Enter a number" } else { "Type here" };
    crate::android::ask_text(title, text, numeric)
}

#[cfg(not(target_os = "android"))]
fn ask(text: &str, numeric: bool) -> Option<String> {
    eprintln!("[phone_text] asked for {} starting from {text:?}", if numeric { "a number" } else { "text" });
    std::env::var("KAYE_DEV_TYPED").ok()
}

/// Hands egui what the text box answered.
pub fn inject(raw_input: &mut egui::RawInput) {
    let mut state = STATE.lock().unwrap();
    raw_input.events.append(&mut state.events);
}

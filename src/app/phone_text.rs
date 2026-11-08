//! Typing on a phone. A text or number field that gets focus opens Android's own text box,
//! with the field's text in it. That box has everything a phone keyboard does, pasting
//! included, and works the same in landscape. What's typed goes back into the field as if
//! it had been typed there: all of it selected, the new text pasted over it, then Enter.

use std::sync::Mutex;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use egui::output::OutputEvent;
use egui::{Event, Key, Modifiers, Role};

struct State {
    /// The text box is open, and its answer comes back here. `None` is Cancel.
    asking: Option<Receiver<Option<String>>>,
    /// What to hand egui next frame.
    events: Vec<Event>,
    /// The field takes several lines, where Enter would add one, so Escape finishes it.
    multiline: bool,
}

static STATE: Mutex<State> = Mutex::new(State { asking: None, events: Vec::new(), multiline: false });

fn key(key: Key, modifiers: Modifiers) -> [Event; 2] {
    let event = |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers };
    [event(true), event(false)]
}

/// Called at the end of every frame.
pub fn after_frame(ctx: &egui::Context) {
    // The phone's own keyboard would only type into the field blind, so it stays away.
    ctx.output_mut(|o| o.ime = None);
    // What Kaye copies goes on the phone's own clipboard too.
    let copied: Vec<String> = ctx.output(|o| o.commands.iter().filter_map(|c| if let egui::OutputCommand::CopyText(text) = c { Some(text.clone()) } else { None }).collect());
    for text in copied {
        crate::android::set_clipboard(&text);
    }
    let mut state = STATE.lock().unwrap();
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
    let focused = ctx.output(|o| {
        o.events.iter().find_map(|event| match event {
            OutputEvent::FocusGained(info) if matches!(info.role, Role::TextInput | Role::MultilineTextInput) => {
                Some((info.current_text_value.clone().unwrap_or_default(), info.role == Role::MultilineTextInput))
            }
            _ => None,
        })
    });
    let Some((text, multiline)) = focused else {
        return;
    };
    let numeric = !text.trim().is_empty() && text.trim().trim_end_matches(['\u{b0}', '%']).trim().parse::<f64>().is_ok();
    let (tx, rx) = channel();
    let repaint = ctx.clone();
    std::thread::spawn(move || {
        let title = if numeric { "Enter a number" } else { "Type here" };
        let _ = tx.send(crate::android::ask_text(title, &text, numeric));
        repaint.request_repaint();
    });
    state.asking = Some(rx);
    state.multiline = multiline;
}

/// Hands egui what the text box answered.
pub fn inject(raw_input: &mut egui::RawInput) {
    let mut state = STATE.lock().unwrap();
    raw_input.events.append(&mut state.events);
}

//! Kaye on a touch screen: the same interface, with everything a finger presses made
//! bigger, and the panels a phone has no room for put behind buttons.
//!
//! It's on for Android. A devtools build on a computer turns it on with `KAYE_TOUCH=1`, so
//! the phone's layout can be tried and scripted there.

use std::sync::atomic::{AtomicBool, Ordering};

use egui::Vec2;

static ON: AtomicBool = AtomicBool::new(cfg!(target_os = "android"));

/// How much bigger something pressed by a finger is than its computer size.
pub const SCALE: f32 = 1.45;

/// A screen narrower than this, in points, is a phone: the side panel starts closed and
/// the timeline starts short. A tablet has room for both.
pub const PHONE_WIDTH: f32 = 1000.0;

pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// Read once at startup.
pub fn init() {
    #[cfg(feature = "devtools")]
    if std::env::var("KAYE_TOUCH").is_ok_and(|v| v == "1") {
        ON.store(true, Ordering::Relaxed);
    }
}

/// A size, made bigger for a finger.
pub fn size(desktop: f32) -> f32 {
    if on() { (desktop * SCALE).round() } else { desktop }
}

/// A font size, made bigger for reading at arm's length. Text grows less than buttons do,
/// or labels wouldn't fit beside them.
pub fn text(desktop: f32) -> f32 {
    if on() { (desktop * 7.0 / 6.0).round() } else { desktop }
}

pub fn size2(desktop: Vec2) -> Vec2 {
    if on() { (desktop * SCALE).round() } else { desktop }
}

/// What's open on a touch screen, which a computer always shows.
#[derive(Clone, Copy, Debug)]
pub struct Panels {
    /// The Workspace and Properties column.
    pub side: bool,
    /// The next drag on empty space in the 3D view draws a selection box, instead of
    /// turning the view.
    pub box_select: bool,
    /// Two fingers are moving the view, and nothing else happens until they all lift.
    pub gesture: bool,
    /// The timeline's tracks show. Closed, only its header with the play buttons is left,
    /// and the 3D view takes the room.
    pub timeline: bool,
    /// Whether the panels have been set for the screen's size yet.
    pub sized: bool,
}

impl Default for Panels {
    fn default() -> Self {
        Self { side: true, box_select: false, gesture: false, timeline: true, sized: false }
    }
}

/// Sets what starts open from the screen's size, once it's known.
pub fn size_panels(panels: &mut Panels, ctx: &egui::Context) {
    if panels.sized || !on() {
        return;
    }
    let width = ctx.content_rect().width();
    if width <= 0.0 {
        return;
    }
    panels.side = width >= PHONE_WIDTH;
    panels.sized = true;
}

/// Whether the screen is a phone's, by its width.
pub fn is_phone(ctx: &egui::Context) -> bool {
    on() && ctx.content_rect().width() < PHONE_WIDTH
}

/// How far from something a press still picks it. A fingertip covers far more than a
/// mouse pointer's tip.
pub fn reach(desktop: f32) -> f32 {
    if on() { desktop * 2.0 } else { desktop }
}

/// A pointer-gone message held back from last frame.
static GONE_WAITING: AtomicBool = AtomicBool::new(false);

/// A lifted finger is reported as released and gone in the same frame, so anything that
/// needs to know where it was let go, like a handle drag or a key being moved, saw no pointer
/// and never ended. Holding "gone" back a frame lets every drag finish first.
pub fn delay_pointer_gone(raw: &mut egui::RawInput) -> bool {
    if !on() {
        return false;
    }
    if GONE_WAITING.swap(false, Ordering::Relaxed) {
        raw.events.insert(0, egui::Event::PointerGone);
    }
    let released = raw.events.iter().any(|e| matches!(e, egui::Event::PointerButton { pressed: false, .. }));
    if released && raw.events.iter().any(|e| matches!(e, egui::Event::PointerGone)) {
        raw.events.retain(|e| !matches!(e, egui::Event::PointerGone));
        GONE_WAITING.store(true, Ordering::Relaxed);
    }
    GONE_WAITING.load(Ordering::Relaxed)
}

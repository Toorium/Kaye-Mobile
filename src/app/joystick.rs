//! Two thumbsticks over the 3D view on a touch screen, for posing without dragging the
//! handles. The left one moves what's selected, as G does, and the right one turns it, as
//! R does. Double tapping the right one switches it to free rotate, as pressing R twice does,
//! and back.
//!
//! A stick doesn't move the limb itself. It steers a pretend pointer, and the same drag G and
//! R start follows that pointer instead of the finger, so snapping, weights, IK targets and
//! the other selected limbs all work the way they do with a mouse. Letting go of a stick ends
//! the drag as one undo step.

use std::f32::consts::FRAC_PI_2;

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2, pos2, vec2};

use super::rig_mode::Mode;
use super::{KayeApp, theme, touch};
use crate::gizmo::OpKind;
use crate::modifiers;
use crate::scene::{BoneId, RigId};

/// How far the knob can go from the middle, in points, when the view has room.
const BASE_RADIUS: f32 = 46.0;
/// Smaller than this and a thumb can't steer it, so the sticks hide.
const MIN_RADIUS: f32 = 26.0;
const KNOB_SHARE: f32 = 0.42;
const MARGIN: f32 = 16.0;
/// Room under each stick for its name.
const LABEL_HEIGHT: f32 = 16.0;
/// A press this far out from a stick still takes it, as a share of its radius.
const GRAB_SHARE: f32 = 1.35;
/// Pushes smaller than this share of the way out do nothing, so a resting thumb holds the
/// pose still.
const DEAD_ZONE: f32 = 0.12;
/// How fast a full push moves the pretend pointer for Move, in points a second.
const MOVE_SPEED: f32 = 240.0;
/// How fast a full push sideways turns the limb for Rotate, in radians a second.
const SPIN_SPEED: f32 = 1.8;
/// How fast a full push moves the pretend pointer for free rotate, in points a second. The
/// drag turns 0.009 radians a point, so this is the same 1.8 radians a second.
const TUMBLE_SPEED: f32 = 200.0;
/// How far out from the limb the pretend pointer circles while turning it.
const SPIN_REACH: f32 = 90.0;
/// Two taps closer together than this, in seconds, are a double tap.
const DOUBLE_TAP: f64 = 0.35;
/// A press let go within this, in seconds, without leaving the dead zone is a tap.
const TAP_TIME: f64 = 0.3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Stick {
    Move,
    Rotate,
}

impl Stick {
    fn op(self) -> OpKind {
        match self {
            Stick::Move => OpKind::Move,
            Stick::Rotate => OpKind::Rotate,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Held {
    stick: Stick,
    /// How far the stick is pushed, up to 1 from the middle.
    push: Vec2,
    pressed_at: f64,
    /// Whether it has been pushed past the dead zone, which starts the drag.
    started: bool,
}

/// The sticks' state, kept from frame to frame.
#[derive(Clone, Debug, Default)]
pub struct Sticks {
    held: Option<Held>,
    /// The right stick turns freely, as R R does, until it's double tapped again.
    free: bool,
    /// When the right stick was last tapped, for spotting a double tap.
    last_tap: Option<f64>,
    /// Where the pretend pointer the drag follows is.
    pointer: Pos2,
    /// Where around the limb the pretend pointer is while turning it, counter-clockwise from
    /// the right.
    spin: f32,
}

impl Sticks {
    /// The pointer the current drag should follow, while a stick is making it.
    pub fn driving(&self) -> Option<Pos2> {
        self.held.filter(|held| held.started).map(|_| self.pointer)
    }
}

/// What a stick would move: the same thing G and R pick.
#[derive(Clone, Copy)]
enum Target {
    Rig(RigId),
    Bone(BoneId),
}

fn target(app: &KayeApp) -> Option<Target> {
    if let Some(rig) = app.movable_rig() {
        return Some(Target::Rig(rig));
    }
    app.selected_bone().filter(|b| !modifiers::locked_by_ik(&app.scene, *b)).map(Target::Bone)
}

/// The middle a turn goes around, which is also what Move starts from.
fn pivot(app: &KayeApp, target: Target) -> glam::Vec3 {
    match target {
        Target::Rig(rig) => app.scene.rig_frame(rig).pos,
        Target::Bone(bone) => app.scene.bones[modifiers::control_of(&app.scene, bone)].frame.pos,
    }
}

pub fn shown(app: &KayeApp) -> bool {
    touch::on() && app.settings.viewport.joysticks && app.mode == Mode::Animation && !app.home.open && !app.scene.is_empty()
}

/// Where the two sticks sit in the bottom corners, and how big they are.
struct Layout {
    radius: f32,
    left: Pos2,
    right: Pos2,
}

impl Layout {
    fn of(rect: Rect) -> Option<Self> {
        let radius = BASE_RADIUS.min(rect.height() * 0.17).min(rect.width() * 0.13);
        if radius < MIN_RADIUS {
            return None;
        }
        let y = rect.bottom() - MARGIN - LABEL_HEIGHT - radius;
        Some(Self {
            radius,
            left: pos2(rect.left() + MARGIN + radius, y),
            right: pos2(rect.right() - MARGIN - radius, y),
        })
    }

    fn center(&self, stick: Stick) -> Pos2 {
        match stick {
            Stick::Move => self.left,
            Stick::Rotate => self.right,
        }
    }
}

/// Reads the sticks and steers the drag they make. Called before anything else in the 3D
/// view reads the pointer, so a finger on a stick neither turns the view nor picks a limb.
pub fn input(app: &mut KayeApp, ui: &mut Ui, rect: Rect, dt: f32) {
    let layout = Layout::of(rect).filter(|_| shown(app));
    let Some(layout) = layout else {
        // Hidden mid-push, by switching tabs or the screen turning: keep what was done.
        if app.sticks.held.is_some() {
            let now = ui.input(|i| i.time);
            release(app, now);
        }
        return;
    };
    let now = ui.input(|i| i.time);
    let pointer = ui.input(|i| i.pointer.latest_pos());
    for stick in [Stick::Move, Stick::Rotate] {
        let center = layout.center(stick);
        let area = Rect::from_center_size(center, Vec2::splat(layout.radius * GRAB_SHARE * 2.0));
        let response = ui.interact(area, ui.scope_id().with(("joystick", stick)), Sense::drag());
        let down = response.is_pointer_button_down_on();
        let held_here = app.sticks.held.is_some_and(|held| held.stick == stick);
        if down && app.sticks.held.is_none() && app.session.is_none() {
            press(app, stick, now);
        }
        if held_here && !down {
            release(app, now);
            continue;
        }
        if let (Some(held), Some(at)) = (app.sticks.held.as_mut().filter(|h| h.stick == stick), response.interact_pointer_pos().or(pointer)) {
            let push = (at - center) / layout.radius;
            held.push = if push.length() > 1.0 { push.normalized() } else { push };
        }
    }
    if app.sticks.held.is_some() {
        steer(app, rect, dt.min(0.1));
        ui.ctx().request_repaint();
    }
}

fn press(app: &mut KayeApp, stick: Stick, now: f64) {
    if stick == Stick::Rotate && app.sticks.last_tap.is_some_and(|tap| now - tap < DOUBLE_TAP) {
        app.sticks.free = !app.sticks.free;
        app.sticks.last_tap = None;
    }
    app.sticks.held = Some(Held { stick, push: Vec2::ZERO, pressed_at: now, started: false });
}

fn release(app: &mut KayeApp, now: f64) {
    let Some(held) = app.sticks.held.take() else {
        return;
    };
    if held.started {
        app.commit_session();
    } else if held.stick == Stick::Rotate && now - held.pressed_at < TAP_TIME {
        app.sticks.last_tap = Some(now);
    }
}

/// Moves the pretend pointer by how far the stick is pushed, starting the drag the first
/// time it leaves the dead zone.
fn steer(app: &mut KayeApp, rect: Rect, dt: f32) {
    let Some(held) = app.sticks.held else {
        return;
    };
    let length = held.push.length();
    if length <= DEAD_ZONE {
        return;
    }
    // Eased, so a small push makes a fine adjustment and a full one moves quickly.
    let amount = ((length - DEAD_ZONE) / (1.0 - DEAD_ZONE)).clamp(0.0, 1.0).powi(2);
    let push = held.push / length * amount;
    if !held.started && !start(app, held.stick, rect) {
        return;
    }
    match held.stick {
        Stick::Move => app.sticks.pointer += push * MOVE_SPEED * dt,
        Stick::Rotate if app.sticks.free => app.sticks.pointer += push * TUMBLE_SPEED * dt,
        Stick::Rotate => {
            // Right turns it clockwise on screen, like a steering wheel.
            app.sticks.spin -= push.x * SPIN_SPEED * dt;
            let center = app
                .session
                .as_ref()
                .and_then(|session| app.camera.world_to_screen(session.start_frame.pos, rect))
                .unwrap_or(rect.center());
            let spin = app.sticks.spin;
            app.sticks.pointer = center + vec2(spin.cos(), -spin.sin()) * SPIN_REACH;
        }
    }
}

/// Starts the G or R drag a stick makes. False when there's nothing it can move.
fn start(app: &mut KayeApp, stick: Stick, rect: Rect) -> bool {
    let Some(target) = target(app) else {
        return false;
    };
    let center = app.camera.world_to_screen(pivot(app, target), rect).unwrap_or(rect.center());
    let free = stick == Stick::Rotate && app.sticks.free;
    // A turn around the view goes by the pointer's angle around the limb, so it starts out to
    // the limb's right. The others only go by how far the pointer has moved.
    let begin = if stick == Stick::Rotate && !free { center + vec2(SPIN_REACH, 0.0) } else { center };
    app.sticks.pointer = begin;
    app.sticks.spin = 0.0;
    let op = stick.op();
    match target {
        Target::Rig(rig) => app.start_rig_session(rig, op, None, true, begin),
        Target::Bone(bone) => app.start_session(bone, op, None, true, begin),
    }
    let Some(session) = app.session.as_mut() else {
        return false;
    };
    if free {
        session.press_rotate();
    }
    if let Some(held) = app.sticks.held.as_mut() {
        held.started = true;
    }
    true
}

/// Draws the sticks over the 3D view.
pub fn paint(app: &KayeApp, painter: &egui::Painter, rect: Rect) {
    let Some(layout) = Layout::of(rect).filter(|_| shown(app)) else {
        return;
    };
    // Dimmed while nothing is picked that they could move.
    let ready = target(app).is_some() || app.sticks.held.is_some_and(|held| held.started);
    let fade = if ready { 1.0 } else { 0.45 };
    for stick in [Stick::Move, Stick::Rotate] {
        let center = layout.center(stick);
        let radius = layout.radius;
        let held = app.sticks.held.filter(|held| held.stick == stick);
        let free = stick == Stick::Rotate && app.sticks.free;
        let ring = if free || held.is_some_and(|h| h.started) { theme::accent() } else { Color32::from_white_alpha(110) };
        painter.circle_filled(center, radius, Color32::from_black_alpha(80).gamma_multiply(fade));
        painter.circle_stroke(center, radius, Stroke::new(1.5, ring.gamma_multiply(fade)));

        let marks = Color32::from_white_alpha(150).gamma_multiply(fade);
        match stick {
            Stick::Move => four_arrows(painter, center, radius, marks),
            Stick::Rotate if free => {
                four_arrows(painter, center, radius, marks);
                painter.circle_stroke(center, radius * 0.62, Stroke::new(1.0, marks.gamma_multiply(0.6)));
            }
            Stick::Rotate => turn_arrows(painter, center, radius, marks),
        }

        let knob_at = center + held.map_or(Vec2::ZERO, |h| h.push) * radius;
        let knob = radius * KNOB_SHARE;
        let knob_fill = if held.is_some() { theme::accent() } else { Color32::from_white_alpha(200) };
        painter.circle_filled(knob_at + vec2(0.0, 1.5), knob, Color32::from_black_alpha(60).gamma_multiply(fade));
        painter.circle_filled(knob_at, knob, knob_fill.gamma_multiply(fade));

        let name = match stick {
            Stick::Move => "Move",
            Stick::Rotate if free => "Free Rotate",
            Stick::Rotate => "Rotate",
        };
        let at = center + vec2(0.0, radius + 3.0);
        let font = FontId::proportional(touch::text(11.0));
        painter.text(at + vec2(1.0, 1.0), Align2::CENTER_TOP, name, font.clone(), Color32::from_black_alpha(170).gamma_multiply(fade));
        painter.text(at, Align2::CENTER_TOP, name, font, theme::overlay().gamma_multiply(fade));
    }
}

/// Small chevrons at the four edges, for a stick that goes every way.
fn four_arrows(painter: &egui::Painter, center: Pos2, radius: f32, color: Color32) {
    let size = radius * 0.13;
    for i in 0..4 {
        let angle = i as f32 * FRAC_PI_2;
        let out = vec2(angle.cos(), angle.sin());
        let side = vec2(-out.y, out.x);
        let tip = center + out * radius * 0.86;
        let back = tip - out * size;
        painter.line_segment([back + side * size, tip], Stroke::new(1.6, color));
        painter.line_segment([back - side * size, tip], Stroke::new(1.6, color));
    }
}

/// Curved arrows on the left and right, for a stick that turns one way or the other.
fn turn_arrows(painter: &egui::Painter, center: Pos2, radius: f32, color: Color32) {
    let reach = radius * 0.8;
    let stroke = Stroke::new(1.6, color);
    // Each arc runs over the top toward its side, ending in an arrowhead: the right one
    // clockwise, the left one counter-clockwise.
    for side in [1.0_f32, -1.0] {
        let from = FRAC_PI_2 - side * 0.35;
        let to = FRAC_PI_2 - side * 1.25;
        let steps = 10;
        let points: Vec<Pos2> = (0..=steps)
            .map(|i| {
                let a = from + (to - from) * i as f32 / steps as f32;
                center + vec2(a.cos(), -a.sin()) * reach
            })
            .collect();
        let tip = *points.last().unwrap_or(&center);
        // Along the arc, the way it's going at its end.
        let a = to;
        let heading = vec2(a.sin(), a.cos()) * side;
        let normal = vec2(-heading.y, heading.x);
        let size = radius * 0.14;
        painter.add(Shape::line(points, stroke));
        painter.line_segment([tip - heading * size + normal * size * 0.8, tip], stroke);
        painter.line_segment([tip - heading * size - normal * size * 0.8, tip], stroke);
    }
}

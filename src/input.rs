//! Unified input layer: maps keyboard and gamepad to abstract actions so
//! every gameplay/menu system works identically on both devices.

use bevy::input::gamepad::GamepadButton;
use bevy::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Confirm,     // Space / A
    Cancel,      // Esc / B
    Loft,        // Left Shift hold = lofted shot / LT
    Sprint,      // Right Trigger (reserved)
    Next,        // Down or S / dpad down
    Prev,        // Up or W / dpad up
    Left,        // A or Left arrow / dpad left
    Right,       // D or Right arrow / dpad right
    CycleType,   // Q / X button
}

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(PlayerInput::default());
        app.add_systems(PreUpdate, poll_input);
    }
}

/// Snapshot of the abstract input state, refreshed every frame.
#[derive(Resource, Default)]
pub struct PlayerInput {
    pub held: Vec<Action>,
    pub just_pressed: Vec<Action>,
    pub move_vec: Vec2, // x: left(-)/right(+), y: prev(+)/next(-)
    pub gamepad_connected: bool,
}

impl PlayerInput {
    pub fn held(&self, a: Action) -> bool {
        self.held.contains(&a)
    }
    pub fn pressed(&self, a: Action) -> bool {
        self.just_pressed.contains(&a)
    }
}

pub fn poll_input(
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mut out: ResMut<PlayerInput>,
) {
    let mut held = Vec::new();
    let mut just = Vec::new();

    // ---- keyboard ----
    let map_kb = [
        (KeyCode::Space, Action::Confirm),
        (KeyCode::Enter, Action::Confirm),
        (KeyCode::Escape, Action::Cancel),
        (KeyCode::ShiftLeft, Action::Loft),
        (KeyCode::KeyW, Action::Prev),
        (KeyCode::ArrowUp, Action::Prev),
        (KeyCode::KeyS, Action::Next),
        (KeyCode::ArrowDown, Action::Next),
        (KeyCode::KeyA, Action::Left),
        (KeyCode::ArrowLeft, Action::Left),
        (KeyCode::KeyD, Action::Right),
        (KeyCode::ArrowRight, Action::Right),
        (KeyCode::KeyQ, Action::CycleType),
    ];
    for (code, action) in map_kb {
        if keys.pressed(code) {
            held.push(action);
            if keys.just_pressed(code) {
                just.push(action);
            }
        }
    }

    // ---- gamepad ----
    let _gp = pads.iter().next();
    out.gamepad_connected = pads.iter().next().is_some();
    if let Some(gp) = pads.iter().next() {
        let map_gp = [
            (GamepadButton::South, Action::Confirm),
            (GamepadButton::East, Action::Cancel),
            (GamepadButton::West, Action::CycleType),
            (GamepadButton::RightTrigger, Action::Sprint),
            (GamepadButton::DPadUp, Action::Prev),
            (GamepadButton::DPadDown, Action::Next),
            (GamepadButton::DPadLeft, Action::Left),
            (GamepadButton::DPadRight, Action::Right),
        ];
        for (btn, action) in map_gp {
            if gp.pressed(btn) {
                held.push(action);
                if gp.just_pressed(btn) {
                    just.push(action);
                }
            }
        }
        // Left trigger doubles as loft.
        if gp.pressed(GamepadButton::LeftTrigger2) {
            held.push(Action::Loft);
            if !out.held.contains(&Action::Loft) {
                just.push(Action::Loft);
            }
        }
    }

    // ---- movement vector ----
    let mut mv = Vec2::ZERO;
    if held.contains(&Action::Left) { mv.x -= 1.0; }
    if held.contains(&Action::Right) { mv.x += 1.0; }
    if held.contains(&Action::Prev) { mv.y += 1.0; }
    if held.contains(&Action::Next) { mv.y -= 1.0; }
    if let Some(gp) = pads.iter().next() {
        let stick = gp.left_stick();
        if stick.x.abs() > 0.25 || stick.y.abs() > 0.25 {
            mv = stick;
        }
    }

    out.held = held;
    out.just_pressed = just;
    out.move_vec = mv.clamp_length_max(1.0);
}

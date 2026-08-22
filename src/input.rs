//! Unified input layer: maps keyboard and gamepad to abstract actions so
//! every gameplay/menu system works identically on both devices.

use bevy::input::gamepad::GamepadButton;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Action {
    Confirm,   // Space / A
    Cancel,    // Esc / B
    Loft,      // Left Shift hold = lofted shot / LT
    Sprint,    // Right Trigger (reserved)
    Next,      // Down or S / dpad down
    Prev,      // Up or W / dpad up
    Left,      // A or Left arrow / dpad left
    Right,     // D or Right arrow / dpad right
    CycleType, // Q / X button
    CycleCam,  // C / Y button - cycle camera
}

#[derive(Resource, Serialize, Deserialize, Clone)]
pub struct KeyBindings {
    pub map: HashMap<Action, KeyCode>,
}

impl Default for KeyBindings {
    fn default() -> Self {
        Self {
            map: default_map(),
        }
    }
}

fn default_map() -> HashMap<Action, KeyCode> {
    let mut m = HashMap::new();
    m.insert(Action::Confirm, KeyCode::Space);
    m.insert(Action::Cancel, KeyCode::Escape);
    m.insert(Action::Loft, KeyCode::ShiftLeft);
    m.insert(Action::Sprint, KeyCode::ShiftRight);
    m.insert(Action::Next, KeyCode::KeyS);
    m.insert(Action::Prev, KeyCode::KeyW);
    m.insert(Action::Left, KeyCode::KeyA);
    m.insert(Action::Right, KeyCode::KeyD);
    m.insert(Action::CycleType, KeyCode::KeyQ);
    m.insert(Action::CycleCam, KeyCode::KeyC);
    m
}

fn config_path() -> std::path::PathBuf {
    if let Some(mut p) = dirs::config_dir() {
        p.push("willow_cricket");
        let _ = std::fs::create_dir_all(&p);
        p.push("controls.json");
        p
    } else {
        std::path::PathBuf::from("cricket_controls.json")
    }
}

impl KeyBindings {
    pub fn load() -> Self {
        let path = config_path();
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(v) = serde_json::from_slice::<Self>(&bytes) {
                return v;
            }
        }
        Self::default()
    }

    pub fn save(&self) {
        let path = config_path();
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, s);
        }
    }
}

/// When Some(action), the settings screen is waiting for the user to press a key
#[derive(Resource, Default)]
pub struct RebindState(pub Option<Action>);

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(KeyBindings::load());
        app.insert_resource(PlayerInput::default());
        app.insert_resource(RebindState::default());
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
    bindings: Res<KeyBindings>,
    mut out: ResMut<PlayerInput>,
) {
    let mut held = Vec::new();
    let mut just = Vec::new();

    // ---- keyboard via bindings + hardcoded secondaries ----
    // Secondary keys that always work alongside the remappable primary
    let secondaries: &[(KeyCode, Action)] = &[
        (KeyCode::Enter, Action::Confirm),
        (KeyCode::ArrowUp, Action::Prev),
        (KeyCode::ArrowDown, Action::Next),
        (KeyCode::ArrowLeft, Action::Left),
        (KeyCode::ArrowRight, Action::Right),
    ];

    for &action in &[
        Action::Confirm,
        Action::Cancel,
        Action::Loft,
        Action::Sprint,
        Action::Next,
        Action::Prev,
        Action::Left,
        Action::Right,
        Action::CycleType,
        Action::CycleCam,
    ] {
        let primary = bindings
            .map
            .get(&action)
            .copied()
            .unwrap_or_else(|| default_map()[&action]);
        let is_held = keys.pressed(primary)
            || secondaries
                .iter()
                .any(|(k, a)| *a == action && keys.pressed(*k));
        let is_just = keys.just_pressed(primary)
            || secondaries
                .iter()
                .any(|(k, a)| *a == action && keys.just_pressed(*k));
        if is_held {
            held.push(action);
        }
        if is_just {
            just.push(action);
        }
    }

    // ---- gamepad ----
    out.gamepad_connected = pads.iter().next().is_some();
    if let Some(gp) = pads.iter().next() {
        let map_gp = [
            (GamepadButton::South, Action::Confirm),
            (GamepadButton::East, Action::Cancel),
            (GamepadButton::West, Action::CycleType),
            (GamepadButton::North, Action::CycleCam),
            (GamepadButton::RightTrigger, Action::Sprint),
            (GamepadButton::DPadUp, Action::Prev),
            (GamepadButton::DPadDown, Action::Next),
            (GamepadButton::DPadLeft, Action::Left),
            (GamepadButton::DPadRight, Action::Right),
        ];
        for (btn, action) in map_gp {
            if gp.pressed(btn) {
                if !held.contains(&action) {
                    held.push(action);
                }
                if gp.just_pressed(btn) && !just.contains(&action) {
                    just.push(action);
                }
            }
        }
        if gp.pressed(GamepadButton::LeftTrigger2) {
            if !held.contains(&Action::Loft) {
                held.push(Action::Loft);
            }
            if !out.held.contains(&Action::Loft) && !just.contains(&Action::Loft) {
                just.push(Action::Loft);
            }
        }
    }

    // ---- movement vector ----
    let mut mv = Vec2::ZERO;
    if held.contains(&Action::Left) {
        mv.x -= 1.0;
    }
    if held.contains(&Action::Right) {
        mv.x += 1.0;
    }
    if held.contains(&Action::Prev) {
        mv.y += 1.0;
    }
    if held.contains(&Action::Next) {
        mv.y -= 1.0;
    }
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

/// Human-friendly keyboard label ("Left Shift" instead of `ShiftLeft`).
pub fn key_label(code: KeyCode) -> String {
    match code {
        KeyCode::Space => "Space".into(),
        KeyCode::Escape => "Esc".into(),
        KeyCode::Enter => "Enter".into(),
        KeyCode::Tab => "Tab".into(),
        KeyCode::ShiftLeft => "Left Shift".into(),
        KeyCode::ShiftRight => "Right Shift".into(),
        KeyCode::ControlLeft => "Left Ctrl".into(),
        KeyCode::ControlRight => "Right Ctrl".into(),
        KeyCode::AltLeft => "Left Alt".into(),
        KeyCode::AltRight => "Right Alt".into(),
        KeyCode::ArrowUp => "Up Arrow".into(),
        KeyCode::ArrowDown => "Down Arrow".into(),
        KeyCode::ArrowLeft => "Left Arrow".into(),
        KeyCode::ArrowRight => "Right Arrow".into(),
        _ => {
            let s = format!("{code:?}");
            s.replace("Key", "").replace("Digit", "")
        }
    }
}

/// Gamepad button glyph for an action (used when a pad is connected).
pub fn gamepad_glyph(action: Action) -> &'static str {
    match action {
        Action::Confirm => "A",
        Action::Cancel => "B",
        Action::Loft => "LT",
        Action::Sprint => "RT",
        Action::Next => "D▼",
        Action::Prev => "D▲",
        Action::Left => "D◀",
        Action::Right => "D▶",
        Action::CycleType => "X",
        Action::CycleCam => "Y",
    }
}

/// Best label for an action given the current input device.
pub fn action_label(action: Action, bindings: &KeyBindings, gamepad: bool) -> String {
    if gamepad {
        return gamepad_glyph(action).to_string();
    }
    bindings
        .map
        .get(&action)
        .map(|k| key_label(*k))
        .unwrap_or_else(|| "-".into())
}

//! Shared UI palette, scaling and accessibility preferences.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Resolution-aware UI scale (1.0 = 1080p reference).
#[derive(Resource, Clone)]
pub struct UiScale(pub f32);

impl Default for UiScale {
    fn default() -> Self {
        UiScale(1.0)
    }
}

/// Player-facing display options persisted across sessions.
#[derive(Resource, Clone, Serialize, Deserialize)]
pub struct UiPreferences {
    pub ui_scale: f32,
    pub high_contrast: bool,
    pub subtitle_scale: f32,
}

impl Default for UiPreferences {
    fn default() -> Self {
        UiPreferences {
            ui_scale: 1.0,
            high_contrast: false,
            subtitle_scale: 1.0,
        }
    }
}

impl UiPreferences {
    pub fn load() -> Self {
        if let Some(mut p) = dirs::config_dir() {
            p.push("willow_cricket");
            p.push("ui.json");
            if let Ok(bytes) = std::fs::read(p) {
                if let Ok(v) = serde_json::from_slice::<Self>(&bytes) {
                    return v;
                }
            }
        }
        Self::default()
    }

    pub fn save(&self) {
        if let Some(mut p) = dirs::config_dir() {
            p.push("willow_cricket");
            let _ = std::fs::create_dir_all(&p);
            p.push("ui.json");
            if let Ok(s) = serde_json::to_string_pretty(self) {
                let _ = std::fs::write(p, s);
            }
        }
    }
}

/// Scale a pixel value for the current UI scale setting.
pub fn spx(n: f32, scale: f32) -> Val {
    px(n * scale)
}

/// Willow broadcast palette — used consistently across menus and HUD.
pub mod palette {
    use bevy::prelude::Color;

    pub fn gold() -> Color {
        Color::srgb(0.98, 0.72, 0.25)
    }
    pub fn panel_bg() -> Color {
        Color::srgba(0.015, 0.025, 0.04, 0.96)
    }
    pub fn panel_border() -> Color {
        Color::srgba(0.72, 0.82, 0.90, 0.26)
    }
    pub fn accent_blue() -> Color {
        Color::srgb(0.08, 0.38, 0.82)
    }
    pub fn text_primary() -> Color {
        Color::srgb(0.93, 0.95, 0.97)
    }
    pub fn text_muted() -> Color {
        Color::srgb(0.77, 0.81, 0.85)
    }
    pub fn selection_bg() -> Color {
        Color::srgba(0.23, 0.43, 0.23, 0.82)
    }
    pub fn selection_border() -> Color {
        Color::srgb(0.84, 0.70, 0.29)
    }
}

/// Fade overlay for menu screen transitions.
#[derive(Resource, Default)]
pub struct MenuTransition {
    pub from_screen: Option<u8>,
    pub t: f32,
    pub active: bool,
}

pub fn tick_menu_transition(
    time: Res<Time>,
    mut trans: ResMut<MenuTransition>,
) {
    if !trans.active {
        return;
    }
    trans.t += time.delta_secs();
    if trans.t >= 0.28 {
        trans.active = false;
        trans.t = 0.0;
        trans.from_screen = None;
    }
}

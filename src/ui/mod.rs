pub mod hud;
pub mod menus;

use bevy::prelude::*;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((hud::HudPlugin, menus::MenusPlugin));
    }
}

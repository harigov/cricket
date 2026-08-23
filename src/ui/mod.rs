pub mod hud;
pub mod menus;
pub mod pause;
pub mod theme;

use bevy::prelude::*;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<theme::UiScale>()
            .init_resource::<theme::UiPreferences>()
            .init_resource::<theme::MenuTransition>()
            .add_systems(Update, theme::tick_menu_transition);
        app.add_plugins((hud::HudPlugin, menus::MenusPlugin, pause::PausePlugin));
    }
}

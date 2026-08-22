pub mod camera_rig;
pub mod player;
pub mod stadium;

use bevy::prelude::*;

/// Renderer-side systems shared across states.
pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                player::tag_skeleton_bones,
                player::animate_skeleton,
                player::animate_figures,
                camera_rig::update_camera,
            ),
        );
    }
}

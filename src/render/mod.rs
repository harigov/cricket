pub mod camera_rig;
pub mod player;
pub mod stadium;

use bevy::prelude::*;
use bevy::{asset::AssetPath, gltf::GltfAssetLabel};

pub fn load_sponsor_ribbon(assets: &AssetServer) -> Handle<Image> {
    bevy::asset::load_embedded_asset!(
        assets,
        "../../assets/branding/stadium/sponsor-ribbon.png"
    )
}

pub fn load_team_crest(assets: &AssetServer, asset_path: &str) -> Handle<Image> {
    match asset_path {
        "branding/teams/ind.png" => bevy::asset::load_embedded_asset!(
            assets,
            "../../assets/branding/teams/ind.png"
        ),
        "branding/teams/aus.png" => bevy::asset::load_embedded_asset!(
            assets,
            "../../assets/branding/teams/aus.png"
        ),
        "branding/teams/eng.png" => bevy::asset::load_embedded_asset!(
            assets,
            "../../assets/branding/teams/eng.png"
        ),
        "branding/teams/pak.png" => bevy::asset::load_embedded_asset!(
            assets,
            "../../assets/branding/teams/pak.png"
        ),
        "branding/teams/rsa.png" => bevy::asset::load_embedded_asset!(
            assets,
            "../../assets/branding/teams/rsa.png"
        ),
        "branding/teams/nzl.png" => bevy::asset::load_embedded_asset!(
            assets,
            "../../assets/branding/teams/nzl.png"
        ),
        "branding/teams/wis.png" => bevy::asset::load_embedded_asset!(
            assets,
            "../../assets/branding/teams/wis.png"
        ),
        "branding/teams/lka.png" => bevy::asset::load_embedded_asset!(
            assets,
            "../../assets/branding/teams/lka.png"
        ),
        "branding/teams/bgd.png" => bevy::asset::load_embedded_asset!(
            assets,
            "../../assets/branding/teams/bgd.png"
        ),
        "branding/teams/afg.png" => bevy::asset::load_embedded_asset!(
            assets,
            "../../assets/branding/teams/afg.png"
        ),
        _ => bevy::asset::load_embedded_asset!(
            assets,
            "../../assets/branding/teams/ind.png"
        ),
    }
}

pub fn load_xbot_scene(assets: &AssetServer) -> Handle<Scene> {
    let path = bevy::asset::embedded_path!("../../assets/characters/Xbot.glb");
    let path = AssetPath::from_path_buf(path).with_source("embedded");
    assets.load(GltfAssetLabel::Scene(0).from_asset(path))
}

/// Renderer-side systems shared across states.
pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        // Match art is bundled into the executable so direct launches from
        // target/debug or target/release retain the complete presentation.
        bevy::asset::embedded_asset!(app, "../../assets/characters/Xbot.glb");
        bevy::asset::embedded_asset!(
            app,
            "../../assets/branding/stadium/sponsor-ribbon.png"
        );
        bevy::asset::embedded_asset!(app, "../../assets/branding/teams/ind.png");
        bevy::asset::embedded_asset!(app, "../../assets/branding/teams/aus.png");
        bevy::asset::embedded_asset!(app, "../../assets/branding/teams/eng.png");
        bevy::asset::embedded_asset!(app, "../../assets/branding/teams/pak.png");
        bevy::asset::embedded_asset!(app, "../../assets/branding/teams/rsa.png");
        bevy::asset::embedded_asset!(app, "../../assets/branding/teams/nzl.png");
        bevy::asset::embedded_asset!(app, "../../assets/branding/teams/wis.png");
        bevy::asset::embedded_asset!(app, "../../assets/branding/teams/lka.png");
        bevy::asset::embedded_asset!(app, "../../assets/branding/teams/bgd.png");
        bevy::asset::embedded_asset!(app, "../../assets/branding/teams/afg.png");
        app.add_systems(
            Update,
            (
                player::tag_skeleton_bones,
                player::apply_team_kit_materials,
                player::animate_skeleton,
                player::animate_figures,
                camera_rig::update_camera,
            ),
        );
    }
}

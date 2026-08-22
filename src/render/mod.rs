pub mod camera_rig;
pub mod outfield_grass;
pub mod player;
pub mod ring_geometry;
pub mod sky;
pub mod stadium;

/// Day lighting group toggled by stadium time.
#[derive(Component)]
pub struct DayEnvironmentLight;

/// Night lighting group toggled by stadium time (moon + flood spots).
#[derive(Component)]
pub struct NightEnvironmentLight;

/// Visible floodlight lamp bank — emissive material swapped on time change.
#[derive(Component)]
pub struct FloodlightFixture;

/// Cached procedural sky textures (generated once at startup).
#[derive(Resource)]
pub struct SkyTextures {
    pub day: Handle<Image>,
    pub night: Handle<Image>,
}

/// Day/night emissive materials for floodlight fixtures.
#[derive(Resource)]
pub struct FloodlightMaterials {
    pub day: Handle<StandardMaterial>,
    pub night: Handle<StandardMaterial>,
}

use bevy::image::{
    CompressedImageFormats, ImageAddressMode, ImageSampler, ImageSamplerDescriptor, ImageType,
};
use bevy::prelude::*;
use bevy::{asset::AssetPath, gltf::GltfAssetLabel};
use bevy::render::render_resource::TextureUsages;

use crate::render::outfield_grass::append_rgba8_srgb_mip_chain;

const OUTFIELD_GRASS_PNG: &[u8] =
    include_bytes!("../../assets/textures/stadium/outfield-grass-albedo-v2.png");

/// Decode the embedded grass albedo and attach a CPU-generated mip chain.
pub fn create_outfield_grass_image() -> Image {
    let mut sampler = ImageSamplerDescriptor::linear();
    sampler.address_mode_u = ImageAddressMode::Repeat;
    sampler.address_mode_v = ImageAddressMode::Repeat;
    sampler.set_anisotropic_filter(8);

    let mut image = Image::from_buffer(
        OUTFIELD_GRASS_PNG,
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::Descriptor(sampler),
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    )
    .expect("embedded outfield grass PNG must decode");
    image.texture_descriptor.usage |= TextureUsages::COPY_DST;
    append_rgba8_srgb_mip_chain(&mut image);
    image
}

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
        bevy::asset::embedded_asset!(
            app,
            "../../assets/textures/stadium/outfield-grass-albedo-v2.png"
        );
        // Shared mocap locomotion graph (idle/run) for every figure.
        player::build_locomotion_clips(app);
        app.add_systems(
            Update,
            (
                player::tag_skeleton_bones,
                player::apply_team_kit_materials,
                player::attach_animation_players,
                player::animate_figures,
            ),
        )
        .add_systems(PostUpdate, player::strip_skeleton_root_motion);
    }
}

pub mod camera_rig;
pub mod crowd;
pub mod environment;
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

/// Sky textures currently hanging on the shared dome.
///
/// Painting 2 M texels of fractal noise is not something to do per frame, so
/// the dome keeps one day/night pair and [`SkyTextureCache`] holds every pair
/// painted so far. `theme` records which stadium's air is on the dome.
#[derive(Resource)]
pub struct SkyTextures {
    pub day: Handle<Image>,
    pub night: Handle<Image>,
    pub theme: crate::core::stadiums::StadiumEnvironment,
}

/// Day/night sky textures painted so far, keyed by theme.
///
/// A tournament revisits grounds, and repainting a sky the player has already
/// seen would stall the frame that builds the stadium.
#[derive(Resource, Default)]
pub struct SkyTextureCache {
    by_theme: std::collections::HashMap<
        crate::core::stadiums::StadiumEnvironment,
        (Handle<Image>, Handle<Image>),
    >,
}

impl SkyTextureCache {
    /// Day and night handles for `theme`, painting them on first request.
    pub fn get_or_paint(
        &mut self,
        theme: crate::core::stadiums::StadiumEnvironment,
        images: &mut Assets<Image>,
    ) -> (Handle<Image>, Handle<Image>) {
        self.by_theme
            .entry(theme)
            .or_insert_with(|| {
                (
                    images.add(sky::create_themed_sky_texture(theme, false)),
                    images.add(sky::create_themed_sky_texture(theme, true)),
                )
            })
            .clone()
    }
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
use bevy::render::render_resource::TextureUsages;
use bevy::{asset::AssetPath, gltf::GltfAssetLabel};

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
    bevy::asset::load_embedded_asset!(assets, "../../assets/branding/stadium/sponsor-ribbon.png")
}

macro_rules! team_crests {
    (
        $( $asset_path:literal => $embed_path:literal ),* $(,)?
    ) => {
        fn register_team_crest_assets(app: &mut App) {
            $(
                bevy::asset::embedded_asset!(app, $embed_path);
            )*
        }

        pub fn load_team_crest(assets: &AssetServer, asset_path: &str) -> Handle<Image> {
            match asset_path {
                $(
                    $asset_path => bevy::asset::load_embedded_asset!(assets, $embed_path),
                )*
                _ => bevy::asset::load_embedded_asset!(
                    assets,
                    "../../assets/branding/teams/ind.png"
                ),
            }
        }
    };
}

team_crests! {
    "branding/teams/ind.png" => "../../assets/branding/teams/ind.png",
    "branding/teams/aus.png" => "../../assets/branding/teams/aus.png",
    "branding/teams/eng.png" => "../../assets/branding/teams/eng.png",
    "branding/teams/pak.png" => "../../assets/branding/teams/pak.png",
    "branding/teams/rsa.png" => "../../assets/branding/teams/rsa.png",
    "branding/teams/nzl.png" => "../../assets/branding/teams/nzl.png",
    "branding/teams/wis.png" => "../../assets/branding/teams/wis.png",
    "branding/teams/lka.png" => "../../assets/branding/teams/lka.png",
    "branding/teams/bgd.png" => "../../assets/branding/teams/bgd.png",
    "branding/teams/afg.png" => "../../assets/branding/teams/afg.png",
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
        bevy::asset::embedded_asset!(app, "../../assets/branding/stadium/sponsor-ribbon.png");
        register_team_crest_assets(app);
        bevy::asset::embedded_asset!(
            app,
            "../../assets/textures/stadium/outfield-grass-albedo-v2.png"
        );
        // Shared mocap locomotion graph (idle/run) for every figure.
        player::build_locomotion_clips(app);
        app.add_systems(
            Update,
            (
                player::disable_figure_frustum_culling,
                player::tag_skeleton_bones,
                player::apply_team_kit_materials,
                player::attach_animation_players,
                player::animate_figures,
            ),
        )
        .add_systems(PostUpdate, player::strip_skeleton_root_motion);
    }
}

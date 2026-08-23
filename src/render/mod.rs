pub mod camera_rig;
pub mod crowd;
pub mod environment;
pub mod outfield_grass;
pub mod player;
pub mod ring_geometry;
pub mod sky;
pub mod stadium;
pub mod stand_geometry;

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

/// Scene root whose imported glTF materials need correcting for this renderer.
///
/// Two things are wrong with the kits as shipped. They are exported from Unity
/// with `metallicFactor` left at the glTF default of 1.0, and a fully metallic
/// surface has no diffuse albedo — it only mirrors its surroundings. And where
/// a model carries no texture, its `baseColorFactor` was authored as the colour
/// the artist wanted to *see*, not as an albedo; under this scene's exposure
/// that arrives several stops hot. Together they turned the pines and rocks
/// into pale cutouts of the sky.
#[derive(Component)]
pub struct ImportedProp;

/// Mesh already visited by [`retune_imported_prop_materials`].
#[derive(Component)]
pub struct PropMaterialFixed;

/// Imported materials already corrected.
///
/// glTF materials are shared by every instance of a model, so this has to be
/// keyed by the material and not by the mesh that led us to it: with a hundred
/// pines on screen, a per-mesh guard still re-corrects the one shared foliage
/// material a hundred times, and the albedo compounds to black.
#[derive(Resource, Default)]
pub struct RetunedPropMaterials(std::collections::HashSet<AssetId<StandardMaterial>>);

/// A prop mesh not yet de-metallised: entity plus its imported material.
type UnfixedPropMesh<'a> = (Entity, &'a MeshMaterial3d<StandardMaterial>);
/// Only mesh entities we have not already visited.
type UnfixedPropMeshFilter = (With<Mesh3d>, Without<PropMaterialFixed>);

/// How much of flat ground's sun a standing prop actually catches.
///
/// `day_albedo` inverts the response of ground facing the sky. A tree or a rock
/// presents mostly vertical faces to a sun 56° up, so correcting it by the full
/// ground response takes the foliage down to near-black. Four tenths is about
/// what the visible faces of an upright convex prop average.
const PROP_SUN_FRACTION: f32 = 0.4;

/// Correct imported kit materials, once per mesh.
///
/// glTF materials are shared per asset, so the first instance of a palm fixes
/// the material for every other palm; the marker just stops us rescanning.
pub fn retune_imported_prop_materials(
    mut commands: Commands,
    props: Query<(), With<ImportedProp>>,
    parents: Query<&ChildOf>,
    meshes: Query<UnfixedPropMesh, UnfixedPropMeshFilter>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut done: ResMut<RetunedPropMaterials>,
) {
    for (entity, mat_handle) in &meshes {
        let mut current = parents.get(entity).ok().map(ChildOf::parent);
        let mut is_prop = false;
        for _ in 0..24 {
            let Some(parent) = current else { break };
            if props.contains(parent) {
                is_prop = true;
                break;
            }
            current = parents.get(parent).ok().map(ChildOf::parent);
        }
        if !is_prop {
            continue;
        }
        commands.entity(entity).insert(PropMaterialFixed);
        if !done.0.insert(mat_handle.0.id()) {
            continue;
        }
        if let Some(material) = materials.get_mut(&mat_handle.0) {
            if material.metallic > 0.5 {
                material.metallic = 0.0;
                // The same export pins roughness at 1.0, which kills every
                // highlight. Bark, stone and foliage all sit below that.
                material.perceptual_roughness = material.perceptual_roughness.min(0.85);
                material.reflectance = 0.18;
            }
            // Only the untextured kits carry their colour in the factor. Where
            // a texture supplies it (the city blocks, the crowd characters) the
            // factor is plain white and remapping it would just dim the map.
            if material.base_color_texture.is_none() {
                let srgb = material.base_color.to_srgba();
                let albedo = environment::day_albedo([srgb.red, srgb.green, srgb.blue]);
                let k = 1.0 / PROP_SUN_FRACTION;
                material.base_color =
                    Color::linear_rgba(albedo[0] * k, albedo[1] * k, albedo[2] * k, srgb.alpha);
            }
        }
    }
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
        app.init_resource::<RetunedPropMaterials>();
        app.add_systems(
            Update,
            (
                player::disable_figure_frustum_culling,
                player::tag_skeleton_bones,
                player::apply_team_kit_materials,
                player::attach_animation_players,
                player::animate_figures,
                retune_imported_prop_materials,
            ),
        )
        .add_systems(PostUpdate, player::strip_skeleton_root_motion);
    }
}

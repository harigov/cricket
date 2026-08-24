//! Player figures – Xbot glTF (Mixamo rig) driven by a hybrid animation
//! system: real mocap clips (`idle`, `run`) for locomotion via
//! `AnimationPlayer`, and hand-authored keyframed poses (batting stance,
//! bat swing, bowling action, throw) applied directly to the bones with
//! smooth blending in between.
//!
//! Also owns kit realism: helmets/caps, batting pads, gloves, a two-piece
//! bat and soft blob contact shadows under every figure.

use bevy::animation::AnimationPlayer;
use bevy::animation::graph::{AnimationGraph, AnimationGraphHandle, AnimationNodeIndex};
use bevy::animation::transition::AnimationTransitions;
use bevy::gltf::{GltfAssetLabel, GltfMaterialName};
use std::time::Duration;

use bevy::camera::visibility::NoFrustumCulling;
use bevy::prelude::*;

use crate::core::ShotKind;
use crate::core::teams::{KitStyle, Team};
use crate::render::crowd;
use crate::render::kit::{self, ShirtSpec};

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Figure {
    pub kind: FigureKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FigureKind {
    Batter,
    NonStriker,
    Bowler,
    Keeper,
    Fielder(usize),
    Umpire,
}
impl FigureKind {
    /// Batters and keepers wear helmets; everyone else gets a cap.
    pub fn wears_helmet(self) -> bool {
        matches!(
            self,
            FigureKind::Batter | FigureKind::NonStriker | FigureKind::Keeper
        )
    }
}

#[derive(Component, Default)]
pub struct Anim {
    pub state: AnimState,
}

#[derive(Clone, Copy, Debug, Default)]
pub enum AnimState {
    #[default]
    Idle,
    Run {
        t: f32,
    },
    BowlAction {
        p: f32,
    },
    /// Ease from delivery follow-through back to standing idle.
    BowlSettle {
        t: f32,
    },
    BatSwing {
        p: f32,
    },
    /// A specific stroke: same 0..1 swing progress as [`AnimState::BatSwing`]
    /// but with the footwork and bat arc chosen by the shot the batter played.
    BatShot {
        p: f32,
        shot: ShotKind,
    },
    /// Batter settled in their stance at the crease, waiting on the bowler.
    Stance,
    Throw {
        p: f32,
    },
}

#[derive(Component, Debug)]
pub struct Bone {
    pub figure: Entity,
    pub kind: BoneKind,
}

/// Imported glTF local rotation captured when the bone is first tagged.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct SkeletonBone {
    /// Bind-pose local translation, restored every frame so the retarget of the
    /// Xbot clips is rotation-only. See `strip_skeleton_root_motion`.
    pub bind_translation: Vec3,
}

/// Bind-pose rotation and translation for a bone the procedural poses drive.
#[derive(Component)]
pub(crate) struct BoneBindPose {
    /// Procedural pose deltas compose onto this (see `compose_pose_rotation`).
    pub rotation: Quat,
    /// Bone's bind-pose rotation in armature space, used to re-express the
    /// pose library in this rig's bone axes. See `compose_pose_rotation`.
    pub world_rotation: Quat,
    /// Re-pinned onto the hips every frame by `strip_skeleton_root_motion`.
    /// Captured per figure rather than hard-coded, because each generated
    /// archetype has its own hip height — and because the legacy Xbot armature
    /// is centimetre-scaled while the generated ones are in metres.
    pub translation: Vec3,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoneKind {
    Hips,
    Spine,
    Spine1,
    Spine2,
    Neck,
    Head,
    LeftShoulder,
    LeftArm,
    LeftForeArm,
    LeftHand,
    RightShoulder,
    RightArm,
    RightForeArm,
    RightHand,
    LeftUpLeg,
    LeftLeg,
    LeftFoot,
    RightUpLeg,
    RightLeg,
    RightFoot,
}
fn bone_kind_for_name(name: &str) -> Option<BoneKind> {
    // glTF node names are `mixamorig:RightHand`; match the bone suffix.
    let name = name.rsplit(':').next().unwrap_or(name);
    match name {
        "Hips" => Some(BoneKind::Hips),
        "Spine" => Some(BoneKind::Spine),
        "Spine1" => Some(BoneKind::Spine1),
        "Spine2" => Some(BoneKind::Spine2),
        "Neck" => Some(BoneKind::Neck),
        "Head" => Some(BoneKind::Head),
        "LeftShoulder" => Some(BoneKind::LeftShoulder),
        "LeftArm" => Some(BoneKind::LeftArm),
        "LeftForeArm" => Some(BoneKind::LeftForeArm),
        "LeftHand" => Some(BoneKind::LeftHand),
        "RightShoulder" => Some(BoneKind::RightShoulder),
        "RightArm" => Some(BoneKind::RightArm),
        "RightForeArm" => Some(BoneKind::RightForeArm),
        "RightHand" => Some(BoneKind::RightHand),
        "LeftUpLeg" => Some(BoneKind::LeftUpLeg),
        "LeftLeg" => Some(BoneKind::LeftLeg),
        "LeftFoot" => Some(BoneKind::LeftFoot),
        "RightUpLeg" => Some(BoneKind::RightUpLeg),
        "RightLeg" => Some(BoneKind::RightLeg),
        "RightFoot" => Some(BoneKind::RightFoot),
        _ => None,
    }
}

/// Links an auto-spawned `AnimationPlayer` back to its owning figure.
#[derive(Component)]
pub struct PlayerOf(pub Entity);

/// Which locomotion clip is currently playing on a player entity.
#[derive(Component, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClipState {
    #[default]
    None,
    Idle,
    Run,
}

/// Animation graph + node indices for the bundled mocap clips.
#[derive(Resource, Clone)]
pub struct LocomotionClips {
    pub graph: Handle<AnimationGraph>,
    pub idle: AnimationNodeIndex,
    pub run: AnimationNodeIndex,
}

/// Embedded path of the character asset.
pub fn xbot_asset_path() -> bevy::asset::AssetPath<'static> {
    let path = bevy::asset::embedded_path!("../../assets/characters/Xbot.glb");
    bevy::asset::AssetPath::from_path_buf(path).with_source("embedded")
}

/// Build the shared animation graph once at startup. Clip indices match the
/// glTF animation order: [agree, headShake, idle, run, sad_pose, sneak_pose, walk].
pub fn build_locomotion_clips(app: &mut App) {
    let assets = app.world().resource::<AssetServer>();
    let idle_clip = assets.load(GltfAssetLabel::Animation(2).from_asset(xbot_asset_path()));
    let run_clip = assets.load(GltfAssetLabel::Animation(3).from_asset(xbot_asset_path()));
    let (graph, nodes) = AnimationGraph::from_clips([idle_clip, run_clip]);
    let graph = app
        .world_mut()
        .resource_mut::<Assets<AnimationGraph>>()
        .add(graph);
    app.insert_resource(LocomotionClips {
        graph,
        idle: nodes[0],
        run: nodes[1],
    });
}

#[derive(Component)]
pub struct Bat;

/// Cricket gear we spawn ourselves (bat, gloves, helmet, cap, pads, crest).
/// The kit recolouring pass must skip these: it classifies unnamed meshes by
/// base colour, and willow/white gear otherwise gets repainted in team colours.
#[derive(Component)]
pub(crate) struct Equipment;

/// Team colours carried by the figure root while its glTF scene streams in.
#[derive(Component, Clone)]
pub struct TeamKit {
    primary_color: Color,
    secondary_color: Color,
    kit_style: KitStyle,
    crest: Handle<StandardMaterial>,
    /// Short team code (e.g. `"IND"`), kept for deterministic per-player skin
    /// tone assignment — see [`player_skin_seed`].
    team_short: String,
    /// Player name shown across the upper back of a named-slot `Shirt` mesh.
    /// `None` on every figure today: no call site threads a roster identity
    /// through [`spawn_figure`] yet, so the name row simply stays blank
    /// until one does.
    player_name: Option<String>,
    /// Squad number shown large on the back of a named-slot `Shirt` mesh.
    /// `None` for the same reason as `player_name`.
    squad_number: Option<u8>,
}

/// Crest badge already parented to the chest bone.
#[derive(Component)]
pub(crate) struct CrestAttached;

/// Lift the streamed scene so the asset's soles rest on the pitch.
///
/// The generated archetypes are authored ground-flat: `build_player_asset.py`
/// clamps the shoe soles to z = 0, leaving every mesh at y >= -0.004 m, so no
/// correction is needed. Kept as a named constant because the legacy Xbot
/// asset did need one and a silent ground offset is easy to reintroduce.
const SCENE_GROUND_Y: f32 = 0.0;

/// MPFB exports the armature in metres with the root at `scale = 1.0`, so a
/// bone's local units *are* metres and equipment needs no rescaling.
///
/// The legacy Xbot armature was the opposite — centimetre bone translations
/// under a `scale = 0.01` root (`mixamorig:Hips` y = 103.99) — which is why
/// this indirection exists. Every equipment size and offset routes through
/// [`metres_to_bone`], so this one constant rescales all of them together.
const BONE_UNITS_PER_METRE: f32 = 1.0;

fn metres_to_bone(metres: f32) -> f32 {
    metres * BONE_UNITS_PER_METRE
}

fn bone_vec3_m(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(metres_to_bone(x), metres_to_bone(y), metres_to_bone(z))
}

/// Build a child-of-bone [`Transform`] from metre-space translation/rotation.
fn equipment_transform_m(translation_m: Vec3, rotation: Quat) -> Transform {
    Transform::from_translation(bone_vec3_m(
        translation_m.x,
        translation_m.y,
        translation_m.z,
    ))
    .with_rotation(rotation)
}

fn equipment_transform_m_scaled(translation_m: Vec3, rotation: Quat, scale: Vec3) -> Transform {
    equipment_transform_m(translation_m, rotation).with_scale(scale)
}

/// Imported Xbot root faces **+Z** in world space when Y rotation is zero.
pub const MODEL_FORWARD_XZ: Vec2 = Vec2::new(0.0, 1.0);

#[derive(Component)]
pub struct KitStyled;

/// Classify an imported Xbot body mesh from its glTF node name or the default
/// salmon / brown PBR colours baked into the asset.
fn kit_mesh_kind(mesh_name: &str, mat: &StandardMaterial) -> Option<KitMeshKind> {
    if mesh_name.contains("Joint") || mesh_name.contains("Joints_MAT") {
        return Some(KitMeshKind::Joints);
    }
    if mesh_name.contains("Surface")
        || mesh_name.contains("HighLimbs")
        || mesh_name.contains("GeoSG")
    {
        return Some(KitMeshKind::Surface);
    }
    let c = mat.base_color.to_srgba();
    if c.red > 0.70 && c.green > 0.22 && c.blue > 0.18 {
        Some(KitMeshKind::Surface)
    } else if c.red < 0.45 && c.green < 0.16 && c.blue < 0.14 {
        Some(KitMeshKind::Joints)
    } else {
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KitMeshKind {
    Surface,
    Joints,
}

// ---------------------------------------------------------------------------
// Named glTF material slots (the future MPFB body/kit asset)
// ---------------------------------------------------------------------------

/// A mesh's glTF material slot, when the asset names it explicitly instead of
/// relying on baked colour classification (see [`kit_mesh_kind`]). The
/// upcoming realistic-human asset carries all four on one Mixamo-named
/// armature; the legacy Xbot asset carries none, so this path only ever
/// engages once that asset lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NamedKitSlot {
    Skin,
    Shirt,
    Pants,
    Shoes,
    Hair,
}

fn named_kit_slot(material_slot_name: &str) -> Option<NamedKitSlot> {
    match material_slot_name {
        "Skin" => Some(NamedKitSlot::Skin),
        "Shirt" => Some(NamedKitSlot::Shirt),
        "Pants" => Some(NamedKitSlot::Pants),
        "Shoes" => Some(NamedKitSlot::Shoes),
        "Hair" => Some(NamedKitSlot::Hair),
        _ => None,
    }
}

/// Skin tones players are drawn from — a richer spread than the crowd's 6
/// tones ([`crowd::crowd_skin_color`]) covering caucasian, south-asian and
/// african ranges, since players are seen at much closer camera distance.
const PLAYER_SKIN_TONES: [Color; 12] = [
    Color::srgb(0.980_4, 0.878_4, 0.784_3), // FA E0 C8 - pale
    Color::srgb(0.945_1, 0.788_2, 0.615_7), // F1 C9 9D
    Color::srgb(0.909_8, 0.721_6, 0.556_9), // E8 B8 8E
    Color::srgb(0.866_7, 0.650_9, 0.478_4), // DD A6 7A - south asian mid
    Color::srgb(0.776_5, 0.545_1, 0.368_6), // C6 8B 5E
    Color::srgb(0.690_2, 0.462_7, 0.298_0), // B0 76 4C
    Color::srgb(0.603_9, 0.403_9, 0.270_6), // 9A 67 45
    Color::srgb(0.541_2, 0.352_9, 0.223_5), // 8A 5A 39 - south asian deep
    Color::srgb(0.462_7, 0.305_9, 0.215_7), // 76 4E 37
    Color::srgb(0.360_8, 0.227_5, 0.129_4), // 5C 3A 21 - african mid
    Color::srgb(0.270_6, 0.168_6, 0.094_1), // 45 2B 18
    Color::srgb(0.188_2, 0.113_7, 0.062_7), // 30 1D 10 - african deep
];

/// Hair tones players are drawn from, independent of skin tone.
const PLAYER_HAIR_TONES: [Color; 6] = [
    Color::srgb(0.070_6, 0.070_6, 0.070_6), // 12 12 12 - black
    Color::srgb(0.180_4, 0.121_6, 0.078_4), // 2E 1F 14 - dark brown
    Color::srgb(0.325_5, 0.223_5, 0.145_1), // 53 39 25 - brown
    Color::srgb(0.545_1, 0.396_1, 0.231_4), // 8B 65 3B - light brown
    Color::srgb(0.792_2, 0.647_1, 0.376_5), // CA A5 60 - blonde
    Color::srgb(0.454_9, 0.454_9, 0.462_7), // 74 74 76 - grey
];

/// Shared skin-tone and hair materials for player figures — one handle per
/// tone, reused across every figure via [`player_skin_tone_index`] and
/// [`player_hair_tone_index`]. Mirrors `crowd::CrowdPalette` (shared
/// handles, no per-player clones).
#[derive(Resource)]
pub struct PlayerSkinPalette {
    pub skin: Vec<Handle<StandardMaterial>>,
    pub hair: Vec<Handle<StandardMaterial>>,
}

/// Build the shared player skin/hair palette once at app startup.
pub fn build_player_skin_palette(materials: &mut Assets<StandardMaterial>) -> PlayerSkinPalette {
    let make = |materials: &mut Assets<StandardMaterial>, c: Color, roughness: f32| {
        materials.add(StandardMaterial {
            base_color: c,
            perceptual_roughness: roughness,
            metallic: 0.0,
            reflectance: 0.05,
            ..Default::default()
        })
    };
    PlayerSkinPalette {
        skin: PLAYER_SKIN_TONES
            .iter()
            .map(|&c| make(materials, c, 0.75))
            .collect(),
        hair: PLAYER_HAIR_TONES
            .iter()
            .map(|&c| make(materials, c, 0.55))
            .collect(),
    }
}

pub fn init_player_skin_palette(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(build_player_skin_palette(&mut materials));
}

/// Every generated body archetype, in the order
/// `scripts/build_player_asset.py` emits them (height x build x ancestry).
///
/// Ancestry here varies body and facial proportion only — visible skin tone is
/// an independent runtime tint from [`PLAYER_SKIN_TONES`], so any archetype can
/// wear any tone.
pub const PLAYER_ARCHETYPES: [&str; 27] = [
    "short_thin_caucasian",
    "short_thin_south_asian",
    "short_thin_african",
    "short_regular_caucasian",
    "short_regular_south_asian",
    "short_regular_african",
    "short_heavy_caucasian",
    "short_heavy_south_asian",
    "short_heavy_african",
    "medium_thin_caucasian",
    "medium_thin_south_asian",
    "medium_thin_african",
    "medium_regular_caucasian",
    "medium_regular_south_asian",
    "medium_regular_african",
    "medium_heavy_caucasian",
    "medium_heavy_south_asian",
    "medium_heavy_african",
    "tall_thin_caucasian",
    "tall_thin_south_asian",
    "tall_thin_african",
    "tall_regular_caucasian",
    "tall_regular_south_asian",
    "tall_regular_african",
    "tall_heavy_caucasian",
    "tall_heavy_south_asian",
    "tall_heavy_african",
];

/// Pick a body archetype for a per-player seed from [`player_skin_seed`].
///
/// Mixed with a different salt than the skin and hair tones so a player's build
/// and colouring vary independently rather than moving in lockstep.
pub fn archetype_for_seed(seed: u32) -> &'static str {
    PLAYER_ARCHETYPES[(crowd::mix_hash(seed, 31) as usize) % PLAYER_ARCHETYPES.len()]
}

/// Deterministic per-player seed: same team + role always resolves to the
/// same skin tone, without storing a persistent per-figure random pick.
pub fn player_skin_seed(team_short: &str, kind: FigureKind) -> u32 {
    let mut h: u32 = 0x9E37_79B1;
    for b in team_short.bytes() {
        h = crowd::mix_hash(h, b as u32);
    }
    let kind_tag: u32 = match kind {
        FigureKind::Batter => 1,
        FigureKind::NonStriker => 2,
        FigureKind::Bowler => 3,
        FigureKind::Keeper => 4,
        FigureKind::Fielder(slot) => 100 + slot as u32,
        FigureKind::Umpire => 5,
    };
    crowd::mix_hash(h, kind_tag)
}

/// Skin palette index for a per-player seed from [`player_skin_seed`].
pub fn player_skin_tone_index(seed: u32) -> usize {
    (crowd::mix_hash(seed, 7) as usize) % PLAYER_SKIN_TONES.len()
}

/// Hair palette index for a per-player seed from [`player_skin_seed`] — a
/// different salt than [`player_skin_tone_index`] so skin tone and hair
/// colour don't move in lockstep.
pub fn player_hair_tone_index(seed: u32) -> usize {
    (crowd::mix_hash(seed, 13) as usize) % PLAYER_HAIR_TONES.len()
}

/// Simple team-coloured material for the `Pants`/`Shoes` named slots. These
/// aren't a parameterizable deliverable like the shirt — cricket trousers
/// and boots are almost always plain — so they just tint from the kit.
fn named_slot_solid(
    materials: &mut Assets<StandardMaterial>,
    color: Color,
    roughness: f32,
) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: color,
        perceptual_roughness: roughness,
        metallic: 0.0,
        reflectance: 0.05,
        ..Default::default()
    })
}

/// Cricket trousers are almost always white/cream regardless of kit colour;
/// only the trim reads as team colour on the real garment. Boots follow suit.
const KIT_TROUSER_COLOR: Color = Color::srgb(0.933_3, 0.929_4, 0.898_0); // F0 ED E5
const KIT_BOOT_COLOR: Color = Color::srgb(0.964_7, 0.964_7, 0.945_1); // F6 F6 F1

/// Resolve the shared/generated material for one named glTF kit slot.
fn named_slot_material(
    slot: NamedKitSlot,
    kit: &TeamKit,
    fig_kind: FigureKind,
    skin_palette: Option<&PlayerSkinPalette>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) -> Option<Handle<StandardMaterial>> {
    match slot {
        NamedKitSlot::Skin => {
            let palette = skin_palette?;
            let seed = player_skin_seed(&kit.team_short, fig_kind);
            let idx = player_skin_tone_index(seed);
            palette.skin.get(idx).cloned()
        }
        NamedKitSlot::Hair => {
            let palette = skin_palette?;
            let seed = player_skin_seed(&kit.team_short, fig_kind);
            let idx = player_hair_tone_index(seed);
            palette.hair.get(idx).cloned()
        }
        NamedKitSlot::Shirt => {
            let mut spec = ShirtSpec::new(kit.primary_color, kit.secondary_color, kit.kit_style);
            if let Some(name) = &kit.player_name {
                spec = spec.with_name(name.clone());
            }
            if let Some(number) = kit.squad_number {
                spec = spec.with_number(number);
            }
            // The 3D chest-crest badge (see `attach_chest_crest`) already
            // puts the sponsor mark on the model; baking it into the shirt
            // texture too would double it up, so the composited crest stays
            // unused here (`None`) until that badge is retired in favour of
            // the texture.
            let image = kit::build_shirt_image(&spec, None);
            Some(materials.add(StandardMaterial {
                base_color_texture: Some(images.add(image)),
                perceptual_roughness: 0.92,
                metallic: 0.0,
                reflectance: 0.06,
                ..Default::default()
            }))
        }
        NamedKitSlot::Pants => Some(named_slot_solid(materials, KIT_TROUSER_COLOR, 0.88)),
        NamedKitSlot::Shoes => Some(named_slot_solid(materials, KIT_BOOT_COLOR, 0.7)),
    }
}

/// Marker: frustum culling was disabled on this figure mesh.
#[derive(Component)]
pub(crate) struct CullingFixed;

// Legacy – kept so old queries don't break, but no longer spawned.
#[derive(Component)]
pub struct Part {
    pub kind: PartKind,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartKind {
    LegL,
    LegR,
    ArmL,
    ArmR,
}

pub fn spawn_figure(
    commands: &mut Commands,
    asset_server: &AssetServer,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    pos: Vec3,
    yaw: f32,
    team: &Team,
    kind: FigureKind,
) -> Entity {
    let archetype = archetype_for_seed(player_skin_seed(&team.short, kind));
    let scene = crate::render::load_player_scene(asset_server, archetype);
    let crest_mat = materials.add(StandardMaterial {
        base_color_texture: Some(crate::render::load_team_crest(
            asset_server,
            &team.crest_asset(),
        )),
        perceptual_roughness: 0.70,
        unlit: true,
        cull_mode: None,
        ..Default::default()
    });
    let fig = commands
        .spawn((
            Figure { kind },
            Anim::default(),
            TeamKit {
                primary_color: team.primary_color,
                secondary_color: team.secondary_color,
                kit_style: team.kit_style,
                crest: crest_mat.clone(),
                team_short: team.short.clone(),
                player_name: None,
                squad_number: None,
            },
            Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(yaw)),
            Visibility::default(),
            InheritedVisibility::default(),
            ViewVisibility::default(),
        ))
        .id();
    // Soft blob contact shadow – grounds every figure visually.
    let blob_mesh = meshes.add(Circle::new(0.62));
    let blob_mat = materials.add(StandardMaterial {
        base_color_texture: Some(images.add(blob_shadow_image())),
        base_color: Color::srgba(0.0, 0.0, 0.0, 0.42),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        cull_mode: None,
        ..Default::default()
    });
    commands.entity(fig).with_children(|p| {
        // Archetypes are authored ground-flat (see SCENE_GROUND_Y).
        p.spawn((
            SceneRoot(scene),
            Transform::from_xyz(0.0, SCENE_GROUND_Y, 0.0),
            Visibility::default(),
            InheritedVisibility::default(),
            ViewVisibility::default(),
        ));
        p.spawn((
            Mesh3d(blob_mesh),
            MeshMaterial3d(blob_mat),
            Transform::from_xyz(0.0, 0.02, 0.0)
                .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
        ));
    });
    fig
}

/// Procedural jersey pattern keyed by team kit style. Mixamo Xbot UVs are a
/// per-part atlas (not head-to-toe in V), so every texel is dyed from team
/// colours; helmets/caps cover the head on the pitch.
fn kit_pattern_image(style: KitStyle, primary: Color, secondary: Color) -> Image {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    const S: u32 = 64;
    let p = primary.to_srgba();
    let s = secondary.to_srgba();
    let mut data = Vec::with_capacity((S * S * 4) as usize);
    for y in 0..S {
        for x in 0..S {
            let u = x as f32 / S as f32;
            let v = y as f32 / S as f32;
            let use_secondary = match style {
                KitStyle::Solid => false,
                KitStyle::VerticalStripes => (x / 4) % 2 == 0,
                KitStyle::HorizontalBand => v > 0.28 && v < 0.52,
                KitStyle::Chevron => v < 0.22 + (u - 0.5).abs() * 0.55,
                KitStyle::DiagonalSplit => u + v * 0.85 > 1.05,
                KitStyle::Hoops => (y / 6) % 2 == 0,
            };
            let (r, g, b) = if use_secondary {
                (s.red, s.green, s.blue)
            } else {
                (p.red, p.green, p.blue)
            };
            data.extend_from_slice(&[(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8, 255]);
        }
    }
    Image::new(
        Extent3d {
            width: S,
            height: S,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

fn blob_shadow_image() -> Image {
    use bevy::asset::RenderAssetUsages;

    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    const S: u32 = 64;
    let mut data = Vec::with_capacity((S * S * 4) as usize);
    let c = (S as f32 - 1.0) / 2.0;
    for y in 0..S {
        for x in 0..S {
            let dx = x as f32 - c;
            let dy = y as f32 - c;
            let r = ((dx * dx + dy * dy).sqrt() / c).clamp(0.0, 1.0);
            let a = (1.0 - r * r).powf(1.8);
            data.extend_from_slice(&[0, 0, 0, (a * 255.0) as u8]);
        }
    }
    Image::new(
        Extent3d {
            width: S,
            height: S,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// Skinned glTF meshes get bind-pose AABBs that sit near the armature root
/// (feet). Disable frustum culling once per streamed mesh so close cameras
/// still draw the posed body when only the origin lies outside the frustum.
pub fn disable_figure_frustum_culling(
    mut commands: Commands,
    figures: Query<(), With<Figure>>,
    parents: Query<&ChildOf>,
    meshes: Query<Entity, (With<Mesh3d>, Without<CullingFixed>)>,
) {
    for entity in &meshes {
        let mut current = parents.get(entity).ok().map(ChildOf::parent);
        let mut is_figure_mesh = false;
        for _ in 0..24 {
            let Some(parent) = current else {
                break;
            };
            if figures.contains(parent) {
                is_figure_mesh = true;
                break;
            }
            current = parents.get(parent).ok().map(ChildOf::parent);
        }
        if !is_figure_mesh {
            continue;
        }
        commands
            .entity(entity)
            .insert((NoFrustumCulling, CullingFixed));
    }
}

/// Mesh rows still awaiting a team-kit tint: entity, optional glTF node name,
/// optional glTF material *slot* name (set when the asset names its
/// materials, e.g. `Skin`/`Shirt`/`Pants`/`Shoes`) and the PBR material
/// handle imported with the figure.
type UnstyledKitMesh<'a> = (
    Entity,
    Option<&'a Name>,
    Option<&'a GltfMaterialName>,
    &'a MeshMaterial3d<StandardMaterial>,
);
/// Only untinted figure meshes — equipment (bat, pads) is recoloured elsewhere.
type UnstyledKitMeshFilter = (Without<KitStyled>, With<Mesh3d>, Without<Equipment>);

/// Keep the imported PBR materials and tint them into believable cricket kit.
///
/// Two paths, tried in order:
/// - **Named slots** (the future MPFB asset): a mesh whose glTF material slot
///   is literally named `Skin`/`Shirt`/`Pants`/`Shoes` gets styled from that
///   name directly — see [`named_kit_slot`].
/// - **Legacy classification** (today's Xbot asset, which names none of its
///   slots): `Beta_Surface` becomes the long-sleeve jersey/trousers (stronger
///   primary tint), `Beta_Joints` takes the secondary colour as trim/helmet
///   shade, both inferred from the mesh's baked colour by [`kit_mesh_kind`].
#[allow(clippy::too_many_arguments)]
pub fn apply_team_kit_materials(
    mut commands: Commands,
    kits: Query<&TeamKit>,
    figures: Query<&Figure>,
    skin_palette: Option<Res<PlayerSkinPalette>>,
    parents: Query<&ChildOf>,
    meshes: Query<UnstyledKitMesh, UnstyledKitMeshFilter>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    for (entity, name, mat_name, mat_handle) in &meshes {
        let mut cur = parents.get(entity).ok().map(ChildOf::parent);
        let mut fig_ent = None;
        for _ in 0..32 {
            let Some(parent) = cur else { break };
            if kits.contains(parent) {
                fig_ent = Some(parent);
                break;
            }
            cur = parents.get(parent).ok().map(ChildOf::parent);
        }
        let Some(fig_ent) = fig_ent else { continue };
        let Ok(kit) = kits.get(fig_ent) else { continue };

        if let Some(slot) = mat_name.and_then(|n| named_kit_slot(n.as_ref())) {
            let fig_kind = figures.get(fig_ent).ok().map(|f| f.kind);
            if let Some(handle) = fig_kind.and_then(|fig_kind| {
                named_slot_material(
                    slot,
                    kit,
                    fig_kind,
                    skin_palette.as_deref(),
                    &mut materials,
                    &mut images,
                )
            }) {
                commands
                    .entity(entity)
                    .insert((MeshMaterial3d(handle), KitStyled));
                continue;
            }
        }

        let Some(mut mat) = materials.get(&mat_handle.0).cloned() else {
            continue;
        };
        let mesh_name = name.map(|n| n.as_str()).unwrap_or("");
        let Some(kind) = kit_mesh_kind(mesh_name, &mat) else {
            continue;
        };
        let team_col = if kind == KitMeshKind::Joints {
            kit.secondary_color
        } else {
            kit.primary_color
        };
        let base_srgba = team_col.to_srgba();
        let orig = mat.base_color.to_srgba();
        if kind == KitMeshKind::Surface {
            if kit.kit_style == KitStyle::Solid {
                // Solid kits tint the whole mesh — reads clearly on broadcast cameras.
                mat.base_color_texture = None;
                mat.base_color = kit.primary_color;
            } else {
                mat.base_color_texture = Some(images.add(kit_pattern_image(
                    kit.kit_style,
                    kit.primary_color,
                    kit.secondary_color,
                )));
                mat.base_color = Color::WHITE;
            }
        } else {
            let lerp = 0.72;
            mat.base_color = Color::srgba(
                orig.red * (1.0 - lerp) + base_srgba.red * lerp,
                orig.green * (1.0 - lerp) + base_srgba.green * lerp,
                orig.blue * (1.0 - lerp) + base_srgba.blue * lerp,
                1.0,
            );
        }
        mat.perceptual_roughness = if kind == KitMeshKind::Joints {
            0.90
        } else {
            0.94
        };
        mat.metallic = 0.0;
        mat.reflectance = if kind == KitMeshKind::Joints {
            0.08
        } else {
            0.06
        };
        let cloned = materials.add(mat);
        commands
            .entity(entity)
            .insert((MeshMaterial3d(cloned), KitStyled));
    }
}

/// Tag newly spawned Mixamo bones with [`Bone`] and attach cricket equipment:
/// helmet/cap on the head, pads on the shins, gloves on both hands and a
/// two-piece bat for batters.
#[allow(clippy::type_complexity)]
pub fn tag_skeleton_bones(
    mut commands: Commands,
    figures: Query<(Entity, &Figure)>,
    crest_done: Query<(), With<CrestAttached>>,
    kits: Query<&TeamKit>,
    candidates: Query<(Entity, &Name, &Transform, Option<&ChildOf>), Without<Bone>>,
    parents: Query<&ChildOf>,
    transforms: Query<&Transform>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let figure_set: std::collections::HashSet<Entity> = figures.iter().map(|(e, _)| e).collect();
    let figure_kind: std::collections::HashMap<Entity, FigureKind> =
        figures.iter().map(|(e, f)| (e, f.kind)).collect();
    // Kit colours per figure (for helmet/cap shells).
    let figure_kit: std::collections::HashMap<Entity, (Color, Color)> = figures
        .iter()
        .filter_map(|(e, _)| {
            // TeamKit lives on the same entity as Figure.
            kits.get(e)
                .ok()
                .map(|k| (e, (k.primary_color, k.secondary_color)))
        })
        .collect();

    for (ent, name, transform, child_of) in &candidates {
        if !name.as_str().contains("mixamorig:") {
            continue;
        }
        let mut cur = child_of.map(|c| c.parent());
        let mut fig_ent = None;
        let mut steps = 0;
        while let Some(p) = cur {
            if figure_set.contains(&p) {
                fig_ent = Some(p);
                break;
            }
            if let Ok(child) = parents.get(p) {
                cur = Some(child.parent());
            } else {
                break;
            }
            steps += 1;
            if steps > 16 {
                break;
            }
        }
        let Some(fig) = fig_ent else { continue };
        // Every bone gets its bind translation recorded, even the ones no pose
        // targets — the imported clips animate all 67 of them.
        commands.entity(ent).insert(SkeletonBone {
            bind_translation: transform.translation,
        });
        let Some(kind) = bone_kind_for_name(name.as_str()) else {
            continue;
        };
        // Accumulate bind rotation up to (but excluding) the figure root, so
        // the figure's own yaw is not folded into the correction.
        let mut world_rotation = transform.rotation;
        let mut ancestor = child_of.map(|c| c.parent());
        let mut hops = 0;
        while let Some(a) = ancestor {
            if a == fig || hops > 16 {
                break;
            }
            if let Ok(tf) = transforms.get(a) {
                world_rotation = tf.rotation * world_rotation;
            }
            ancestor = parents.get(a).ok().map(ChildOf::parent);
            hops += 1;
        }
        commands.entity(ent).insert((
            Bone { figure: fig, kind },
            BoneBindPose {
                rotation: transform.rotation,
                world_rotation,
                translation: transform.translation,
            },
        ));

        let fk = figure_kind.get(&fig).copied();
        let kit_col = figure_kit.get(&fig).copied();
        let is_batter = matches!(fk, Some(FigureKind::Batter | FigureKind::NonStriker));
        let is_keeper = fk == Some(FigureKind::Keeper);

        match (kind, fk) {
            (BoneKind::RightHand, Some(FigureKind::Batter | FigureKind::NonStriker)) => {
                attach_bat(ent, &mut commands, &mut meshes, &mut materials);
                attach_glove(ent, &mut commands, &mut meshes, &mut materials, false);
            }
            (BoneKind::LeftHand, Some(FigureKind::Batter | FigureKind::NonStriker)) => {
                attach_glove(ent, &mut commands, &mut meshes, &mut materials, true);
            }
            (BoneKind::Head, Some(fk)) => {
                if fk.wears_helmet()
                    && let Some((primary, _)) = kit_col
                {
                    attach_helmet(ent, &mut commands, &mut meshes, &mut materials, primary);
                } else if !fk.wears_helmet()
                    && let Some((primary, _)) = kit_col
                {
                    attach_cap(ent, &mut commands, &mut meshes, &mut materials, primary);
                }
            }
            (BoneKind::LeftLeg | BoneKind::RightLeg, Some(_)) if is_batter || is_keeper => {
                attach_pad(ent, &mut commands, &mut meshes, &mut materials, is_keeper);
            }
            (BoneKind::Spine2, Some(_)) => {
                if crest_done.get(fig).is_err()
                    && let Ok(kit) = kits.get(fig)
                {
                    attach_chest_crest(ent, &mut commands, &mut meshes, kit.crest.clone());
                    commands.entity(fig).insert(CrestAttached);
                }
            }
            _ => {}
        }
    }
}

fn matte(
    materials: &mut Assets<StandardMaterial>,
    base_color: Color,
    perceptual_roughness: f32,
) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color,
        perceptual_roughness,
        ..Default::default()
    })
}

fn matte_reflectance(
    materials: &mut Assets<StandardMaterial>,
    base_color: Color,
    perceptual_roughness: f32,
    reflectance: f32,
) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color,
        perceptual_roughness,
        reflectance,
        ..Default::default()
    })
}

fn matte_shell(
    materials: &mut Assets<StandardMaterial>,
    base_color: Color,
    perceptual_roughness: f32,
    reflectance: f32,
) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color,
        perceptual_roughness,
        reflectance,
        metallic: 0.0,
        ..Default::default()
    })
}

fn spawn_mesh_child(
    parent: Entity,
    commands: &mut Commands,
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    transform: Transform,
) {
    commands.entity(parent).with_children(|p| {
        p.spawn((Equipment, Mesh3d(mesh), MeshMaterial3d(material), transform));
    });
}

fn attach_chest_crest(
    spine: Entity,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    crest: Handle<StandardMaterial>,
) {
    spawn_mesh_child(
        spine,
        commands,
        meshes.add(Rectangle::new(metres_to_bone(0.19), metres_to_bone(0.19))),
        crest,
        equipment_transform_m(Vec3::new(0.0, 0.11, 0.12), Quat::from_rotation_x(-0.08)),
    );
}

fn willow_mat(materials: &mut Assets<StandardMaterial>) -> Handle<StandardMaterial> {
    matte_reflectance(materials, Color::srgb_u8(0xE6, 0xD2, 0xA0), 0.72, 0.14)
}

fn attach_bat(
    hand: Entity,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let blade = meshes.add(Cuboid::new(
        metres_to_bone(0.11),
        metres_to_bone(0.60),
        metres_to_bone(0.046),
    ));
    let handle = meshes.add(Capsule3d::new(metres_to_bone(0.017), metres_to_bone(0.26)));
    let wood = willow_mat(materials);
    let grip_mat = matte(materials, Color::srgb_u8(0x24, 0x28, 0x30), 0.9);
    // Mixamo arm chain runs along local -X; mesh long axis is +Y. Map +Y onto
    // hand +Z so the blade hangs down beside the pads; slight yaw opens the
    // face toward the batting-end camera in the grounded stance grip.
    let swing = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2) * Quat::from_rotation_y(-0.06);
    let blade_tf = equipment_transform_m(Vec3::new(-0.04, 0.0, 0.34), swing);
    let handle_tf = equipment_transform_m(Vec3::new(-0.04, 0.0, -0.08), swing);
    commands.entity(hand).with_children(|p| {
        p.spawn((
            Bat,
            Equipment,
            Mesh3d(blade),
            MeshMaterial3d(wood.clone()),
            blade_tf,
        ));
        p.spawn((
            Equipment,
            Mesh3d(handle),
            MeshMaterial3d(grip_mat),
            handle_tf,
        ));
    });
}

fn attach_glove(
    hand: Entity,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    left: bool,
) {
    let glove = meshes.add(Sphere::new(metres_to_bone(0.062)).mesh().ico(2).unwrap());
    let mat = matte(materials, Color::srgb_u8(0xE8, 0xE2, 0xD2), 0.85);
    let x = if left { -0.03 } else { 0.03 };
    spawn_mesh_child(
        hand,
        commands,
        glove,
        mat,
        equipment_transform_m_scaled(
            Vec3::new(x, -0.01, 0.045),
            Quat::IDENTITY,
            Vec3::new(1.0, 1.25, 0.85),
        ),
    );
}

fn attach_helmet(
    head: Entity,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    primary: Color,
) {
    let s = primary.to_srgba();
    let shell_col = Color::srgb(s.red * 0.55, s.green * 0.55, s.blue * 0.55);
    let shell = meshes.add(Sphere::new(metres_to_bone(0.128)).mesh().ico(3).unwrap());
    let peak = meshes.add(Cuboid::new(
        metres_to_bone(0.17),
        metres_to_bone(0.028),
        metres_to_bone(0.14),
    ));
    let shell_mat = matte_shell(materials, shell_col, 0.78, 0.12);
    let peak_mat = matte_shell(materials, shell_col, 0.82, 0.10);
    commands.entity(head).with_children(|p| {
        p.spawn((
            Equipment,
            Mesh3d(shell),
            MeshMaterial3d(shell_mat),
            equipment_transform_m_scaled(
                Vec3::new(0.0, 0.075, 0.005),
                Quat::IDENTITY,
                Vec3::new(1.0, 1.08, 1.14),
            ),
        ));
        p.spawn((
            Equipment,
            Mesh3d(peak),
            MeshMaterial3d(peak_mat),
            equipment_transform_m(Vec3::new(0.0, 0.115, 0.115), Quat::IDENTITY),
        ));
    });
}

fn attach_cap(
    head: Entity,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    primary: Color,
) {
    let dome = meshes.add(Sphere::new(metres_to_bone(0.118)).mesh().ico(2).unwrap());
    let brim = meshes.add(Cylinder::new(metres_to_bone(0.105), metres_to_bone(0.012)));
    let dome_mat = matte(materials, primary, 0.85);
    commands.entity(head).with_children(|p| {
        p.spawn((
            Equipment,
            Mesh3d(dome),
            MeshMaterial3d(dome_mat.clone()),
            equipment_transform_m_scaled(
                Vec3::new(0.0, 0.09, 0.0),
                Quat::IDENTITY,
                Vec3::new(1.0, 0.72, 1.0),
            ),
        ));
        p.spawn((
            Equipment,
            Mesh3d(brim),
            MeshMaterial3d(dome_mat),
            equipment_transform_m_scaled(
                Vec3::new(0.0, 0.095, 0.09),
                Quat::from_rotation_x(0.18),
                Vec3::new(1.0, 1.0, 1.35),
            ),
        ));
    });
}

fn attach_pad(
    leg: Entity,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    keeper: bool,
) {
    let w = if keeper { 0.10 } else { 0.082 };
    let h = if keeper { 0.26 } else { 0.21 };
    let pad = meshes.add(Cuboid::new(
        metres_to_bone(w * 2.0),
        metres_to_bone(h * 2.0),
        metres_to_bone(0.056),
    ));
    let mat = matte(materials, Color::srgb_u8(0xF1, 0xEE, 0xE4), 0.88);
    spawn_mesh_child(
        leg,
        commands,
        pad,
        mat,
        equipment_transform_m(Vec3::new(0.0, -h - 0.02, 0.055), Quat::IDENTITY),
    );
}

/// Hook each auto-spawned `AnimationPlayer` to its owning figure and give it
/// the shared animation graph so clip playback can begin.
pub fn attach_animation_players(
    mut commands: Commands,
    clips: Option<Res<LocomotionClips>>,
    figures: Query<(), With<Figure>>,
    players: Query<(Entity, Option<&ChildOf>), Added<AnimationPlayer>>,
    parents: Query<&ChildOf>,
) {
    let Some(clips) = clips else { return };
    for (player_ent, child_of) in &players {
        let mut cur = child_of.map(|c| c.parent());
        let mut fig = None;
        let mut steps = 0;
        while let Some(p) = cur {
            if figures.contains(p) {
                fig = Some(p);
                break;
            }
            if let Ok(child) = parents.get(p) {
                cur = Some(child.parent());
            } else {
                break;
            }
            steps += 1;
            if steps > 16 {
                break;
            }
        }
        let Some(fig) = fig else { continue };

        commands.entity(player_ent).insert((
            PlayerOf(fig),
            ClipState::None,
            AnimationTransitions::new(),
            AnimationGraphHandle(clips.graph.clone()),
        ));
    }
}

pub fn yaw_to_face(dir: Vec2) -> f32 {
    dir.x.atan2(dir.y)
}

/// Y-axis rotation (radians) for a figure at `from` to face `to` in XZ.
pub fn face_target(from: Vec2, to: Vec2) -> f32 {
    yaw_to_face(to - from)
}

/// Convenience wrapper around [`face_target`].
pub fn face_target_quat(from: Vec2, to: Vec2) -> Quat {
    Quat::from_rotation_y(face_target(from, to))
}

/// A batsman does not stand square to the bowler: a right-hander turns roughly
/// side-on, chest toward the off side, and looks back down the pitch over the
/// front shoulder. Yaw applied on top of "face the bowler" — slightly under a
/// right angle so the stance reads open rather than fully closed.
pub const BATTER_STANCE_YAW: f32 = 1.35;

/// Facing for a batter waiting at the crease. [`face_target_quat`] would stand
/// them square on to the bowler, which is what made the old stance look like a
/// person queueing rather than a batsman.
pub fn batter_stance_quat(from: Vec2, to: Vec2) -> Quat {
    face_target_quat(from, to) * Quat::from_rotation_y(BATTER_STANCE_YAW)
}

/// Restore every bone's bind-pose translation, leaving the clips rotation-only.
///
/// This does two jobs. It strips root motion so feet stay grounded, and it
/// absorbs the unit mismatch between the Xbot-authored idle/run clips and the
/// generated archetypes they now play on: Xbot's armature is centimetre-scaled
/// under a 0.01 root, the archetypes are metres under a 1.0 root, and the clips
/// animate translation on **all 67 bones**. Left alone, each bone is displaced
/// about a hundredfold and the figures balloon across the ground.
///
/// Rotations retarget cleanly between the two scales, so discarding every
/// translation track is exactly the right correction rather than a workaround.
pub fn strip_skeleton_root_motion(mut bones: Query<(&SkeletonBone, &mut Transform)>) {
    for (bone, mut tf) in &mut bones {
        tf.translation = bone.bind_translation;
    }
}

// ---------------------------------------------------------------------------
// Hybrid animation controller
// ---------------------------------------------------------------------------

const BLEND_RATE: f32 = 12.0;
const BOWL_SETTLE_SECS: f32 = 0.85;

/// Batters at the crease use procedural stance poses instead of the upright
/// Mixamo idle clip.
fn idle_state_uses_locomotion_clip(kind: FigureKind) -> bool {
    !matches!(kind, FigureKind::Batter | FigureKind::NonStriker)
}

/// Locomotion clip selection — currently always procedural.
///
/// The bundled Xbot idle/run clips set **absolute bone-local rotations** in the
/// Mixamo bone frame. Bevy's animation system writes those straight onto the
/// skeleton before any of our systems run, so unlike the procedural pose
/// library there is no seam at which to apply the basis change that
/// [`compose_pose_rotation`] performs. Played verbatim on a MakeHuman rig they
/// produce a near-bind pose, so figures stand with their arms out.
///
/// Idle and run therefore go through `idle_sway` and `run_pose` instead. The
/// clips and their graph are retained: retargeting them offline onto the
/// generated rig is the natural way to reinstate mocap locomotion.
fn locomotion_clip_for_anim(
    _state: AnimState,
    _kind: FigureKind,
    _clips: &LocomotionClips,
) -> Option<(AnimationNodeIndex, ClipState)> {
    // Reinstate the idle/run mapping here once the clips are retargeted onto
    // the generated rig offline; see the doc comment above.
    None
}

/// Drive every figure: locomotion through real mocap clips (idle/run), and
/// keyframed poses for the batting stance, bat swing, bowling action and
/// throws, blended smoothly from whatever pose the skeleton is currently in.
pub fn animate_figures(
    time: Res<Time>,
    clips: Option<Res<LocomotionClips>>,
    mut figures: Query<(Entity, &Figure, &mut Anim)>,
    mut players: Query<(
        Entity,
        &PlayerOf,
        &mut AnimationPlayer,
        &mut AnimationTransitions,
        &mut ClipState,
    )>,
    mut bones: Query<(&Bone, &BoneBindPose, &mut Transform)>,
) {
    let t_global = time.elapsed_secs();
    let blend = (BLEND_RATE * time.delta_secs()).clamp(0.0, 1.0);

    // First match wins, mirroring the previous per-figure linear `find` — a
    // figure's glTF scene may contain more than one AnimationPlayer.
    let mut player_by_figure: std::collections::HashMap<Entity, Entity> =
        std::collections::HashMap::new();
    for (player_ent, po, ..) in players.iter() {
        player_by_figure.entry(po.0).or_insert(player_ent);
    }

    for (fig_ent, fig, mut anim) in &mut figures {
        // Which clip (if any) should drive this figure right now?
        let desired = clips
            .as_ref()
            .and_then(|c| locomotion_clip_for_anim(anim.state, fig.kind, c));

        // Find this figure's player and switch clips when needed.
        let player_state = player_by_figure
            .get(&fig_ent)
            .and_then(|&player_ent| players.get_mut(player_ent).ok())
            .map(
                |(_, _, mut player, mut transitions, mut cs)| match desired {
                    Some((idx, want)) => {
                        if *cs != want {
                            transitions
                                .play(&mut player, idx, Duration::from_millis(220))
                                .repeat();
                            *cs = want;
                        }
                        true
                    }
                    None => {
                        if *cs != ClipState::None {
                            player.stop_all();
                            *cs = ClipState::None;
                        }
                        false
                    }
                },
            )
            .unwrap_or(false);
        if player_state {
            continue; // clip drives the skeleton this frame
        }

        // Procedural pose targets.
        let mut pose = PoseTargets::default();
        match &mut anim.state {
            AnimState::Idle => match fig.kind {
                FigureKind::Batter => batter_stance(t_global, &mut pose),
                FigureKind::NonStriker => non_striker_stance(t_global, &mut pose),
                _ => idle_sway(t_global, &mut pose),
            },
            AnimState::Run { t } => run_pose(*t, &mut pose),
            AnimState::BowlAction { p } => bowl_action(*p, &mut pose),
            AnimState::BowlSettle { t } => {
                bowl_settle(*t, &mut pose);
                let new_t = *t + time.delta_secs() / BOWL_SETTLE_SECS;
                *t = new_t;
                if new_t >= 1.0 {
                    anim.state = AnimState::Idle;
                }
            }
            AnimState::BatSwing { p } => bat_swing(*p, &mut pose),
            AnimState::BatShot { p, shot } => bat_shot(*p, *shot, &mut pose),
            AnimState::Stance => match fig.kind {
                FigureKind::NonStriker => non_striker_stance(t_global, &mut pose),
                _ => batter_stance(t_global, &mut pose),
            },
            AnimState::Throw { p } => throw_pose(*p, &mut pose),
        }
        apply_pose(
            fig_ent,
            &pose,
            if !idle_state_uses_locomotion_clip(fig.kind)
                && matches!(anim.state, AnimState::Idle | AnimState::Stance)
            {
                1.0
            } else {
                blend
            },
            &mut bones,
        );
    }
}

/// Compose a procedural delta onto the imported bind rotation, re-expressing it
/// in this rig's bone axes. Identity delta restores the bind pose.
///
/// Every pose in this file was authored against the Mixamo/Xbot rig, whose bind
/// rotation is **identity at every bone** — the skeleton's shape lives entirely
/// in the bone translations. That makes an Xbot-authored delta effectively a
/// *world-space* rotation.
///
/// The generated MakeHuman archetypes use Blender's convention instead: bones
/// point along their own local +Y and carry a non-identity bind rotation. The
/// same quaternion therefore means a different pose, and applying the library
/// verbatim leaves every figure in its T-pose.
///
/// Conjugating by the bone's bind rotation in armature space maps the delta
/// back into world space and out into this bone's frame, so one correction
/// makes the entire pose library — stance, bowl action, every shot — transfer
/// without re-authoring a single angle.
fn compose_pose_rotation(bind: Quat, world: Quat, delta: Quat) -> Quat {
    bind * (world.inverse() * delta * world)
}

/// Local rotation targets per bone for one frame of procedural animation.
#[derive(Default)]
struct PoseTargets {
    hips: Quat,
    spine: Quat,
    spine1: Quat,
    spine2: Quat,
    neck: Quat,
    ls: Quat,
    rs: Quat,
    la: Quat,
    ra: Quat,
    lfa: Quat,
    rfa: Quat,
    lup: Quat,
    rup: Quat,
    ll: Quat,
    rl: Quat,
    lf: Quat,
    rf: Quat,
}

impl PoseTargets {
    fn delta_for(&self, kind: BoneKind) -> Quat {
        match kind {
            BoneKind::Hips => self.hips,
            BoneKind::Spine => self.spine,
            BoneKind::Spine1 => self.spine1,
            BoneKind::Spine2 => self.spine2,
            BoneKind::Neck => self.neck,
            BoneKind::Head => Quat::IDENTITY,
            BoneKind::LeftShoulder => self.ls,
            BoneKind::RightShoulder => self.rs,
            BoneKind::LeftArm => self.la,
            BoneKind::RightArm => self.ra,
            BoneKind::LeftForeArm => self.lfa,
            BoneKind::RightForeArm => self.rfa,
            BoneKind::LeftUpLeg => self.lup,
            BoneKind::RightUpLeg => self.rup,
            BoneKind::LeftLeg => self.ll,
            BoneKind::RightLeg => self.rl,
            BoneKind::LeftFoot => self.lf,
            BoneKind::RightFoot => self.rf,
            BoneKind::LeftHand | BoneKind::RightHand => Quat::IDENTITY,
        }
    }

    fn set_delta(&mut self, kind: BoneKind, delta: Quat) {
        match kind {
            BoneKind::Hips => self.hips = delta,
            BoneKind::Spine => self.spine = delta,
            BoneKind::Spine1 => self.spine1 = delta,
            BoneKind::Spine2 => self.spine2 = delta,
            BoneKind::Neck => self.neck = delta,
            BoneKind::LeftShoulder => self.ls = delta,
            BoneKind::RightShoulder => self.rs = delta,
            BoneKind::LeftArm => self.la = delta,
            BoneKind::RightArm => self.ra = delta,
            BoneKind::LeftForeArm => self.lfa = delta,
            BoneKind::RightForeArm => self.rfa = delta,
            BoneKind::LeftUpLeg => self.lup = delta,
            BoneKind::RightUpLeg => self.rup = delta,
            BoneKind::LeftLeg => self.ll = delta,
            BoneKind::RightLeg => self.rl = delta,
            BoneKind::LeftFoot => self.lf = delta,
            BoneKind::RightFoot => self.rf = delta,
            BoneKind::Head | BoneKind::LeftHand | BoneKind::RightHand => {}
        }
    }
}

/// Bones blended during bowl follow-through settle.
const BOWL_SETTLE_BONES: &[BoneKind] = &[
    BoneKind::Hips,
    BoneKind::Spine,
    BoneKind::Spine1,
    BoneKind::Spine2,
    BoneKind::Neck,
    BoneKind::LeftShoulder,
    BoneKind::RightShoulder,
    BoneKind::LeftArm,
    BoneKind::RightArm,
    BoneKind::LeftForeArm,
    BoneKind::RightForeArm,
    BoneKind::LeftUpLeg,
    BoneKind::RightUpLeg,
    BoneKind::LeftLeg,
    BoneKind::RightLeg,
    BoneKind::LeftFoot,
    BoneKind::RightFoot,
];

fn rx(a: f32) -> Quat {
    Quat::from_rotation_x(a)
}
fn ry(a: f32) -> Quat {
    Quat::from_rotation_y(a)
}
fn rz(a: f32) -> Quat {
    Quat::from_rotation_z(a)
}

/// Mixamo Xbot bind pose is a T-pose (arms out via bone offsets, zero local
/// rotation). These deltas mirror the idle mocap clip's rest frame so
/// procedural poses start from a natural hang instead of horizontal arms.
fn arms_bind_neutral(pose: &mut PoseTargets) {
    pose.ls = Quat::from_xyzw(0.02216, -0.09768, -0.08424, 0.9914);
    pose.rs = Quat::from_xyzw(0.03022, 0.127, 0.09578, 0.9868);
    pose.la = Quat::from_xyzw(0.1043, -0.09528, -0.532, 0.8349);
    pose.ra = Quat::from_xyzw(-0.01674, 0.04638, 0.5453, 0.8368);
    pose.lfa = Quat::from_xyzw(0.0, -0.1121, 0.0, 0.9937);
    pose.rfa = Quat::from_xyzw(0.0, 0.06569, 0.0, 0.9978);
}

/// Upper-arm swing layered on top of [`arms_bind_neutral`].
fn apply_bat_swing_arms(pose: &mut PoseTargets, arm_z: f32, arm_x: f32, bend: f32) {
    arms_bind_neutral(pose);
    pose.ra = pose.ra * rz(arm_z * 0.85) * rx(arm_x);
    pose.la = pose.la * rz(arm_z * 0.60) * rx(arm_x * 0.94);
    pose.rfa = rx(bend);
    pose.lfa = rx(bend * 0.92);
}

/// Keyframed value over normalised time with smoothstep easing between keys.
fn kf(points: &[(f32, f32)], p: f32) -> f32 {
    let p = p.clamp(0.0, 1.0);
    if p <= points[0].0 {
        return points[0].1;
    }
    for w in points.windows(2) {
        let (p0, v0) = w[0];
        let (p1, v1) = w[1];
        if p <= p1 {
            let t = ((p - p0) / (p1 - p0).max(1e-5)).clamp(0.0, 1.0);
            let t = t * t * (3.0 - 2.0 * t);
            return v0 + (v1 - v0) * t;
        }
    }
    points.last().unwrap().1
}

/// Crouched, side-on batting stance: knees flexed, spine tilted forward,
/// both hands gathered on the bat handle in front of the pads. Subtle
/// breathing and an occasional bat tap against the turf behind the back foot.
fn batter_stance(t: f32, pose: &mut PoseTargets) {
    let breathe = (t * 1.4).sin();
    let tap_cycle = (t * 0.28).sin();
    let bat_tap = if tap_cycle > 0.88 {
        ((tap_cycle - 0.88) / 0.12).min(1.0) * 0.20
    } else {
        0.0
    };

    let arm_z = 0.30 + breathe * 0.02;
    let arm_x = 0.54 + breathe * 0.015 - bat_tap * 0.10;
    let bend = 0.88 - bat_tap * 0.14;
    apply_bat_swing_arms(pose, arm_z, arm_x, bend);
    // Pull the top hand across to the shifted handle grip.
    pose.la *= ry(0.36);
    pose.lfa *= ry(0.10);

    // The root already carries the side-on turn (see `batter_stance_quat`), so
    // the spine only adds a small extra shoulder rotation — yawing the spine
    // far enough to fake side-on on its own just twists the mesh.
    pose.hips = rz(0.06) * rx(-0.16);
    pose.spine = rx(0.26 + breathe * 0.02) * ry(-0.16) * rz(0.04);
    pose.spine1 = rx(0.10) * ry(-0.10);
    // Head turned back over the front shoulder to sight the bowler.
    pose.neck = ry(-1.28) * rx(-0.08);

    // Deep crouch with weight forward over the balls of the feet.
    let crouch = 0.58 + breathe * 0.025;
    pose.lup = rx(crouch);
    pose.rup = rx(crouch * 0.84);
    pose.ll = rx(-0.70 - breathe * 0.02);
    pose.rl = rx(-0.66 - bat_tap * 0.36);
    pose.lf = rx(-0.32);
    pose.rf = rx(-0.36 + bat_tap * 0.14);
}

/// Non-striker waits in a ready crouch, weight forward, poised to sprint.
fn non_striker_stance(t: f32, pose: &mut PoseTargets) {
    let breathe = (t * 1.1).sin();
    let crouch = 0.40 + breathe * 0.02;
    pose.lup = rx(crouch);
    pose.rup = rx(crouch * 1.05);
    pose.ll = rx(-0.56 - breathe * 0.015);
    pose.rl = rx(-0.60);
    pose.hips = ry(-0.38) * rx(-0.10);
    pose.spine = rx(0.24 + breathe * 0.015) * ry(-0.16);
    pose.neck = ry(0.30);
    arms_bind_neutral(pose);
    pose.la = pose.la * rx(0.12) * rz(0.08);
    pose.ra = pose.ra * rx(0.14) * rz(-0.06);
    pose.lfa *= rx(0.22);
    pose.rfa *= rx(0.24);
    pose.lf = rx(-0.18);
    pose.rf = rx(-0.18);
}

/// Relaxed fielder/bowler idle with subtle weight shift.
fn idle_sway(t: f32, pose: &mut PoseTargets) {
    // Fielders, bowler and keeper reach this instead of the Xbot idle clip, so
    // the arms must be brought down from the T-pose bind here.
    arms_bind_neutral(pose);
    let sway = (t * 0.7).sin() * 0.045;
    pose.spine = rz(sway);
    pose.hips = rz(-sway * 0.4);
    // Compose onto the neutral hang rather than replacing it, or the arms snap
    // back to the T-pose bind and only the sway survives.
    pose.ra *= rx(0.06 * (t * 0.9).sin());
    pose.la *= rx(-0.06 * (t * 0.9).sin());
}

/// Jog cycle used where the mocap clip can't reach (e.g. brief repositions).
fn run_pose(t: f32, pose: &mut PoseTargets) {
    const FREQ: f32 = 13.0;
    const AMP: f32 = 0.72;
    let ph = t * FREQ;
    let a = ph.sin() * AMP;
    pose.lup = rx(a * 0.55);
    pose.rup = rx(-a * 0.55);
    pose.ll = rx(a * 0.32);
    pose.rl = rx(-a * 0.32);
    pose.la = rx(-a * 0.62);
    pose.ra = rx(a * 0.62);
    pose.hips = rx((ph * 2.0).sin() * 0.05);
    pose.spine = rx(0.12);
}

/// Full bowling action: gather → coil → delivery → upright follow-through.
fn bowl_action(p: f32, pose: &mut PoseTargets) {
    let pc = p.clamp(0.0, 1.0);
    let arm = kf(
        &[
            (0.0, 0.5),
            (0.30, 2.45),
            (0.45, 2.85),
            (0.62, -0.95),
            (0.80, -1.25),
            (1.0, -1.05),
        ],
        pc,
    );
    let hips_y = kf(&[(0.0, 0.0), (0.40, -0.18), (0.60, 0.22), (1.0, 0.10)], pc);
    let lean = kf(&[(0.0, 0.0), (0.35, -0.20), (0.62, 0.24), (1.0, 0.14)], pc);
    let front_leg = kf(&[(0.0, 0.0), (0.45, 0.65), (0.62, -0.30), (1.0, -0.12)], pc);
    // Delivery stride was over-rotating the back leg, driving the foot mesh
    // through the pitch; eased so feet stay near bind-pose height.
    let back_leg = kf(
        &[(0.0, 0.0), (0.50, -0.68), (0.70, -0.48), (1.0, -0.20)],
        pc,
    );
    let counter = kf(&[(0.0, 0.0), (0.35, -1.15), (0.62, -0.25), (1.0, 0.05)], pc);
    pose.ra = rx(arm);
    pose.rfa = rx(kf(
        &[(0.0, -0.35), (0.45, -0.15), (0.62, 0.12), (1.0, -0.05)],
        pc,
    ));
    pose.la = rx(counter) * rz(-0.38);
    pose.hips = ry(hips_y);
    pose.spine = rx(lean);
    pose.rup = rx(back_leg);
    pose.lup = rx(front_leg);
    pose.rl = rx(kf(&[(0.0, 0.0), (0.5, 0.72), (1.0, 0.42)], pc));
    pose.ll = rx(kf(&[(0.0, 0.0), (0.55, -0.35), (1.0, -0.08)], pc));
}

/// Blend delivery follow-through into a relaxed standing pose.
fn bowl_settle(p: f32, pose: &mut PoseTargets) {
    let mut end = PoseTargets::default();
    bowl_action(1.0, &mut end);
    let mut idle = PoseTargets::default();
    idle_sway(0.0, &mut idle);
    let t = p.clamp(0.0, 1.0);
    let t = t * t * (3.0 - 2.0 * t);
    for &kind in BOWL_SETTLE_BONES {
        let blended = end.delta_for(kind).slerp(idle.delta_for(kind), t);
        pose.set_delta(kind, blended);
    }
}

/// Bat swing: high backlift → accelerating downswing through the line → full
/// follow-through with hip and shoulder rotation.
fn bat_swing(p: f32, pose: &mut PoseTargets) {
    let pc = p.clamp(0.0, 1.0);
    let arm_z = kf(
        &[
            (0.0, 0.32),
            (0.14, 1.08),
            (0.30, 0.82),
            (0.48, -0.55),
            (0.60, -1.62),
            (0.74, -2.28),
            (1.0, -2.55),
        ],
        pc,
    );
    let arm_x = kf(
        &[
            (0.0, 0.58),
            (0.14, 0.18),
            (0.30, 0.32),
            (0.48, 0.88),
            (0.60, 1.22),
            (0.74, 0.78),
            (1.0, 0.42),
        ],
        pc,
    );
    let spine_y = kf(
        &[
            (0.0, -0.28),
            (0.22, -0.38),
            (0.48, -0.08),
            (0.62, 0.34),
            (1.0, 0.42),
        ],
        pc,
    );
    let hips_y = kf(
        &[
            (0.0, -0.10),
            (0.28, -0.22),
            (0.58, 0.32),
            (0.72, 0.38),
            (1.0, 0.30),
        ],
        pc,
    );
    let bend = kf(
        &[
            (0.0, 0.82),
            (0.30, 0.62),
            (0.58, 1.08),
            (0.72, 0.92),
            (1.0, 0.48),
        ],
        pc,
    );
    apply_bat_swing_arms(pose, arm_z, arm_x, bend);
    pose.spine = ry(spine_y) * rx(0.20);
    pose.hips = ry(hips_y);
    // Weight shifts onto the front foot through contact.
    pose.lup = rx(kf(&[(0.0, 0.50), (0.58, 0.26), (1.0, 0.20)], pc));
    pose.rup = rx(kf(&[(0.0, 0.54), (0.58, 0.66), (1.0, 0.70)], pc));
    pose.ll = rx(kf(&[(0.0, -0.66), (0.58, -0.52), (1.0, -0.38)], pc));
    pose.rl = rx(kf(&[(0.0, -0.70), (0.58, -0.62), (1.0, -0.55)], pc));
}

/// Dispatch a named stroke to one of a few parameterised swing archetypes.
fn bat_shot(p: f32, shot: ShotKind, pose: &mut PoseTargets) {
    match shot {
        ShotKind::Defend => bat_shot_defensive(p, false, pose),
        ShotKind::Backfoot => bat_shot_defensive(p, true, pose),
        ShotKind::StraightDrive => bat_shot_vertical_drive(p, 0.0, 0.0, pose),
        ShotKind::CoverDrive => bat_shot_vertical_drive(p, -0.48, 0.0, pose),
        ShotKind::OnDrive => bat_shot_vertical_drive(p, 0.42, 0.0, pose),
        ShotKind::LoftedDrive => bat_shot_vertical_drive(p, 0.0, 1.0, pose),
        ShotKind::Flick => bat_shot_flick(p, pose),
        ShotKind::SquareCut | ShotKind::LateCut | ShotKind::Pull | ShotKind::Hook => {
            bat_shot_cross_bat(p, shot, pose)
        }
        ShotKind::Sweep | ShotKind::SlogSweep => bat_shot_sweep(p, shot.aerial(), pose),
        ShotKind::Slog => bat_shot_slog(p, pose),
    }
}

/// Vertical-bat front-foot drive family — swing plane rotates with target line.
fn bat_shot_vertical_drive(p: f32, plane_y: f32, loft: f32, pose: &mut PoseTargets) {
    let pc = p.clamp(0.0, 1.0);
    let loft_k = loft.clamp(0.0, 1.0);
    let arm_z = kf(
        &[
            (0.0, 0.28),
            (0.14, 1.05),
            (0.30, 0.78),
            (0.48, -0.48),
            (0.60, -1.55),
            (0.74, -2.18 - loft_k * 0.35),
            (1.0, -2.42 - loft_k * 0.45),
        ],
        pc,
    );
    let arm_x = kf(
        &[
            (0.0, 0.55),
            (0.14, 0.16),
            (0.30, 0.30),
            (0.48, 0.92 + loft_k * 0.18),
            (0.60, 1.28 + loft_k * 0.35),
            (0.74, 0.82 + loft_k * 0.22),
            (1.0, 0.38),
        ],
        pc,
    );
    let spine_y = kf(
        &[
            (0.0, plane_y - 0.22),
            (0.22, plane_y - 0.32),
            (0.48, plane_y + 0.02),
            (0.62, plane_y + 0.36),
            (1.0, plane_y + 0.44),
        ],
        pc,
    );
    let hips_y = kf(
        &[
            (0.0, plane_y * 0.4 - 0.08),
            (0.28, plane_y * 0.5 - 0.18),
            (0.58, plane_y * 0.6 + 0.28),
            (0.72, plane_y * 0.65 + 0.34),
            (1.0, plane_y * 0.55 + 0.28),
        ],
        pc,
    );
    let bend = kf(
        &[
            (0.0, 0.82),
            (0.30, 0.60),
            (0.58, 1.05 + loft_k * 0.12),
            (0.72, 0.90),
            (1.0, 0.46),
        ],
        pc,
    );
    apply_bat_swing_arms(pose, arm_z, arm_x, bend);
    pose.spine = ry(spine_y) * rx(0.22);
    pose.hips = ry(hips_y);
    pose.lup = rx(kf(
        &[(0.0, 0.48), (0.40, 0.22), (0.58, 0.18), (1.0, 0.16)],
        pc,
    ));
    pose.rup = rx(kf(
        &[(0.0, 0.52), (0.40, 0.62), (0.58, 0.68), (1.0, 0.72)],
        pc,
    ));
    pose.ll = rx(kf(
        &[(0.0, -0.64), (0.40, -0.38), (0.58, -0.48), (1.0, -0.42)],
        pc,
    ));
    pose.rl = rx(kf(&[(0.0, -0.68), (0.58, -0.58), (1.0, -0.52)], pc));
}

/// Wristy leg-side clip with minimal stride.
fn bat_shot_flick(p: f32, pose: &mut PoseTargets) {
    let pc = p.clamp(0.0, 1.0);
    let arm_z = kf(
        &[
            (0.0, 0.22),
            (0.22, 0.62),
            (0.45, -0.35),
            (0.62, -1.05),
            (1.0, -1.28),
        ],
        pc,
    );
    let arm_x = kf(
        &[
            (0.0, 0.52),
            (0.22, 0.48),
            (0.45, 0.72),
            (0.62, 0.95),
            (1.0, 0.55),
        ],
        pc,
    );
    let wrist = kf(&[(0.0, 0.0), (0.40, 0.55), (0.62, 0.85), (1.0, 0.35)], pc);
    arms_bind_neutral(pose);
    pose.ra = pose.ra * rz(arm_z * 0.78) * rx(arm_x);
    pose.la = pose.la * rz(arm_z * 0.55) * rx(arm_x * 0.88);
    pose.rfa = rx(0.72 + wrist);
    pose.lfa = rx(0.68 + wrist * 0.85);
    pose.spine = ry(kf(
        &[(0.0, 0.18), (0.45, 0.38), (0.62, 0.48), (1.0, 0.32)],
        pc,
    )) * rx(0.18);
    pose.hips = ry(kf(&[(0.0, 0.08), (0.45, 0.28), (1.0, 0.22)], pc));
    pose.lup = rx(kf(&[(0.0, 0.50), (0.58, 0.44), (1.0, 0.40)], pc));
    pose.rup = rx(kf(&[(0.0, 0.54), (0.58, 0.58), (1.0, 0.56)], pc));
    pose.ll = rx(kf(&[(0.0, -0.66), (0.58, -0.60), (1.0, -0.54)], pc));
    pose.rl = rx(kf(&[(0.0, -0.70), (0.58, -0.64), (1.0, -0.58)], pc));
}

/// Back-foot horizontal bat arc across the body.
fn bat_shot_cross_bat(p: f32, shot: ShotKind, pose: &mut PoseTargets) {
    let pc = p.clamp(0.0, 1.0);
    let (plane_y, height, shoulder) = match shot {
        ShotKind::SquareCut => (-0.72, 0.0, 0.42),
        ShotKind::LateCut => (-0.88, 0.05, 0.48),
        ShotKind::Pull => (0.58, 0.12, 0.55),
        ShotKind::Hook => (0.62, 0.38, 0.82),
        _ => (-0.72, 0.0, 0.42),
    };
    let arm_z = kf(
        &[
            (0.0, 0.35),
            (0.18, 1.15),
            (0.38, 0.45),
            (0.52, -0.85),
            (0.68, -1.95 - height),
            (1.0, -2.35 - height * 0.6),
        ],
        pc,
    );
    let arm_x = kf(
        &[
            (0.0, 0.48),
            (0.18, 0.22),
            (0.38, 0.38),
            (0.52, 0.55),
            (0.68, 0.42 + height * 0.35),
            (1.0, 0.28),
        ],
        pc,
    );
    arms_bind_neutral(pose);
    pose.ra = pose.ra * rz(arm_z) * rx(arm_x);
    pose.la = pose.la * rz(arm_z * 0.72) * rx(arm_x * 0.82);
    pose.rfa = rx(kf(
        &[(0.0, 0.65), (0.52, 0.35), (0.68, 0.18), (1.0, 0.08)],
        pc,
    ));
    pose.lfa = rx(kf(&[(0.0, 0.62), (0.52, 0.32), (1.0, 0.06)], pc));
    pose.spine = ry(kf(
        &[
            (0.0, plane_y * 0.3 - 0.12),
            (0.35, plane_y * 0.5),
            (0.62, plane_y * 0.7 + shoulder * 0.2),
            (1.0, plane_y * 0.55 + shoulder * 0.15),
        ],
        pc,
    )) * rx(kf(
        &[(0.0, 0.12), (0.35, -0.08), (0.62, 0.18), (1.0, 0.10)],
        pc,
    ));
    pose.hips = ry(kf(
        &[
            (0.0, -0.05),
            (0.35, plane_y * 0.35),
            (0.62, plane_y * 0.55),
            (1.0, plane_y * 0.42),
        ],
        pc,
    ));
    pose.lup = rx(kf(
        &[(0.0, 0.42), (0.35, 0.28), (0.62, 0.22), (1.0, 0.26)],
        pc,
    ));
    pose.rup = rx(kf(
        &[(0.0, 0.55), (0.35, 0.72), (0.62, 0.78), (1.0, 0.68)],
        pc,
    ));
    pose.ll = rx(kf(
        &[(0.0, -0.58), (0.35, -0.42), (0.62, -0.35), (1.0, -0.38)],
        pc,
    ));
    pose.rl = rx(kf(
        &[(0.0, -0.72), (0.35, -0.82), (0.62, -0.75), (1.0, -0.62)],
        pc,
    ));
}

/// Front-knee sweep — bat swings low and across; knee stays above the turf.
fn bat_shot_sweep(p: f32, aerial: bool, pose: &mut PoseTargets) {
    let pc = p.clamp(0.0, 1.0);
    let loft = if aerial { 0.35 } else { 0.0 };
    let arm_z = kf(
        &[
            (0.0, 0.18),
            (0.25, 0.55),
            (0.48, -0.42),
            (0.65, -1.35 - loft),
            (1.0, -1.65 - loft),
        ],
        pc,
    );
    let arm_x = kf(
        &[
            (0.0, 0.62),
            (0.25, 0.72),
            (0.48, 0.95),
            (0.65, 0.78),
            (1.0, 0.52),
        ],
        pc,
    );
    arms_bind_neutral(pose);
    pose.ra = pose.ra * rz(arm_z) * rx(arm_x);
    pose.la = pose.la * rz(arm_z * 0.65) * rx(arm_x * 0.90);
    pose.rfa = rx(kf(
        &[(0.0, 0.88), (0.48, 0.55), (0.65, 0.38), (1.0, 0.22)],
        pc,
    ));
    pose.lfa = rx(kf(&[(0.0, 0.85), (0.48, 0.52), (1.0, 0.20)], pc));
    pose.spine = ry(kf(&[(0.0, 0.32), (0.48, 0.52), (1.0, 0.38)], pc))
        * rx(kf(&[(0.0, 0.28), (0.48, 0.55), (1.0, 0.42)], pc));
    pose.hips = ry(kf(&[(0.0, 0.12), (0.48, 0.35), (1.0, 0.28)], pc));
    // Front knee drops toward the turf; foot-only safety clamp ignores the knee.
    pose.lup = rx(kf(
        &[(0.0, 0.55), (0.35, 0.95), (0.55, 1.12), (1.0, 0.88)],
        pc,
    ));
    pose.rup = rx(kf(&[(0.0, 0.48), (0.35, 0.38), (1.0, 0.42)], pc));
    pose.ll = rx(kf(
        &[(0.0, -0.62), (0.35, -0.95), (0.55, -1.05), (1.0, -0.82)],
        pc,
    ));
    pose.rl = rx(kf(&[(0.0, -0.68), (0.35, -0.55), (1.0, -0.48)], pc));
    pose.lf = rx(kf(&[(0.0, -0.18), (0.55, -0.32), (1.0, -0.22)], pc));
}

/// Short dead-bat push — barely any follow-through.
fn bat_shot_defensive(p: f32, backfoot: bool, pose: &mut PoseTargets) {
    let pc = p.clamp(0.0, 1.0);
    let arm_z = kf(
        &[(0.0, 0.12), (0.30, -0.05), (0.55, -0.10), (1.0, 0.02)],
        pc,
    );
    let arm_x = kf(&[(0.0, 0.48), (0.30, 0.54), (0.55, 0.50), (1.0, 0.44)], pc);
    arms_bind_neutral(pose);
    pose.ra = pose.ra * rz(arm_z * 0.55) * rx(arm_x);
    pose.la = pose.la * rz(arm_z * 0.40) * rx(arm_x * 0.92);
    pose.rfa = rx(kf(&[(0.0, 0.78), (0.55, 0.72), (1.0, 0.68)], pc));
    pose.lfa = rx(kf(&[(0.0, 0.82), (0.55, 0.76), (1.0, 0.72)], pc));
    pose.spine = ry(kf(&[(0.0, -0.08), (0.55, 0.02), (1.0, -0.04)], pc)) * rx(0.16);
    pose.hips = ry(kf(&[(0.0, -0.04), (0.55, 0.04), (1.0, 0.0)], pc));
    if backfoot {
        pose.lup = rx(kf(&[(0.0, 0.38), (0.55, 0.48), (1.0, 0.44)], pc));
        pose.rup = rx(kf(&[(0.0, 0.52), (0.55, 0.62), (1.0, 0.58)], pc));
        pose.ll = rx(kf(&[(0.0, -0.52), (0.55, -0.48), (1.0, -0.44)], pc));
        pose.rl = rx(kf(&[(0.0, -0.72), (0.55, -0.78), (1.0, -0.70)], pc));
    } else {
        pose.lup = rx(kf(&[(0.0, 0.46), (0.55, 0.32), (1.0, 0.36)], pc));
        pose.rup = rx(kf(&[(0.0, 0.54), (0.55, 0.60), (1.0, 0.56)], pc));
        pose.ll = rx(kf(&[(0.0, -0.64), (0.55, -0.50), (1.0, -0.46)], pc));
        pose.rl = rx(kf(&[(0.0, -0.70), (0.55, -0.62), (1.0, -0.58)], pc));
    }
}

/// Full-blooded cross-bat heave with the front leg clearing.
fn bat_shot_slog(p: f32, pose: &mut PoseTargets) {
    let pc = p.clamp(0.0, 1.0);
    let arm_z = kf(
        &[
            (0.0, 0.42),
            (0.16, 1.22),
            (0.34, 0.55),
            (0.50, -1.05),
            (0.68, -2.45),
            (1.0, -2.85),
        ],
        pc,
    );
    let arm_x = kf(
        &[
            (0.0, 0.52),
            (0.16, 0.18),
            (0.34, 0.42),
            (0.50, 0.72),
            (0.68, 0.55),
            (1.0, 0.32),
        ],
        pc,
    );
    arms_bind_neutral(pose);
    pose.ra = pose.ra * rz(arm_z) * rx(arm_x);
    pose.la = pose.la * rz(arm_z * 0.68) * rx(arm_x * 0.80);
    pose.rfa = rx(kf(
        &[(0.0, 0.70), (0.50, 0.28), (0.68, 0.12), (1.0, 0.05)],
        pc,
    ));
    pose.lfa = rx(kf(&[(0.0, 0.68), (0.50, 0.25), (1.0, 0.04)], pc));
    pose.spine = ry(kf(
        &[(0.0, 0.22), (0.50, 0.55), (0.68, 0.62), (1.0, 0.48)],
        pc,
    )) * rx(kf(&[(0.0, 0.10), (0.50, 0.22), (1.0, 0.08)], pc));
    pose.hips = ry(kf(
        &[(0.0, 0.05), (0.50, 0.42), (0.68, 0.48), (1.0, 0.35)],
        pc,
    ));
    pose.lup = rx(kf(
        &[(0.0, 0.45), (0.40, 0.15), (0.55, -0.05), (1.0, 0.08)],
        pc,
    ));
    pose.rup = rx(kf(
        &[(0.0, 0.55), (0.40, 0.72), (0.68, 0.82), (1.0, 0.68)],
        pc,
    ));
    pose.ll = rx(kf(&[(0.0, -0.58), (0.55, -0.28), (1.0, -0.22)], pc));
    pose.rl = rx(kf(
        &[(0.0, -0.72), (0.40, -0.88), (0.68, -0.78), (1.0, -0.58)],
        pc,
    ));
}

/// Quick underarm-ish return throw with wrist snap.
fn throw_pose(p: f32, pose: &mut PoseTargets) {
    let pc = p.clamp(0.0, 1.0);
    let arm = kf(
        &[
            (0.0, -0.5),
            (0.30, -2.25),
            (0.52, 0.45),
            (0.78, 0.95),
            (1.0, 0.65),
        ],
        pc,
    );
    let fore = kf(&[(0.0, -0.7), (0.30, -1.15), (0.52, 0.25), (1.0, 0.10)], pc);
    let twist = kf(&[(0.0, 0.0), (0.35, -0.30), (0.60, 0.35), (1.0, 0.28)], pc);
    pose.ra = rx(arm);
    pose.rfa = rx(fore);
    pose.la = rz(-0.35);
    pose.spine = ry(twist);
    pose.hips = ry(twist * 0.6);
}

fn apply_pose(
    fig_ent: Entity,
    pose: &PoseTargets,
    blend: f32,
    bones: &mut Query<(&Bone, &BoneBindPose, &mut Transform)>,
) {
    for (bone, bind, mut tf) in &mut *bones {
        if bone.figure != fig_ent {
            continue;
        }
        let delta = pose.delta_for(bone.kind);
        let target = compose_pose_rotation(bind.rotation, bind.world_rotation, delta);
        tf.rotation = tf.rotation.slerp(target, blend);
    }
}

/// Legacy – does nothing now (kept for compatibility).
pub fn animate_skeleton(
    _time: Res<Time>,
    _figures: Query<(Entity, &Figure, &Anim)>,
    _bones: Query<(&Bone, &mut Transform)>,
) {
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::Vec2;

    #[test]
    fn metres_to_bone_units_scales_by_armature_ratio() {
        // The generated archetypes export bone translations in metres under a
        // root at scale 1.0, so bone units and metres coincide. (The legacy
        // Xbot armature was centimetres under a 0.01 root, hence the ratio.)
        assert!((metres_to_bone(1.0) - 1.0).abs() < 1e-5);
        assert!((metres_to_bone(0.44) - 0.44).abs() < 1e-5);
        let tf = equipment_transform_m(Vec3::new(0.0, -0.44, 0.10), Quat::IDENTITY);
        assert!((tf.translation.y + 0.44).abs() < 1e-4);
        assert!((tf.translation.z - 0.10).abs() < 1e-4);
    }

    #[test]
    fn every_archetype_name_has_a_built_asset() {
        // Guards the asset contract: a typo here, or a rename in
        // `scripts/build_player_asset.py`, would otherwise surface as an
        // invisible player at runtime rather than a failing build.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let missing: Vec<&str> = PLAYER_ARCHETYPES
            .iter()
            .copied()
            .filter(|name| {
                !root
                    .join(format!("assets/characters/players/{name}.glb"))
                    .exists()
            })
            .collect();
        assert!(missing.is_empty(), "no GLB for archetypes: {missing:?}");
    }

    #[test]
    fn archetype_selection_is_deterministic_and_spreads() {
        let seed = player_skin_seed("IND", FigureKind::Bowler);
        assert_eq!(archetype_for_seed(seed), archetype_for_seed(seed));

        // Eleven fielders must not all end up the same shape.
        let picks: std::collections::HashSet<&str> = (0..11)
            .map(|slot| archetype_for_seed(player_skin_seed("IND", FigureKind::Fielder(slot))))
            .collect();
        assert!(
            picks.len() >= 5,
            "archetypes barely vary across a side: {picks:?}"
        );
    }

    #[test]
    fn archetype_and_skin_tone_vary_independently() {
        // Same body, different colouring across teams — the two must not be
        // locked together, or every tall player would share a skin tone.
        let pairs: std::collections::HashSet<(&str, usize)> = ["IND", "AUS", "ENG", "RSA", "NZL"]
            .into_iter()
            .map(|team| {
                let seed = player_skin_seed(team, FigureKind::Batter);
                (archetype_for_seed(seed), player_skin_tone_index(seed))
            })
            .collect();
        let bodies: std::collections::HashSet<&str> = pairs.iter().map(|(a, _)| *a).collect();
        let tones: std::collections::HashSet<usize> = pairs.iter().map(|(_, t)| *t).collect();
        assert!(bodies.len() > 1 && tones.len() > 1);
    }

    #[test]
    fn kit_pattern_uses_team_colour_at_high_v() {
        let primary = Color::srgb(0.1, 0.2, 0.9);
        let secondary = Color::srgb(0.9, 0.8, 0.1);
        let img = kit_pattern_image(KitStyle::Solid, primary, secondary);
        // High-V texels were wrongly mapped to skin tone before the UV fix.
        let idx = ((64 * 58 + 32) * 4) as usize;
        let data = img.data.as_ref().expect("kit pattern pixel data");
        let r = data[idx] as f32 / 255.0;
        let g = data[idx + 1] as f32 / 255.0;
        let b = data[idx + 2] as f32 / 255.0;
        let p = primary.to_srgba();
        assert!(
            (r - p.red).abs() < 0.02 && (g - p.green).abs() < 0.02 && (b - p.blue).abs() < 0.02,
            "high-v pixel should be primary kit colour, got ({r},{g},{b})",
        );
    }

    #[test]
    fn batter_stance_hips_turn_side_on_and_forearms_match() {
        let mut stance = PoseTargets::default();
        batter_stance(0.0, &mut stance);
        // Side-on comes from the figure root (`batter_stance_quat`), not from
        // yawing the spine: twisting the spine that far deforms the mesh.
        assert!(
            stance.spine.angle_between(Quat::IDENTITY) < 0.6,
            "spine should only add a small shoulder turn, not fake side-on alone"
        );
        let mut swing_start = PoseTargets::default();
        batter_stance(0.0, &mut stance);
        bat_swing(0.0, &mut swing_start);
        assert!(
            stance.la.angle_between(swing_start.la) < 0.48,
            "stance left arm should stay near stroke-ready after cross-pull"
        );
        assert!(
            stance.ra.angle_between(swing_start.ra) < 0.14,
            "stance grip should match stroke-ready right arm"
        );
        assert!(
            stance.lfa.angle_between(Quat::IDENTITY) > 0.35,
            "left forearm should be flexed for the grip"
        );
        assert!(
            stance.rfa.angle_between(Quat::IDENTITY) > 0.35,
            "right forearm should be flexed for the grip"
        );
        assert!(
            stance.lup.angle_between(Quat::IDENTITY) > 0.35,
            "knees should be visibly flexed"
        );
    }

    #[test]
    fn bat_swing_reaches_full_follow_through() {
        let mut start = PoseTargets::default();
        let mut contact = PoseTargets::default();
        let mut end = PoseTargets::default();
        bat_swing(0.0, &mut start);
        bat_swing(0.60, &mut contact);
        bat_swing(1.0, &mut end);
        assert!(
            contact.ra.angle_between(start.ra) > 0.35,
            "backlift should separate from contact pose"
        );
        assert!(
            end.ra.angle_between(contact.ra) > 0.25,
            "follow-through should continue past contact"
        );
    }

    #[test]
    fn bone_kind_matches_mixamo_prefix() {
        assert_eq!(
            bone_kind_for_name("mixamorig:RightHand"),
            Some(BoneKind::RightHand)
        );
        assert_eq!(
            bone_kind_for_name("mixamorig:LeftShoulder"),
            Some(BoneKind::LeftShoulder)
        );
        assert_eq!(
            bone_kind_for_name("mixamorig:RightShoulder"),
            Some(BoneKind::RightShoulder)
        );
    }

    /// The colour fallback in `kit_mesh_kind` classifies willow and white gear
    /// as jersey material, so equipment must be excluded from the recolour pass
    /// by marker, not by colour. Regression: the bat rendered in team colours.
    fn willow_and_pad_colours_would_be_misread_as_kit() {
        let willow = StandardMaterial {
            base_color: Color::srgb_u8(0xE6, 0xD2, 0xA0),
            ..Default::default()
        };
        assert_eq!(
            kit_mesh_kind("", &willow),
            Some(KitMeshKind::Surface),
            "willow trips the colour fallback, so `Without<Equipment>` is what keeps \
             the bat from being repainted - do not rely on colour to exclude gear"
        );
    }

    #[test]
    fn kit_mesh_kind_detects_imported_surface_colour() {
        let mat = StandardMaterial {
            base_color: Color::srgb(0.837, 0.302, 0.264),
            ..Default::default()
        };
        assert_eq!(kit_mesh_kind("", &mat), Some(KitMeshKind::Surface));
    }

    #[test]
    fn face_target_striker_faces_bowler_end() {
        let bowler_end = Vec2::new(-10.0, 0.0);
        let yaw = face_target(Vec2::new(9.0, -0.15), bowler_end);
        assert!(
            (yaw + std::f32::consts::FRAC_PI_2).abs() < 0.1,
            "striker should face bowler's end (-X), got {yaw}"
        );
    }

    #[test]
    fn face_target_bowler_faces_striker() {
        let yaw = face_target(Vec2::new(-18.0, 0.35), Vec2::new(9.0, -0.15));
        assert!(
            (yaw - std::f32::consts::FRAC_PI_2).abs() < 0.1,
            "bowler should face down the pitch (+X), got {yaw}"
        );
    }

    #[test]
    fn yaw_to_face_plus_z_is_zero() {
        assert!(yaw_to_face(Vec2::new(0.0, 1.0)).abs() < 1e-5);
    }

    #[test]
    fn model_forward_matches_identity_yaw() {
        assert!(yaw_to_face(MODEL_FORWARD_XZ).abs() < 1e-5);
    }

    /// Rotate the model's forward axis by `yaw` using the same `Quat` the
    /// spawner applies, so the test exercises Bevy's real rotation rather than
    /// re-deriving the formula the convention already assumes.
    fn rotate_model_forward(yaw: f32) -> Vec2 {
        let f = MODEL_FORWARD_XZ;
        let v = Quat::from_rotation_y(yaw) * Vec3::new(f.x, 0.0, f.y);
        Vec2::new(v.x, v.z)
    }

    #[test]
    fn yaw_to_face_round_trips_model_forward() {
        let dirs = [
            Vec2::new(1.0, 0.0),
            Vec2::new(-1.0, 0.0),
            Vec2::new(0.0, 1.0),
            Vec2::new(0.0, -1.0),
            Vec2::new(3.0, 4.0),
            Vec2::new(-12.0, 5.0),
        ];
        for dir in dirs {
            let yaw = yaw_to_face(dir);
            let out = rotate_model_forward(yaw);
            let want = dir.normalize();
            assert!(
                (out - want).length() < 1e-5,
                "dir {dir:?} -> yaw {yaw} -> {out:?}, want {want:?}"
            );
        }
    }

    #[test]
    fn compose_pose_identity_delta_restores_bind() {
        let bind = Quat::from_rotation_y(1.2) * Quat::from_rotation_x(0.31);
        let world = Quat::from_rotation_z(0.7);
        let target = compose_pose_rotation(bind, world, Quat::IDENTITY);
        assert!(
            target.dot(bind).abs() > 0.999,
            "expected bind, got {target:?}"
        );
    }

    #[test]
    fn compose_pose_delta_is_local_to_bind_on_a_mixamo_frame_rig() {
        // Xbot's bind rotation is identity at every bone, so on that rig the
        // correction collapses and a delta stays a plain local rotation.
        let bind = Quat::from_rotation_x(0.5);
        let delta = Quat::from_rotation_z(0.25);
        let target = compose_pose_rotation(bind, Quat::IDENTITY, delta);
        assert!(target.dot(bind * delta).abs() > 0.999);
    }

    #[test]
    fn compose_pose_applies_delta_in_world_space_whatever_the_bone_frame() {
        // The pose library is authored against Xbot, whose bones sit in world
        // orientation at bind. A rig whose bones are rotated differently must
        // still swing the *same way in the world*, or the whole library would
        // have to be re-authored per rig.
        let delta = Quat::from_rotation_z(0.4);
        for world in [
            Quat::IDENTITY,
            Quat::from_rotation_y(1.1),
            Quat::from_rotation_x(-0.8) * Quat::from_rotation_z(2.2),
        ] {
            // A root-level bone whose bind orientation in armature space is
            // `world`, so its posed world rotation is just its local rotation.
            let posed_world = compose_pose_rotation(world, world, delta);
            let want = delta * world;
            assert!(
                posed_world.dot(want).abs() > 0.999,
                "world-space swing differs for bone frame {world:?}"
            );
        }
    }

    #[test]
    fn crease_batters_use_procedural_stance_not_idle_clip() {
        for kind in [FigureKind::Batter, FigureKind::NonStriker] {
            assert!(
                !idle_state_uses_locomotion_clip(kind),
                "{kind:?} should use procedural stance, not mocap idle",
            );
            assert!(
                locomotion_clip_for_anim(
                    AnimState::Idle,
                    kind,
                    &LocomotionClips {
                        graph: Handle::default(),
                        idle: AnimationNodeIndex::new(0),
                        run: AnimationNodeIndex::new(1),
                    }
                )
                .is_none(),
                "{kind:?} should not resolve to idle clip",
            );
        }
    }

    #[test]
    fn field_roles_fall_back_to_procedural_idle() {
        // The Xbot mocap clips cannot retarget onto the generated MakeHuman rig
        // — see `locomotion_clip_for_anim` — so every role now poses
        // procedurally. `idle_state_uses_locomotion_clip` still distinguishes
        // batters (who get a batting stance) from everyone else (idle sway).
        let kinds = [
            FigureKind::Bowler,
            FigureKind::Keeper,
            FigureKind::Fielder(0),
            FigureKind::Umpire,
        ];
        for kind in kinds {
            assert!(
                idle_state_uses_locomotion_clip(kind),
                "{kind:?} should idle-sway rather than take a batting stance",
            );
            assert!(
                locomotion_clip_for_anim(
                    AnimState::Idle,
                    kind,
                    &LocomotionClips {
                        graph: Handle::default(),
                        idle: AnimationNodeIndex::new(0),
                        run: AnimationNodeIndex::new(1),
                    }
                )
                .is_none(),
                "{kind:?} must not resolve to a clip while retarget is unavailable",
            );
        }
    }

    #[test]
    fn idle_sway_brings_arms_down_from_the_tpose_bind() {
        // Without the clip, this is the only thing stopping fielders standing
        // with their arms straight out.
        let mut pose = PoseTargets::default();
        idle_sway(0.0, &mut pose);
        assert!(
            pose.la.angle_between(Quat::IDENTITY) > 0.35
                && pose.ra.angle_between(Quat::IDENTITY) > 0.35,
            "upper arms are still at the T-pose bind",
        );
    }

    #[test]
    fn batter_stance_quat_turns_the_body_side_on() {
        let bowler_end = Vec2::new(-crate::core::geometry::PITCH_HALF_LEN, 0.0);
        let batsman = crate::core::geometry::BATSMAN_POS;
        let square = face_target_quat(batsman, bowler_end);
        let stance = batter_stance_quat(batsman, bowler_end);

        // Standing square on to the bowler is what made the old stance wrong.
        let turn = square.angle_between(stance);
        assert!(
            (1.0..=1.6).contains(&turn),
            "stance should turn roughly side-on from the square facing, got {turn}"
        );

        // A right-hander's chest ends up pointing down the off side (+Z).
        let forward = stance * Vec3::new(MODEL_FORWARD_XZ.x, 0.0, MODEL_FORWARD_XZ.y);
        assert!(
            forward.z > 0.85,
            "chest should face the off side (+Z), got {forward:?}"
        );
    }

    #[test]
    fn bat_shot_strokes_differ_from_generic_swing() {
        let mut defend = PoseTargets::default();
        let mut cover = PoseTargets::default();
        let mut swing = PoseTargets::default();
        bat_shot(0.55, ShotKind::Defend, &mut defend);
        bat_shot(0.55, ShotKind::CoverDrive, &mut cover);
        bat_swing(0.55, &mut swing);
        assert!(
            defend.ra.angle_between(swing.ra) > 0.08,
            "defensive block should differ from generic swing"
        );
        assert!(
            cover.ra.angle_between(swing.ra) > 0.05,
            "cover drive should differ from generic swing"
        );
        assert!(
            cover.spine.angle_between(defend.spine) > 0.04,
            "cover drive plane should differ from defensive block"
        );
    }

    #[test]
    fn bowl_settle_end_state_uses_identity_deltas_for_unset_bones() {
        let mut end = PoseTargets::default();
        bowl_action(1.0, &mut end);
        // Bones not keyed in bowl_action stay at identity delta → bind at apply time.
        assert_eq!(end.spine1, Quat::IDENTITY);
        assert_eq!(end.neck, Quat::IDENTITY);
    }

    /// Imported glTF meshes share one material handle; kit styling must clone per mesh.
    #[test]
    fn apply_team_kit_materials_clones_shared_import_handle_per_figure() {
        use crate::core::teams::builtin_teams;

        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<Assets<StandardMaterial>>();
        app.init_resource::<Assets<Image>>();
        app.add_systems(Update, apply_team_kit_materials);

        let shared_import = {
            let mut mats = app.world_mut().resource_mut::<Assets<StandardMaterial>>();
            mats.add(StandardMaterial {
                base_color: Color::srgb(0.82, 0.22, 0.22),
                ..Default::default()
            })
        };
        let orig_import_color = app
            .world()
            .resource::<Assets<StandardMaterial>>()
            .get(&shared_import)
            .unwrap()
            .base_color;

        let crest = {
            let mut mats = app.world_mut().resource_mut::<Assets<StandardMaterial>>();
            mats.add(StandardMaterial::default())
        };

        let teams = builtin_teams();
        let india = &teams[0];
        let australia = &teams[1];

        let spawn_surface = |world: &mut World, fig: Entity, mat: Handle<StandardMaterial>| {
            world
                .spawn((
                    Name::new("Beta_Surface"),
                    Mesh3d(Handle::default()),
                    MeshMaterial3d(mat),
                    ChildOf(fig),
                ))
                .id()
        };

        let india_fig = app
            .world_mut()
            .spawn((
                Figure {
                    kind: FigureKind::Fielder(0),
                },
                TeamKit {
                    primary_color: india.primary_color,
                    secondary_color: india.secondary_color,
                    kit_style: india.kit_style,
                    crest: crest.clone(),
                    team_short: india.short.clone(),
                    player_name: None,
                    squad_number: None,
                },
            ))
            .id();
        let aus_fig = app
            .world_mut()
            .spawn((
                Figure {
                    kind: FigureKind::Fielder(1),
                },
                TeamKit {
                    primary_color: australia.primary_color,
                    secondary_color: australia.secondary_color,
                    kit_style: australia.kit_style,
                    crest: crest.clone(),
                    team_short: australia.short.clone(),
                    player_name: None,
                    squad_number: None,
                },
            ))
            .id();

        let india_mesh = spawn_surface(app.world_mut(), india_fig, shared_import.clone());
        let aus_mesh = spawn_surface(app.world_mut(), aus_fig, shared_import.clone());

        app.update();

        let world = app.world();
        let materials = world.resource::<Assets<StandardMaterial>>();

        assert_eq!(
            materials.get(&shared_import).unwrap().base_color,
            orig_import_color,
            "shared imported material must not be mutated",
        );

        let india_handle = world
            .entity(india_mesh)
            .get::<MeshMaterial3d<StandardMaterial>>()
            .unwrap()
            .0
            .clone();
        let aus_handle = world
            .entity(aus_mesh)
            .get::<MeshMaterial3d<StandardMaterial>>()
            .unwrap()
            .0
            .clone();

        assert_ne!(india_handle, shared_import);
        assert_ne!(aus_handle, shared_import);
        assert_ne!(india_handle, aus_handle);

        let india_mat = materials.get(&india_handle).unwrap();
        let aus_mat = materials.get(&aus_handle).unwrap();
        assert!(
            india_mat.base_color_texture.is_none(),
            "solid kit should tint base colour directly",
        );
        assert_eq!(india_mat.base_color, india.primary_color);
        let aus_tex = aus_mat
            .base_color_texture
            .clone()
            .expect("patterned kit should have pattern texture");
        assert_ne!(
            india_mat.base_color, aus_mat.base_color,
            "distinct kits must get distinct materials",
        );
        assert!(aus_tex != Handle::default());

        assert!(world.entity(india_mesh).contains::<KitStyled>());
        assert!(world.entity(aus_mesh).contains::<KitStyled>());
    }

    #[test]
    fn player_skin_tone_index_is_deterministic_and_in_range() {
        for seed in 0..2000_u32 {
            let a = player_skin_tone_index(seed);
            let b = player_skin_tone_index(seed);
            assert_eq!(a, b, "same seed must give the same tone every time");
            assert!(a < PLAYER_SKIN_TONES.len());
        }
    }

    #[test]
    fn player_skin_seed_is_deterministic_per_team_and_role() {
        let a = player_skin_seed("IND", FigureKind::Fielder(3));
        let b = player_skin_seed("IND", FigureKind::Fielder(3));
        assert_eq!(a, b);
        let batter = player_skin_seed("IND", FigureKind::Batter);
        let bowler = player_skin_seed("IND", FigureKind::Bowler);
        assert_ne!(
            batter, bowler,
            "distinct roles should not collide trivially"
        );
        let other_team = player_skin_seed("AUS", FigureKind::Batter);
        assert_ne!(
            batter, other_team,
            "distinct teams should not collide trivially"
        );
    }

    #[test]
    fn player_skin_tones_span_a_broad_range() {
        // Section 1 asks for a richer set than the crowd's 6 tones, spanning
        // caucasian / south-asian / african ranges.
        assert!(PLAYER_SKIN_TONES.len() > 6);
        let mut seen = std::collections::HashSet::new();
        for seed in 0..5000_u32 {
            seen.insert(player_skin_tone_index(seed));
        }
        assert_eq!(
            seen.len(),
            PLAYER_SKIN_TONES.len(),
            "every tone in the palette should be reachable"
        );
    }

    #[test]
    fn apply_team_kit_materials_uses_named_skin_slot_over_legacy_classification() {
        use crate::core::teams::builtin_teams;

        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<Assets<StandardMaterial>>();
        app.init_resource::<Assets<Image>>();
        app.add_systems(Update, apply_team_kit_materials);

        let india = &builtin_teams()[0];
        let crest = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default());
        let skin_palette = build_player_skin_palette(
            &mut app.world_mut().resource_mut::<Assets<StandardMaterial>>(),
        );
        let expected_idx =
            player_skin_tone_index(player_skin_seed(&india.short, FigureKind::Batter));
        let expected_handle = skin_palette.skin[expected_idx].clone();
        app.insert_resource(skin_palette);

        let fig = app
            .world_mut()
            .spawn((
                Figure {
                    kind: FigureKind::Batter,
                },
                TeamKit {
                    primary_color: india.primary_color,
                    secondary_color: india.secondary_color,
                    kit_style: india.kit_style,
                    crest: crest.clone(),
                    team_short: india.short.clone(),
                    player_name: None,
                    squad_number: None,
                },
            ))
            .id();
        // A colour that would otherwise be classified as `Joints` by the
        // legacy path — proves the named slot takes precedence rather than
        // falling through to colour classification.
        let baked = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial {
                base_color: Color::srgb(0.1, 0.1, 0.1),
                ..Default::default()
            });
        let mesh = app
            .world_mut()
            .spawn((
                GltfMaterialName("Skin".to_string()),
                Mesh3d(Handle::default()),
                MeshMaterial3d(baked),
                ChildOf(fig),
            ))
            .id();

        app.update();

        let world = app.world();
        let handle = world
            .entity(mesh)
            .get::<MeshMaterial3d<StandardMaterial>>()
            .unwrap()
            .0
            .clone();
        assert_eq!(
            handle, expected_handle,
            "Skin slot should use the shared palette handle"
        );
        assert!(world.entity(mesh).contains::<KitStyled>());
    }

    #[test]
    fn apply_team_kit_materials_builds_composited_shirt_texture_for_named_slot() {
        use crate::core::teams::builtin_teams;

        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<Assets<StandardMaterial>>();
        app.init_resource::<Assets<Image>>();
        app.add_systems(Update, apply_team_kit_materials);

        let india = &builtin_teams()[0];
        let crest = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default());

        let fig = app
            .world_mut()
            .spawn((
                Figure {
                    kind: FigureKind::Bowler,
                },
                TeamKit {
                    primary_color: india.primary_color,
                    secondary_color: india.secondary_color,
                    kit_style: india.kit_style,
                    crest: crest.clone(),
                    team_short: india.short.clone(),
                    player_name: None,
                    squad_number: None,
                },
            ))
            .id();
        let baked = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default());
        let mesh = app
            .world_mut()
            .spawn((
                GltfMaterialName("Shirt".to_string()),
                Mesh3d(Handle::default()),
                MeshMaterial3d(baked),
                ChildOf(fig),
            ))
            .id();

        app.update();

        let world = app.world();
        let handle = world
            .entity(mesh)
            .get::<MeshMaterial3d<StandardMaterial>>()
            .unwrap()
            .0
            .clone();
        let materials = world.resource::<Assets<StandardMaterial>>();
        let images = world.resource::<Assets<Image>>();
        let tex_handle = materials
            .get(&handle)
            .and_then(|m| m.base_color_texture.clone())
            .expect("named Shirt slot should get a composited texture");
        let image = images.get(&tex_handle).unwrap();
        assert_eq!(image.texture_descriptor.size.width, kit::SHIRT_TEXTURE_SIZE);
        assert!(world.entity(mesh).contains::<KitStyled>());
    }
}

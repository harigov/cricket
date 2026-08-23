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
use bevy::gltf::GltfAssetLabel;
use std::time::Duration;

use bevy::camera::visibility::NoFrustumCulling;
use bevy::prelude::*;

use crate::core::teams::{KitStyle, Team};

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
pub(crate) struct BoneBindPose(pub Quat);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoneKind {
    Hips,
    Spine,
    Spine1,
    Spine2,
    Neck,
    Head,
    LeftArm,
    LeftForeArm,
    LeftHand,
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
        "LeftArm" => Some(BoneKind::LeftArm),
        "LeftForeArm" => Some(BoneKind::LeftForeArm),
        "LeftHand" => Some(BoneKind::LeftHand),
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
}

/// Crest badge already parented to the chest bone.
#[derive(Component)]
pub(crate) struct CrestAttached;

/// Bind-pose foot height above the imported armature origin (metres).
const FOOT_BIND_Y: f32 = 0.0844;
/// Lift the streamed scene so bind-pose feet sit on the pitch (y = 0).
const SCENE_GROUND_Y: f32 = -FOOT_BIND_Y;
/// Mixamo hips rest translation in armature space (metres, after glTF scale).
const HIPS_BIND_TRANSLATION: Vec3 = Vec3::new(0.0, 1.039_914_7, 0.020_760_939);

/// Mixamo bone local translations are centimetre-like; the imported `Armature`
/// node applies `scale = 0.01` so `mixamorig:Hips` y = 103.99 → 1.04 m in
/// world space. Equipment parented to a bone must be sized and offset in bone
/// units, not metres.
const BONE_UNITS_PER_METRE: f32 = 100.0;

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
    let scene = crate::render::load_xbot_scene(asset_server);
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
        // Scene offset grounds bind-pose feet on y = 0 (see FOOT_BIND_Y).
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

/// Mesh rows still awaiting a team-kit tint: entity, optional glTF node name
/// and the PBR material handle imported with the figure.
type UnstyledKitMesh<'a> = (
    Entity,
    Option<&'a Name>,
    &'a MeshMaterial3d<StandardMaterial>,
);
/// Only untinted figure meshes — equipment (bat, pads) is recoloured elsewhere.
type UnstyledKitMeshFilter = (Without<KitStyled>, With<Mesh3d>, Without<Equipment>);

/// Keep the imported PBR materials and tint them into believable cricket kit:
/// `Beta_Surface` becomes the long-sleeve jersey/trousers (stronger primary
/// tint), `Beta_Joints` takes the secondary colour as trim/helmet shade.
pub fn apply_team_kit_materials(
    mut commands: Commands,
    kits: Query<&TeamKit>,
    parents: Query<&ChildOf>,
    meshes: Query<UnstyledKitMesh, UnstyledKitMeshFilter>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    for (entity, name, mat_handle) in &meshes {
        let mut cur = parents.get(entity).ok().map(ChildOf::parent);
        let mut kit = None;
        for _ in 0..32 {
            let Some(parent) = cur else { break };
            if let Ok(found) = kits.get(parent) {
                kit = Some(found);
                break;
            }
            cur = parents.get(parent).ok().map(ChildOf::parent);
        }
        let Some(kit) = kit else { continue };
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
        let Some(kind) = bone_kind_for_name(name.as_str()) else {
            continue;
        };
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
        commands
            .entity(ent)
            .insert((Bone { figure: fig, kind }, BoneBindPose(transform.rotation)));

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
    // hand +Z so the blade hangs down beside the pads in the idle mocap grip.
    let swing = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2) * Quat::from_rotation_y(-0.14);
    let blade_tf = equipment_transform_m(Vec3::new(-0.02, 0.0, 0.36), swing);
    let handle_tf = equipment_transform_m(Vec3::new(-0.02, 0.0, -0.14), swing);
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

/// Strip vertical root motion from mocap clips so feet stay grounded.
pub fn strip_skeleton_root_motion(mut bones: Query<(&Bone, &mut Transform)>) {
    for (bone, mut tf) in &mut bones {
        if bone.kind == BoneKind::Hips {
            tf.translation = HIPS_BIND_TRANSLATION;
        }
    }
}

// ---------------------------------------------------------------------------
// Hybrid animation controller
// ---------------------------------------------------------------------------

const BLEND_RATE: f32 = 12.0;
const BOWL_SETTLE_SECS: f32 = 0.85;

/// Every figure kind uses the shared idle mocap clip while in [`AnimState::Idle`].
fn idle_state_uses_locomotion_clip(_kind: FigureKind) -> bool {
    true
}

/// Locomotion clip selection for hybrid animation (idle/run clips vs procedural).
fn locomotion_clip_for_anim(
    state: AnimState,
    kind: FigureKind,
    clips: &LocomotionClips,
) -> Option<(AnimationNodeIndex, ClipState)> {
    match state {
        AnimState::Idle if idle_state_uses_locomotion_clip(kind) => {
            Some((clips.idle, ClipState::Idle))
        }
        AnimState::Run { .. } => Some((clips.run, ClipState::Run)),
        _ => None,
    }
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
            AnimState::Idle => idle_sway(t_global, &mut pose),
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
            AnimState::Throw { p } => throw_pose(*p, &mut pose),
        }
        apply_pose(fig_ent, &pose, blend, &mut bones);
    }
}

/// Compose a procedural delta (authored in bone-local space) onto the imported
/// bind rotation. Identity delta restores the bind pose.
fn compose_pose_rotation(bind: Quat, delta: Quat) -> Quat {
    bind * delta
}

/// Local rotation targets per bone for one frame of procedural animation.
#[derive(Default)]
struct PoseTargets {
    hips: Quat,
    spine: Quat,
    spine1: Quat,
    spine2: Quat,
    neck: Quat,
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
/// arms gathered low in front ready to swing.
fn batter_stance(t: f32, pose: &mut PoseTargets) {
    let breathe = (t * 1.4).sin();
    let crouch_knees = 0.52 + breathe * 0.02;
    pose.lup = rx(crouch_knees);
    pose.rup = rx(crouch_knees * 1.04);
    pose.ll = rx(-0.66 - breathe * 0.02);
    pose.rl = rx(-0.70);
    pose.hips = rz(0.05) * rx(-0.12);
    pose.spine = rx(0.34 + breathe * 0.015) * ry(-0.28);
    pose.neck = ry(0.42); // eyes up toward the bowler
    // Both arms down in front, hands together on the handle.
    pose.ra = rx(0.62) * rz(-0.34);
    pose.rfa = rx(0.85);
    pose.la = rx(0.58) * rz(0.30);
    pose.lfa = rx(0.95);
    pose.rf = rx(-0.25);
    pose.lf = rx(-0.25);
}

/// Relaxed fielder/bowler idle with subtle weight shift.
fn idle_sway(t: f32, pose: &mut PoseTargets) {
    let sway = (t * 0.7).sin() * 0.045;
    pose.spine = rz(sway);
    pose.hips = rz(-sway * 0.4);
    pose.ra = rx(0.06 * (t * 0.9).sin());
    pose.la = rx(-0.06 * (t * 0.9).sin());
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
    let back_leg = kf(
        &[(0.0, 0.0), (0.50, -0.85), (0.70, -0.55), (1.0, -0.22)],
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
    pose.rl = rx(kf(&[(0.0, 0.0), (0.5, 0.95), (1.0, 0.45)], pc));
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
    pose.ra = rz(arm_z * 0.85) * rx(arm_x) * rz(-0.34);
    pose.la = rz(arm_z * 0.60) * rx(arm_x * 0.94) * rz(0.30);
    pose.rfa = rx(bend);
    pose.lfa = rx(bend * 0.92);
    pose.spine = ry(spine_y) * rx(0.20);
    pose.hips = ry(hips_y);
    // Weight shifts onto the front foot through contact.
    pose.lup = rx(kf(&[(0.0, 0.50), (0.58, 0.26), (1.0, 0.20)], pc));
    pose.rup = rx(kf(&[(0.0, 0.54), (0.58, 0.66), (1.0, 0.70)], pc));
    pose.ll = rx(kf(&[(0.0, -0.66), (0.58, -0.52), (1.0, -0.38)], pc));
    pose.rl = rx(kf(&[(0.0, -0.70), (0.58, -0.62), (1.0, -0.55)], pc));
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
        let target = compose_pose_rotation(bind.0, delta);
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
        assert!((metres_to_bone(1.0) - 100.0).abs() < 1e-5);
        assert!((metres_to_bone(0.44) - 44.0).abs() < 1e-5);
        let tf = equipment_transform_m(Vec3::new(0.0, -0.44, 0.10), Quat::IDENTITY);
        assert!((tf.translation.y + 44.0).abs() < 1e-4);
        assert!((tf.translation.z - 10.0).abs() < 1e-4);
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
    }

    /// The colour fallback in `kit_mesh_kind` classifies willow and white gear
    /// as jersey material, so equipment must be excluded from the recolour pass
    /// by marker, not by colour. Regression: the bat rendered in team colours.
    #[test]
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
        let target = compose_pose_rotation(bind, Quat::IDENTITY);
        assert!(
            target.dot(bind).abs() > 0.999,
            "expected bind, got {target:?}"
        );
    }

    #[test]
    fn compose_pose_delta_is_local_to_bind() {
        let bind = Quat::from_rotation_x(0.5);
        let delta = Quat::from_rotation_z(0.25);
        let target = compose_pose_rotation(bind, delta);
        let expected = bind * delta;
        assert!(target.dot(expected).abs() > 0.999);
    }

    #[test]
    fn all_figure_kinds_use_idle_clip_in_idle_state() {
        let kinds = [
            FigureKind::Batter,
            FigureKind::NonStriker,
            FigureKind::Bowler,
            FigureKind::Keeper,
            FigureKind::Fielder(0),
            FigureKind::Umpire,
        ];
        for kind in kinds {
            assert!(
                idle_state_uses_locomotion_clip(kind),
                "{kind:?} should use idle locomotion clip",
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
                .is_some(),
                "{kind:?} should resolve to idle clip",
            );
        }
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
}

//! Player figures – realistic human meshes via MIT Xbot glTF (Mixamo rig).
//! The figure entity holds `Figure` + `Anim` + `Transform` + `SceneRoot`.
//! A post-spawn system tags Mixamo bones with `Bone` so `animate_skeleton`
//! can drive them code-driven (no external clips needed).

use bevy::prelude::*;
use bevy::gltf::GltfAssetLabel;

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Figure { pub kind: FigureKind }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FigureKind { Batter, NonStriker, Bowler, Keeper, Fielder(usize), Umpire }

#[derive(Component, Default)]
pub struct Anim { pub state: AnimState }

#[derive(Clone, Copy, Debug)]
pub enum AnimState { Idle, Run { t: f32 }, BowlAction { p: f32 }, BatSwing { p: f32 }, Throw { p: f32 } }
impl Default for AnimState { fn default() -> Self { AnimState::Idle } }

#[derive(Component, Debug)]
pub struct Bone { pub figure: Entity, pub kind: BoneKind }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoneKind {
    Hips, Spine, Spine1, Spine2, Neck, Head,
    LeftArm, LeftForeArm, LeftHand,
    RightArm, RightForeArm, RightHand,
    LeftUpLeg, LeftLeg, LeftFoot,
    RightUpLeg, RightLeg, RightFoot,
}
fn bone_kind_for_name(name: &str) -> Option<BoneKind> {
    match name {
        "mixamorig:Hips" => Some(BoneKind::Hips),
        "mixamorig:Spine" => Some(BoneKind::Spine),
        "mixamorig:Spine1" => Some(BoneKind::Spine1),
        "mixamorig:Spine2" => Some(BoneKind::Spine2),
        "mixamorig:Neck" => Some(BoneKind::Neck),
        "mixamorig:Head" => Some(BoneKind::Head),
        "mixamorig:LeftArm" => Some(BoneKind::LeftArm),
        "mixamorig:LeftForeArm" => Some(BoneKind::LeftForeArm),
        "mixamorig:LeftHand" => Some(BoneKind::LeftHand),
        "mixamorig:RightArm" => Some(BoneKind::RightArm),
        "mixamorig:RightForeArm" => Some(BoneKind::RightForeArm),
        "mixamorig:RightHand" => Some(BoneKind::RightHand),
        "mixamorig:LeftUpLeg" => Some(BoneKind::LeftUpLeg),
        "mixamorig:LeftLeg" => Some(BoneKind::LeftLeg),
        "mixamorig:LeftFoot" => Some(BoneKind::LeftFoot),
        "mixamorig:RightUpLeg" => Some(BoneKind::RightUpLeg),
        "mixamorig:RightLeg" => Some(BoneKind::RightLeg),
        "mixamorig:RightFoot" => Some(BoneKind::RightFoot),
        _ => None,
    }
}

#[derive(Component)] pub struct Bat;

// Legacy – kept so old queries don't break, but no longer spawned.
#[derive(Component)] pub struct Part { pub kind: PartKind }
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum PartKind { LegL, LegR, ArmL, ArmR }

pub fn spawn_figure(
    commands: &mut Commands,
    asset_server: &AssetServer,
    pos: Vec3,
    facing_deg: f32,
    _shirt: Color,
    _trousers: Color,
    kind: FigureKind,
) -> Entity {
    let scene = asset_server.load(GltfAssetLabel::Scene(0).from_asset("characters/Xbot.glb"));
    let fig = commands.spawn((
        Figure { kind },
        Anim::default(),
        Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(facing_deg.to_radians())),
        Visibility::default(),
        InheritedVisibility::default(),
        ViewVisibility::default(),
    )).id();
    // Scene is a child offset so feet sit on y=0 (hips ~0.92m up)
    commands.entity(fig).with_children(|p| {
        p.spawn((
            SceneRoot(scene),
            Transform::from_xyz(0.0, 0.92, 0.0),
            Visibility::default(),
            InheritedVisibility::default(),
            ViewVisibility::default(),
        ));
    });
    fig
}

/// Tag newly spawned Mixamo bones. Walks up `ChildOf` chain to find the
/// owning `Figure` entity and inserts `Bone`. Also attaches a bat to
/// batters' right hand.
pub fn tag_skeleton_bones(
    mut commands: Commands,
    figures: Query<(Entity, &Figure)>,
    // All Name entities without Bone yet (potential bones)
    candidates: Query<(Entity, &Name, Option<&ChildOf>), Without<Bone>>,
    // Need to know parent of any entity for walk-up – separate query that
    // includes all entities that have ChildOf
    parents: Query<&ChildOf>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Build figure lookup
    let figure_set: std::collections::HashSet<Entity> = figures.iter().map(|(e,_)| e).collect();
    let figure_kind: std::collections::HashMap<Entity, FigureKind> =
        figures.iter().map(|(e,f)| (e, f.kind)).collect();

    for (ent, name, child_of) in &candidates {
        let Some(kind) = bone_kind_for_name(name.as_str()) else { continue };
        // Walk up to find Figure
        let mut cur = child_of.map(|c| c.parent());
        let mut fig_ent = None;
        let mut steps = 0;
        while let Some(p) = cur {
            if figure_set.contains(&p) { fig_ent = Some(p); break; }
            // Move up one more level
            if let Ok(child) = parents.get(p) { cur = Some(child.parent()); } else { break; }
            steps += 1;
            if steps > 16 { break; }
        }
        let Some(fig) = fig_ent else { continue };
        commands.entity(ent).insert(Bone { figure: fig, kind });

        // Attach bat to batters' right hand (once)
        if kind == BoneKind::RightHand {
            if let Some(k) = figure_kind.get(&fig) {
                if matches!(k, FigureKind::Batter | FigureKind::NonStriker) {
                    let bat_mesh = meshes.add(Cuboid::new(0.055, 0.85, 0.11));
                    let bat_mat = materials.add(Color::srgb_u8(0xD9, 0xB4, 0x6A));
                    commands.entity(ent).with_children(|p| {
                        p.spawn((
                            Bat,
                            Mesh3d(bat_mesh),
                            MeshMaterial3d(bat_mat),
                            Transform::from_xyz(0.02, -0.12, 0.06).with_rotation(Quat::from_rotation_x(-0.35)),
                        ));
                    });
                }
            }
        }
    }
}

const RUN_FREQ: f32 = 13.0;
const RUN_AMP: f32 = 0.72;

pub fn yaw_to_face(dir: bevy::math::Vec2) -> f32 { dir.y.atan2(-dir.x) }

/// Legacy – does nothing now (kept for RenderPlugin compatibility).
pub fn animate_figures(
    _time: Res<Time>,
    _figures: Query<(&Anim, &Children, Option<&Figure>)>,
    _parts: Query<(&Part, &mut Transform)>,
) {}

/// Drive Xbot skeleton from `AnimState`.
pub fn animate_skeleton(
    time: Res<Time>,
    figures: Query<(Entity, &Figure, &Anim)>,
    mut bones: Query<(&Bone, &mut Transform)>,
) {
    let t_global = time.elapsed_secs();
    for (fig_ent, fig, anim) in &figures {
        // Compute targets
        let mut hips = Quat::IDENTITY;
        let mut spine = Quat::IDENTITY;
        let spine1 = Quat::IDENTITY;
        let spine2 = Quat::IDENTITY;
        let head = Quat::IDENTITY;
        let mut la = Quat::IDENTITY;
        let mut ra = Quat::IDENTITY;
        let mut lfa = Quat::IDENTITY;
        let mut rfa = Quat::IDENTITY;
        let mut lup = Quat::IDENTITY;
        let mut rup = Quat::IDENTITY;
        let mut ll = Quat::IDENTITY;
        let mut rl = Quat::IDENTITY;

        let is_batter = matches!(fig.kind, FigureKind::Batter | FigureKind::NonStriker);
        match anim.state {
            AnimState::Idle => {
                if is_batter {
                    let w = (t_global * 1.3).sin() * 0.13;
                    ra = Quat::from_rotation_z(w);
                    la = Quat::from_rotation_z(-w * 0.45);
                    hips = Quat::from_rotation_z((t_global*0.9).sin()*0.05);
                } else {
                    let sway = (t_global*0.7).sin()*0.04;
                    spine = Quat::from_rotation_z(sway);
                }
            }
            AnimState::Run { t } => {
                let ph = t * RUN_FREQ;
                let a = ph.sin()*RUN_AMP;
                lup = Quat::from_rotation_x(a*0.55);
                rup = Quat::from_rotation_x(-a*0.55);
                ll = Quat::from_rotation_x(a*0.32);
                rl = Quat::from_rotation_x(-a*0.32);
                la = Quat::from_rotation_x(-a*0.62);
                ra = Quat::from_rotation_x(a*0.62);
                hips = Quat::from_rotation_x((ph*2.0).sin()*0.05);
            }
            AnimState::BowlAction { p } => {
                let pc = p.clamp(0.0,1.0);
                ra = Quat::from_rotation_x(lerp(2.6, -2.1, pc));
                rfa = Quat::from_rotation_x(-0.55*pc);
                la = Quat::from_rotation_z(-0.42);
                lup = Quat::from_rotation_x(0.48*(1.0-pc));
                rup = Quat::from_rotation_x(-0.58*(1.0-pc)+0.78*pc);
                spine = Quat::from_rotation_x(0.22*pc);
            }
            AnimState::BatSwing { p } => {
                let pc = (p.clamp(0.0,1.0)*2.0).min(1.0);
                let ang = lerp(1.25, -2.05, pc);
                ra = Quat::from_rotation_z(ang*0.68) * Quat::from_rotation_x(0.28);
                la = Quat::from_rotation_z(ang*0.52);
                rfa = Quat::from_rotation_x(0.42);
                lfa = Quat::from_rotation_x(0.42);
                spine = Quat::from_rotation_y(ang*0.14);
            }
            AnimState::Throw { p } => {
                let pc = p.clamp(0.0,1.0);
                ra = Quat::from_rotation_x(lerp(-2.25, 0.7, pc));
                rfa = Quat::from_rotation_x(lerp(-0.85, 0.2, pc));
            }
        }
        let blend = (14.0 * time.delta_secs()).clamp(0.0,1.0);
        let drift = blend*0.45;
        for (bone, mut tf) in &mut bones {
            if bone.figure != fig_ent { continue; }
            let target = match bone.kind {
                BoneKind::Hips => hips,
                BoneKind::Spine => spine,
                BoneKind::Spine1 => spine1,
                BoneKind::Spine2 => spine2,
                BoneKind::Neck | BoneKind::Head => head,
                BoneKind::LeftArm => la,
                BoneKind::RightArm => ra,
                BoneKind::LeftForeArm => lfa,
                BoneKind::RightForeArm => rfa,
                BoneKind::LeftUpLeg => lup,
                BoneKind::RightUpLeg => rup,
                BoneKind::LeftLeg => ll,
                BoneKind::RightLeg => rl,
                _ => Quat::IDENTITY,
            };
            if target != Quat::IDENTITY {
                tf.rotation = tf.rotation.slerp(target, blend);
            } else {
                tf.rotation = tf.rotation.slerp(Quat::IDENTITY, drift);
            }
            let _ = (spine1, spine2, head); // keep bindings used
        }
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 { a + (b - a) * t }

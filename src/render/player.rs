//! Procedural player figures built from primitives, with lightweight
//! code-driven animation (running, bowling action, bat swing, throw).

use bevy::prelude::*;

/// Identifies which player this figure represents on the field.
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

/// Animation state machine, driven by gameplay systems.
#[derive(Component, Default)]
pub struct Anim {
    pub state: AnimState,
}

#[derive(Clone, Copy, Debug)]
pub enum AnimState {
    Idle,
    Run { t: f32 },
    /// Bowling delivery stride; `p` goes 0..1 over the action.
    BowlAction { p: f32 },
    BatSwing { p: f32 },
    Throw { p: f32 },
}

impl Default for AnimState {
    fn default() -> Self {
        AnimState::Idle
    }
}

// ---- part marker components ----
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
#[derive(Component)] pub struct Bat;

const SKIN: Color = Color::srgb_u8(0xC6, 0x93, 0x6B);

pub fn spawn_figure(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    pos: Vec3,
    facing_deg: f32,
    shirt: Color,
    trousers: Color,
    kind: FigureKind,
) -> Entity {
    let shirt_mat = materials.add(shirt);
    let trouser_mat = materials.add(trousers);
    let skin_mat = materials.add(SKIN);

    let torso_mesh = meshes.add(Capsule3d::new(0.17, 0.42));
    let head_mesh = meshes.add(Sphere::new(0.115));
    let limb_mesh = meshes.add(Capsule3d::new(0.065, 0.62));

    let is_batter =
        matches!(kind, FigureKind::Batter | FigureKind::NonStriker);

    let id = commands
        .spawn((
            Figure { kind },
            Anim::default(),
            Transform::from_translation(pos)
                .with_rotation(Quat::from_rotation_y(facing_deg.to_radians())),
            Visibility::default(),
        ))
        .id();

    commands.entity(id).with_children(|p| {
        // Torso & head
        p.spawn((
            Mesh3d(torso_mesh.clone()),
            MeshMaterial3d(shirt_mat.clone()),
            Transform::from_xyz(0.0, 1.08, 0.0),
        ));
        p.spawn((
            Mesh3d(head_mesh.clone()),
            MeshMaterial3d(skin_mat.clone()),
            Transform::from_xyz(0.0, 1.52, 0.0),
        ));

        // Legs: pivot at hip, mesh hangs below so rotation looks natural.
        for (kind, x) in [(PartKind::LegL, -0.09), (PartKind::LegR, 0.09)] {
            p.spawn((
                Part { kind },
                Transform::from_xyz(x, 0.78, 0.0),
            )).with_children(|c| {
                c.spawn((
                    Mesh3d(limb_mesh.clone()),
                    MeshMaterial3d(trouser_mat.clone()),
                    Transform::from_xyz(0.0, -0.38, 0.0),
                ));
            });
        }

        // Arms: pivot at shoulders.
        for (kind, x) in [(PartKind::ArmL, -0.26), (PartKind::ArmR, 0.26)] {
            p.spawn((
                Part { kind },
                Transform::from_xyz(x, 1.34, 0.0),
            )).with_children(|c| {
                c.spawn((
                    Mesh3d(limb_mesh.clone()),
                    MeshMaterial3d(shirt_mat.clone()),
                    Transform::from_xyz(0.0, -0.30, 0.0),
                ));
                // Bat hangs off the right arm of batters.
                if kind == PartKind::ArmR && is_batter {
                    c.spawn((
                        Bat,
                        Mesh3d(meshes.add(Cuboid::new(0.055, 0.85, 0.11))),
                        MeshMaterial3d(materials.add(
                            Color::srgb_u8(0xD9, 0xB4, 0x6A))),
                        Transform::from_xyz(0.02, -0.72, 0.06)
                            .with_rotation(Quat::from_rotation_x(-0.35)),
                    ));
                }
            });
        }
    });

    id
}

const RUN_FREQ: f32 = 13.0;
const RUN_AMP: f32 = 0.75;

/// Yaw (radians) that makes a figure face the given XZ direction.
/// Figures are modelled facing -X at yaw 0.
pub fn yaw_to_face(dir: bevy::math::Vec2) -> f32 {
    dir.y.atan2(-dir.x)
}

pub fn animate_figures(
    figures: Query<(&Anim, &Children)>,
    mut parts: Query<(&Part, &mut Transform)>,
) {
    for (anim, children) in &figures {
        // Compute desired rotations per part kind.
        let (mut ll, mut lr, mut al, mut ar) = (0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32);
        match anim.state {
            AnimState::Idle => {}
            AnimState::Run { t } => {
                let phase = t * RUN_FREQ;
                ll = phase.sin() * RUN_AMP;
                lr = -ll;
                al = -phase.sin() * RUN_AMP * 0.7;
                ar = -al;
            }
            AnimState::BowlAction { p } => {
                // Delivery stride: right arm windmills over the top.
                ar = lerp(2.6, -2.2, p.clamp(0.0, 1.0));
                al = -0.4;
                ll = 0.6 * (1.0 - p);
                lr = -0.6 * (1.0 - p) + 0.9 * p;
            }
            AnimState::BatSwing { p } => {
                // Horizontal swing across the body.
                ar = lerp(1.4, -2.4, (p.clamp(0.0, 1.0) * 2.0).min(1.0));
                al = ar * 0.5;
                ll = 0.15;
                lr = -0.15;
            }
            AnimState::Throw { p } => {
                ar = lerp(-2.4, 0.8, p.clamp(0.0, 1.0));
                al = 0.3;
            }
        }
        let targets = [
            (PartKind::LegL, ll),
            (PartKind::LegR, lr),
            (PartKind::ArmL, al),
            (PartKind::ArmR, ar),
        ];
        for child in children.iter() {
            if let Ok((part, mut tf)) = parts.get_mut(child) {
                if let Some((_, angle)) =
                    targets.iter().find(|(k, _)| *k == part.kind)
                {
                    // Rotate pivots about Z (swings limbs in the XY plane,
                    // which reads well from the standard camera angles).
                    tf.rotation = Quat::from_rotation_z(*angle);
                }
            }
        }
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

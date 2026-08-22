use std::f32::consts::{FRAC_PI_2, PI, TAU};


use crate::core::geometry as geo;
use crate::core::stadiums::Stadium;
use bevy::prelude::*;

#[derive(Component)]
pub struct StadiumRoot;

#[derive(Component)]
pub struct Stumps {
    /// true = striker's (batsman) end.
    pub striker_end: bool,
}

const STUMP_GAP: f32 = 0.114; // half distance between outer stumps

pub fn build_stadium(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    stadium: &Stadium,
) -> Entity {
    let root = commands
        .spawn((StadiumRoot, Transform::default(), Visibility::default()))
        .id();

    let outfield_mat = mat(stadium.outfield_color);
    let pitch_mat = mat(Color::srgb_u8(0xC8, 0xA9, 0x7A));
    let white_mat = mat(Color::WHITE);
    let stump_mat = mat(Color::srgb_u8(0xF5, 0xE9, 0xC8));
    let stand_mat = mat(stadium.stand_color);

    commands.entity(root).with_children(|p| {
        // ---- Outfield ----
        let r = stadium.boundary_radius();
        p.spawn((
            Mesh3d(meshes.add(Circle::new(r + 6.0))),
            MeshMaterial3d(materials.add(outfield_mat)),
            Transform::from_rotation(Quat::from_rotation_x(-FRAC_PI_2)),
        ));

        // ---- Pitch strip ----
        p.spawn((
            Mesh3d(meshes.add(Plane3d::default()
                .mesh()
                .size(geo::PITCH_LENGTH + 2.0, geo::PITCH_WIDTH))),
            MeshMaterial3d(materials.add(pitch_mat)),
            Transform::from_translation(Vec3::Y * 0.01)
                .with_rotation(Quat::from_rotation_x(-FRAC_PI_2)),
        ));

        // ---- Creases ----
        for sign in [-1.0_f32, 1.0] {
            let x = sign * (geo::PITCH_HALF_LEN - geo::CREASE_DEPTH);
            p.spawn((
                Mesh3d(meshes.add(Plane3d::default().mesh().size(0.06, geo::PITCH_WIDTH))),
                MeshMaterial3d(materials.add(white_mat.clone())),
                Transform::from_translation(Vec3::new(x, 0.02, 0.0))
                    .with_rotation(Quat::from_rotation_x(-FRAC_PI_2)),
            ));
            for z in [-geo::PITCH_WIDTH / 2.0, geo::PITCH_WIDTH / 2.0] {
                p.spawn((
                    Mesh3d(meshes.add(Plane3d::default()
                        .mesh()
                        .size(geo::CREASE_DEPTH * 2.0, 0.06))),
                    MeshMaterial3d(materials.add(white_mat.clone())),
                    Transform::from_translation(Vec3::new(x - sign * 1.1, 0.02, z))
                        .with_rotation(Quat::from_rotation_x(-FRAC_PI_2)),
                ));
            }
        }

        // ---- Boundary rope ----
        let rope_mat = mat(Color::srgb_u8(0xEE, 0xEE, 0xEE));
        for seg in 0..96 {
            let a0 = seg as f32 / 96.0 * TAU;
            let a1 = (seg + 1) as f32 / 96.0 * TAU;
            let mid = (a0 + a1) / 2.0;
            let len = 2.0 * r * (PI / 96.0);
            p.spawn((
                Mesh3d(meshes.add(Cuboid::new(len, 0.08, 0.08))),
                MeshMaterial3d(materials.add(rope_mat.clone())),
                Transform::from_translation(Vec3::new(mid.cos() * r, 0.05, mid.sin() * r))
                    .with_rotation(Quat::from_rotation_y(-mid)),
            ));
        }
    });

    // ---- Stumps both ends (marked entities so gameplay can find them) ----
    for sign in [-1.0_f32, 1.0] {
        let striker_end = sign > 0.0;
        let end_root = commands
            .spawn((
                Stumps { striker_end },
                Transform::from_xyz(sign * geo::PITCH_HALF_LEN, 0.0, 0.0),
            ))
            .id();
        for i in -1..=1_i32 {
            commands.entity(end_root).with_children(|p| {
                p.spawn((
                    Mesh3d(meshes.add(Cylinder::new(0.02, geo::STUMP_HEIGHT))),
                    MeshMaterial3d(materials.add(stump_mat.clone())),
                    Transform::from_xyz(
                        0.0,
                        geo::STUMP_HEIGHT / 2.0,
                        i as f32 * STUMP_GAP,
                    ),
                ));
            });
        }
        commands.entity(end_root).with_children(|p| {
            p.spawn((
                Mesh3d(meshes.add(Cuboid::new(0.03, 0.02, STUMP_GAP * 2.0))),
                MeshMaterial3d(materials.add(stump_mat.clone())),
                Transform::from_xyz(0.0, geo::STUMP_HEIGHT + 0.01, 0.0),
            ));
        });
        commands.entity(root).add_child(end_root);
    }

    // ---- Stands + floodlights ----
    commands.entity(root).with_children(|p| {
        let r = stadium.boundary_radius();
        for seg in 0..40 {
            let a = seg as f32 / 40.0 * TAU;
            for tier in 0..3 {
                let dist = r + 8.0 + tier as f32 * 4.5;
                let h = 4.5 + tier as f32 * 3.0;
                p.spawn((
                    Mesh3d(meshes.add(Cuboid::new(10.5, h, 3.0))),
                    MeshMaterial3d(materials.add(stand_mat.clone())),
                    Transform::from_translation(Vec3::new(
                        a.cos() * dist,
                        h / 2.0 - 1.0,
                        a.sin() * dist,
                    ))
                    .with_rotation(Quat::from_rotation_y(-a + FRAC_PI_2)),
                ));
            }
            if seg % 8 == 0 {
                let tower_r = r + 11.0;
                p.spawn((
                    Mesh3d(meshes.add(Cylinder::new(0.35, 30.0))),
                    MeshMaterial3d(materials.add(mat(Color::srgb_u8(0x88, 0x88, 0x90)))),
                    Transform::from_translation(Vec3::new(a.cos() * tower_r, 15.0, a.sin() * tower_r)),
                ));
                p.spawn((
                    Mesh3d(meshes.add(Cuboid::new(4.0, 2.0, 0.5))),
                    MeshMaterial3d(materials.add(mat(Color::srgb(3.0, 3.0, 2.8)))),
                    Transform::from_translation(Vec3::new(a.cos() * tower_r, 31.0, a.sin() * tower_r))
                        .with_rotation(Quat::from_rotation_y(-a)),
                ));
            }
        }
    });

    root
}

fn mat(color: Color) -> StandardMaterial {
    StandardMaterial {
        base_color: color,
        perceptual_roughness: 0.9,
        ..Default::default()
    }
}

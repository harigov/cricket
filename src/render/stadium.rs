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

    let outfield_base = stadium.outfield_color;
    let pitch_mat = mat(Color::srgb_u8(0xC8, 0xA9, 0x7A));
    let pitch_worn_mat = mat(Color::srgb_u8(0xB8, 0x9A, 0x6E));
    let white_mat = mat(Color::WHITE);
    let stump_mat = mat(Color::srgb_u8(0xF5, 0xE9, 0xC8));
    let stand_mat = mat(stadium.stand_color);
    let sight_screen_mat = mat(Color::srgb_u8(0x1A, 0x1A, 0x1E));

    commands.entity(root).with_children(|p| {
        // ---- Striped outfield: concentric annuli alternating two greens ----
        let r = stadium.boundary_radius() + 6.0;
        let stripe_count = 10;
        let step = r / stripe_count as f32;
        let base_srgba = outfield_base.to_srgba();
        let light = Color::srgb(
            (base_srgba.red * 1.42).min(1.0),
            (base_srgba.green * 1.32).min(1.0),
            (base_srgba.blue * 1.18).min(1.0),
        );
        for i in 0..stripe_count {
            let inner = i as f32 * step;
            let outer = (i + 1) as f32 * step;
            let col = if i % 2 == 0 { outfield_base } else { light };
            let mesh: Handle<Mesh> = if inner < 0.1 {
                meshes.add(Circle::new(outer))
            } else {
                meshes.add(Annulus::new(inner, outer))
            };
            // Inner rings higher so they sit cleanly atop outer ones
            let y = i as f32 * 0.003;
            p.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(materials.add(mat(col))),
                Transform::from_translation(Vec3::Y * y)
                    .with_rotation(Quat::from_rotation_x(-FRAC_PI_2)),
            ));
        }

        // ---- Pitch strip (with worn center strip) ----
        p.spawn((
            Mesh3d(meshes.add(
                Plane3d::default()
                    .mesh()
                    .size(geo::PITCH_LENGTH + 2.0, geo::PITCH_WIDTH),
            )),
            MeshMaterial3d(materials.add(pitch_mat)),
            Transform::from_translation(Vec3::Y * 0.05),
        ));
        // Worn darker strip down the middle
        p.spawn((
            Mesh3d(meshes.add(
                Plane3d::default()
                    .mesh()
                    .size(geo::PITCH_LENGTH + 1.0, geo::PITCH_WIDTH * 0.35),
            )),
            MeshMaterial3d(materials.add(pitch_worn_mat)),
            Transform::from_translation(Vec3::Y * 0.06),
        ));

        // ---- Creases ----
        for sign in [-1.0_f32, 1.0] {
            let x = sign * (geo::PITCH_HALF_LEN - geo::CREASE_DEPTH);
            p.spawn((
                Mesh3d(meshes.add(Plane3d::default().mesh().size(0.06, geo::PITCH_WIDTH))),
                MeshMaterial3d(materials.add(white_mat.clone())),
                Transform::from_translation(Vec3::new(x, 0.07, 0.0)),
            ));
            for z in [-geo::PITCH_WIDTH / 2.0, geo::PITCH_WIDTH / 2.0] {
                p.spawn((
                    Mesh3d(meshes.add(
                        Plane3d::default()
                            .mesh()
                            .size(geo::CREASE_DEPTH * 2.0, 0.06),
                    )),
                    MeshMaterial3d(materials.add(white_mat.clone())),
                    Transform::from_translation(Vec3::new(x - sign * 1.1, 0.07, z)),
                ));
            }
        }

        // ---- Boundary rope + low ad-wall behind it ----
        let rope_mat = mat(Color::srgb_u8(0xEE, 0xEE, 0xEE));
        let wall_mat = mat(Color::srgb_u8(0x22, 0x44, 0x22));
        for seg in 0..96 {
            let a0 = seg as f32 / 96.0 * TAU;
            let a1 = (seg + 1) as f32 / 96.0 * TAU;
            let mid = (a0 + a1) / 2.0;
            let r = stadium.boundary_radius();
            let len = 2.0 * r * (PI / 96.0);
            // rope
            p.spawn((
                Mesh3d(meshes.add(Cuboid::new(len, 0.08, 0.08))),
                MeshMaterial3d(materials.add(rope_mat.clone())),
                Transform::from_translation(Vec3::new(mid.cos() * r, 0.05, mid.sin() * r))
                    .with_rotation(Quat::from_rotation_y(-mid)),
            ));
            // low wall segment behind rope (every 3rd)
            if seg % 4 == 0 {
                let wall_r = r + 1.2;
                p.spawn((
                    Mesh3d(meshes.add(Cuboid::new(len * 1.05, 0.6, 0.15))),
                    MeshMaterial3d(materials.add(wall_mat.clone())),
                    Transform::from_translation(Vec3::new(
                        mid.cos() * wall_r,
                        0.30,
                        mid.sin() * wall_r,
                    ))
                    .with_rotation(Quat::from_rotation_y(-mid)),
                ));
            }
        }

        // ---- Sight screens behind each set of stumps ----
        for sign in [-1.0_f32, 1.0] {
            let r = stadium.boundary_radius();
            // place 2m inside boundary along pitch axis
            let x = sign * (r - 2.0);
            p.spawn((
                Mesh3d(meshes.add(Cuboid::new(0.15, 4.2, 10.0))),
                MeshMaterial3d(materials.add(sight_screen_mat.clone())),
                Transform::from_translation(Vec3::new(x, 2.1, 0.0)),
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

    // ---- Stands + floodlights + crowd blobs ----
    commands.entity(root).with_children(|p| {
        let r = stadium.boundary_radius();
        // Pre-make crowd blob mesh/material variants
        let crowd_mesh = meshes.add(Cuboid::new(0.62, 0.85, 0.52));
        let crowd_cols = [
            Color::srgb_u8(0xF5, 0xF5, 0xF5),
            Color::srgb_u8(0xE8, 0x2A, 0x2A),
            Color::srgb_u8(0x2A, 0x6A, 0xE8),
            Color::srgb_u8(0xFF, 0xD5, 0x40),
            Color::srgb_u8(0x1A, 0xB0, 0x4A),
        ];
        let crowd_mats: Vec<_> = crowd_cols
            .iter()
            .map(|c| materials.add(mat(*c)))
            .collect();

        for seg in 0..40 {
            let a = seg as f32 / 40.0 * TAU;
            let ca = a.cos();
            let sa = a.sin();
            for tier in 0..3 {
                let dist = r + 8.0 + tier as f32 * 4.5;
                let h = 4.5 + tier as f32 * 3.0;
                // Main stand block
                p.spawn((
                    Mesh3d(meshes.add(Cuboid::new(10.5, h, 3.0))),
                    MeshMaterial3d(materials.add(stand_mat.clone())),
                    Transform::from_translation(Vec3::new(ca * dist, h / 2.0 - 1.0, sa * dist))
                        .with_rotation(Quat::from_rotation_y(-a + FRAC_PI_2)),
                ));
                // Crowd blobs on top of this tier (4 per segment)
                let top_y = h - 1.0 + 0.60;
                for k in 0..4 {
                    let off_along = (k as f32 - 1.5) * 1.9;
                    let tx = -sa;
                    let tz = ca;
                    let col_idx = ((seg * 7 + tier * 13 + k * 5) as usize) % crowd_mats.len();
                    let hj = ((seg * 11 + tier * 17 + k * 7) % 5) as f32 * 0.05;
                    p.spawn((
                        Mesh3d(crowd_mesh.clone()),
                        MeshMaterial3d(crowd_mats[col_idx].clone()),
                        Transform::from_translation(Vec3::new(
                            ca * dist + tx * off_along,
                            top_y + hj,
                            sa * dist + tz * off_along,
                        )),
                    ));
                }
            }
            if seg % 8 == 0 {
                let tower_r = r + 11.0;
                p.spawn((
                    Mesh3d(meshes.add(Cylinder::new(0.35, 30.0))),
                    MeshMaterial3d(materials.add(mat(Color::srgb_u8(0x88, 0x88, 0x90)))),
                    Transform::from_translation(Vec3::new(ca * tower_r, 15.0, sa * tower_r)),
                ));
                p.spawn((
                    Mesh3d(meshes.add(Cuboid::new(4.0, 2.0, 0.5))),
                    MeshMaterial3d(materials.add(mat(Color::srgb(3.0, 3.0, 2.8)))),
                    Transform::from_translation(Vec3::new(ca * tower_r, 31.0, sa * tower_r))
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

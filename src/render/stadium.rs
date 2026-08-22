use std::f32::consts::{FRAC_PI_2, PI, TAU};

use crate::core::geometry as geo;
use crate::core::stadiums::Stadium;
use crate::core::teams::Team;
use bevy::gltf::GltfAssetLabel;
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
    asset_server: &AssetServer,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    stadium: &Stadium,
    batting_team: &Team,
    fielding_team: &Team,
) -> Entity {
    let root = commands
        .spawn((
            StadiumRoot,
            Transform::default(),
            Visibility::default(),
            InheritedVisibility::default(),
            ViewVisibility::default(),
        ))
        .id();

    let outfield_base = stadium.outfield_color;
    let pitch_mat = mat(Color::srgb_u8(0xC8, 0xA9, 0x7A));
    let pitch_worn_mat = mat(Color::srgb_u8(0xB8, 0x9A, 0x6E));
    let white_mat = mat(Color::WHITE);
    let stump_mat = mat(Color::srgb_u8(0xF5, 0xE9, 0xC8));
    let sight_screen_mat = mat(Color::srgb_u8(0x1A, 0x1A, 0x1E));

    commands.entity(root).with_children(|p| {
        // ---- Realistic outer stadium shell (Poly Pizza CC-BY, 104KB) ----
        // Low-poly stylized arena scaled to our boundary. Provides tiered
        // seating, roof and floodlight towers – far more realistic than
        // procedural cuboids, yet only 1.6k tris.
        let stadium_scene = asset_server.load(
            GltfAssetLabel::Scene(0).from_asset("stadium/poly_stadium.glb"),
        );
        // Poly stadium is ~214m wide, we scale ~0.62 to match 60-68m boundary + outer apron
        let scale = (stadium.boundary_radius() + 12.0) / 107.0;
        p.spawn((
            SceneRoot(stadium_scene),
            Transform::from_translation(Vec3::Y * -0.8).with_scale(Vec3::splat(scale)),
            Visibility::default(),
            InheritedVisibility::default(),
            ViewVisibility::default(),
        ));

        // ---- Striped outfield: concentric annuli alternating two greens ----
        // Kept procedural for crisp mown stripes that the Poly model lacks.
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
            let y = i as f32 * 0.003 + 0.01;
            p.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: col,
                    perceptual_roughness: 0.95,
                    ..mat(col)
                })),
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
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb_u8(0xC8, 0xA9, 0x7A),
                perceptual_roughness: 0.85,
                ..pitch_mat
            })),
            Transform::from_translation(Vec3::Y * 0.05),
        ));
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

        // ---- Boundary rope + broadcast sponsor wall + team crest pylons ----
        let rope_mat = mat(Color::srgb_u8(0xEE, 0xEE, 0xEE));
        let sponsor_mat = materials.add(texture_mat(
            crate::render::load_sponsor_ribbon(asset_server),
        ));
        let batting_crest_mat = materials.add(texture_mat(
            crate::render::load_team_crest(asset_server, &batting_team.crest_asset()),
        ));
        let fielding_crest_mat = materials.add(texture_mat(
            crate::render::load_team_crest(asset_server, &fielding_team.crest_asset()),
        ));
        let board_frame_mat = materials.add(mat(Color::srgb_u8(0x08, 0x12, 0x1C)));
        for seg in 0..96 {
            let a0 = seg as f32 / 96.0 * TAU;
            let a1 = (seg + 1) as f32 / 96.0 * TAU;
            let mid = (a0 + a1) / 2.0;
            let r = stadium.boundary_radius();
            let len = 2.0 * r * (PI / 96.0);
            p.spawn((
                Mesh3d(meshes.add(Cuboid::new(len, 0.08, 0.08))),
                MeshMaterial3d(materials.add(rope_mat.clone())),
                Transform::from_translation(Vec3::new(mid.cos() * r, 0.05, mid.sin() * r))
                    .with_rotation(Quat::from_rotation_y(-mid)),
            ));
            if seg % 2 == 0 {
                let wall_r = r + 1.2;
                let board_width = len * 1.85;
                p.spawn((
                    Mesh3d(meshes.add(Cuboid::new(board_width + 0.14, 1.52, 0.16))),
                    MeshMaterial3d(board_frame_mat.clone()),
                    Transform::from_translation(Vec3::new(
                        mid.cos() * (wall_r + 0.02),
                        0.78,
                        mid.sin() * (wall_r + 0.02),
                    ))
                    .with_rotation(Quat::from_rotation_y(-mid)),
                ));
                p.spawn((
                    Mesh3d(meshes.add(Cuboid::new(board_width, 1.35, 0.18))),
                    MeshMaterial3d(sponsor_mat.clone()),
                    Transform::from_translation(Vec3::new(
                        mid.cos() * wall_r,
                        0.78,
                        mid.sin() * wall_r,
                    ))
                    .with_rotation(Quat::from_rotation_y(-mid)),
                ));
            }

            // Eight square identity pylons alternate the two match teams.
            if seg % 12 == 6 {
                let crest_r = r + 1.48;
                let crest_mat = if (seg / 12) % 2 == 0 {
                    batting_crest_mat.clone()
                } else {
                    fielding_crest_mat.clone()
                };
                p.spawn((
                    Mesh3d(meshes.add(Cuboid::new(2.42, 2.42, 0.20))),
                    MeshMaterial3d(board_frame_mat.clone()),
                    Transform::from_translation(Vec3::new(
                        mid.cos() * (crest_r + 0.03),
                        1.34,
                        mid.sin() * (crest_r + 0.03),
                    ))
                    .with_rotation(Quat::from_rotation_y(-mid)),
                ));
                p.spawn((
                    Mesh3d(meshes.add(Cuboid::new(2.24, 2.24, 0.23))),
                    MeshMaterial3d(crest_mat),
                    Transform::from_translation(Vec3::new(
                        mid.cos() * crest_r,
                        1.34,
                        mid.sin() * crest_r,
                    ))
                    .with_rotation(Quat::from_rotation_y(-mid)),
                ));
            }
        }

        // ---- Sight screens behind each set of stumps ----
        for sign in [-1.0_f32, 1.0] {
            let r = stadium.boundary_radius();
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
                Visibility::default(),
                InheritedVisibility::default(),
                ViewVisibility::default(),
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

    // ---- Realistic crowd: Kenney Blocky Characters CC0 (4 variants, 113KB each) ----
    // Replaces 480 cuboid blobs (5k tris, flat colors) with ~120 low-poly
    // humans (1k tris each, ~120k tris total) seated on the Poly stadium's
    // tiers. Each is a full glTF with PBR, far more realistic yet still
    // performant via instanced SceneRoots.
    let crowd_variants = [
        asset_server.load(GltfAssetLabel::Scene(0).from_asset("crowd/crowd-a.glb")),
        asset_server.load(GltfAssetLabel::Scene(0).from_asset("crowd/crowd-b.glb")),
        asset_server.load(GltfAssetLabel::Scene(0).from_asset("crowd/crowd-c.glb")),
        asset_server.load(GltfAssetLabel::Scene(0).from_asset("crowd/crowd-d.glb")),
    ];

    commands.entity(root).with_children(|p| {
        let r = stadium.boundary_radius();
        // Place crowd in 30 segments x 3 tiers x ~1-2 per tier = ~120
        for seg in 0..30 {
            let a = seg as f32 / 30.0 * TAU;
            let ca = a.cos();
            let sa = a.sin();
            for tier in 0..3 {
                // Skip some tiers randomly for natural gaps
                if (seg + tier) % 7 == 0 { continue; }
                let dist = r + 9.5 + tier as f32 * 4.2;
                let h = 3.8 + tier as f32 * 2.8;
                // Tangential direction along stands
                let tx = -sa;
                let tz = ca;
                let count = if tier == 2 { 2 } else { 1 };
                for k in 0..count {
                    let off_along = (k as f32 - 0.5) * 1.6 + ((seg * 13 + tier * 7) % 3) as f32 * 0.2;
                    let variant = crowd_variants[(seg * 7 + tier * 11 + k * 5) % crowd_variants.len()].clone();
                    // Face the pitch (inward)
                    let yaw = -a + FRAC_PI_2 + std::f32::consts::PI;
                    // Slight random scale 0.88-1.02 and Y jitter for natural variation
                    let s = 0.88 + ((seg * 11 + tier * 17 + k * 13) % 7) as f32 * 0.02;
                    let yj = ((seg * 19 + tier * 23 + k * 11) % 5) as f32 * 0.04;
                    p.spawn((
                        SceneRoot(variant),
                        Transform::from_translation(Vec3::new(
                            ca * dist + tx * off_along,
                            h - 0.9 + yj,
                            sa * dist + tz * off_along,
                        ))
                        .with_rotation(Quat::from_rotation_y(yaw))
                        .with_scale(Vec3::splat(s * 0.92)),
                        Visibility::default(),
                        InheritedVisibility::default(),
                        ViewVisibility::default(),
                    ));
                }
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

fn texture_mat(texture: Handle<Image>) -> StandardMaterial {
    StandardMaterial {
        base_color_texture: Some(texture),
        perceptual_roughness: 0.72,
        unlit: true,
        cull_mode: None,
        ..Default::default()
    }
}

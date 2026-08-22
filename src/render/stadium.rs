use std::f32::consts::{PI, TAU};

use crate::core::geometry as geo;
use crate::core::stadiums::Stadium;
use crate::core::teams::Team;
use crate::render::outfield_grass::{self, MOW_BAND_COUNT};
use crate::render::ring_geometry::{
    floodlight_angles, floodlight_radius, ring_face_center_rotation, ring_position,
    ring_segment_transform, ring_tangent,
};
use crate::render::{FloodlightFixture, FloodlightMaterials, NightEnvironmentLight};
use bevy::gltf::GltfAssetLabel;
use bevy::prelude::*;

#[derive(Component)]
pub struct StadiumRoot;

#[derive(Component)]
pub struct Stumps {
    /// true = striker's (batsman) end.
    pub striker_end: bool,
}

const STUMP_GAP: f32 = 0.114;
const TIER_COUNT: usize = 5;
const TIER_SEGMENTS: usize = 96;
const AISLE_EVERY: usize = 12;
const CROWD_SEGMENTS: usize = 90;
const CROWD_AISLE_EVERY: usize = 10;

struct BowlLayout {
    inner_radius: f32,
    tier_depth: f32,
    tier_rise: f32,
    tread_thickness: f32,
    base_height: f32,
}

impl BowlLayout {
    fn from_boundary(boundary: f32) -> Self {
        Self {
            inner_radius: boundary + 3.2,
            tier_depth: 1.9,
            tier_rise: 0.74,
            tread_thickness: 0.52,
            base_height: 0.42,
        }
    }

    fn outer_radius(&self) -> f32 {
        self.inner_radius + self.tier_depth * TIER_COUNT as f32
    }

    fn tier_mid_radius(&self, tier: usize) -> f32 {
        self.inner_radius + (tier as f32 + 0.5) * self.tier_depth
    }

    fn tier_height(&self, tier: usize) -> f32 {
        self.base_height + tier as f32 * self.tier_rise
    }
}

struct SharedStadiumAssets {
    unit_cuboid: Handle<Mesh>,
    rope_mesh: Handle<Mesh>,
    column_mesh: Handle<Mesh>,
    tower_pole_mesh: Handle<Mesh>,
    tower_truss_mesh: Handle<Mesh>,
    lamp_bank_mesh: Handle<Mesh>,
    rope_mat: Handle<StandardMaterial>,
    white_mat: Handle<StandardMaterial>,
    stump_mat: Handle<StandardMaterial>,
    sight_screen_mat: Handle<StandardMaterial>,
    board_frame_mat: Handle<StandardMaterial>,
    tier_mats: [Handle<StandardMaterial>; TIER_COUNT],
    riser_mat: Handle<StandardMaterial>,
    rail_mat: Handle<StandardMaterial>,
    column_mat: Handle<StandardMaterial>,
    canopy_mat: Handle<StandardMaterial>,
    tower_mat: Handle<StandardMaterial>,
    lamp_day_mat: Handle<StandardMaterial>,
    lamp_night_mat: Handle<StandardMaterial>,
    sponsor_mats: Vec<Handle<StandardMaterial>>,
    grass_tex: Handle<Image>,
    grass_mesh: Handle<Mesh>,
}

fn build_shared_assets(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    asset_server: &AssetServer,
    stadium: &Stadium,
) -> SharedStadiumAssets {
    let sc = stadium.stand_color.to_srgba();
    let tint = |mul: f32, add: f32| {
        Color::srgb(
            (sc.red * mul + add).clamp(0.0, 1.0),
            (sc.green * mul + add).clamp(0.0, 1.0),
            (sc.blue * mul + add).clamp(0.0, 1.0),
        )
    };

    let tier_mats: [Handle<StandardMaterial>; TIER_COUNT] = std::array::from_fn(|i| {
        let shade = 1.0 - i as f32 * 0.08;
        materials.add(mat(tint(shade, 0.04)))
    });

    SharedStadiumAssets {
        unit_cuboid: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        rope_mesh: meshes.add(Cuboid::new(1.0, 0.08, 0.08)),
        column_mesh: meshes.add(Cylinder::new(0.22, 1.0)),
        tower_pole_mesh: meshes.add(Cylinder::new(0.38, 1.0)),
        tower_truss_mesh: meshes.add(Cuboid::new(1.0, 0.35, 0.35)),
        lamp_bank_mesh: meshes.add(Cuboid::new(1.0, 0.55, 0.42)),
        rope_mat: materials.add(mat(Color::srgb_u8(0xEE, 0xEE, 0xEE))),
        white_mat: materials.add(mat(Color::WHITE)),
        stump_mat: materials.add(mat(Color::srgb_u8(0xF5, 0xE9, 0xC8))),
        sight_screen_mat: materials.add(mat(Color::srgb_u8(0x1A, 0x1A, 0x1E))),
        board_frame_mat: materials.add(mat(Color::srgb_u8(0x08, 0x12, 0x1C))),
        tier_mats,
        riser_mat: materials.add(mat(tint(0.62, 0.02))),
        rail_mat: materials.add(mat(tint(0.48, 0.03))),
        column_mat: materials.add(mat(tint(0.55, 0.06))),
        canopy_mat: materials.add(mat(tint(0.42, 0.05))),
        tower_mat: materials.add(mat(Color::srgb_u8(0x48, 0x4C, 0x52))),
        lamp_day_mat: materials.add(StandardMaterial {
            base_color: Color::srgb_u8(0xC8, 0xCE, 0xD4),
            emissive: LinearRgba::from(Color::srgb(0.08, 0.09, 0.11)),
            perceptual_roughness: 0.55,
            ..default()
        }),
        lamp_night_mat: materials.add(StandardMaterial {
            base_color: Color::srgb_u8(0xF2, 0xF0, 0xE6),
            emissive: LinearRgba::from(Color::srgb(1.35, 1.28, 1.05)),
            perceptual_roughness: 0.45,
            ..default()
        }),
        sponsor_mats: vec![
            materials.add(texture_mat(
                crate::render::load_sponsor_ribbon(asset_server),
            )),
            materials.add(texture_mat(images.add(sponsor_board_image(0)))),
            materials.add(texture_mat(images.add(sponsor_board_image(1)))),
        ],
        grass_tex: images.add(crate::render::create_outfield_grass_image()),
        grass_mesh: meshes.add(
            Plane3d::default()
                .mesh()
                .size(1.0, 1.0)
                .subdivisions(4),
        ),
    }
}

pub fn build_stadium(
    commands: &mut Commands,
    asset_server: &AssetServer,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    stadium: &Stadium,
    batting_team: &Team,
    fielding_team: &Team,
) -> Entity {
    let shared = build_shared_assets(meshes, materials, images, asset_server, stadium);
    let bowl = BowlLayout::from_boundary(stadium.boundary_radius());
    let outfield_base = stadium.outfield_color;

    let pitch_img = images.add(create_pitch_image());
    let pitch_mat = StandardMaterial {
        base_color: Color::srgb_u8(0xC8, 0xA9, 0x7A),
        base_color_texture: Some(pitch_img.clone()),
        perceptual_roughness: 0.92,
        ..Default::default()
    };
    let pitch_worn_mat = StandardMaterial {
        base_color: Color::srgb_u8(0xB8, 0x9A, 0x6E),
        base_color_texture: Some(pitch_img),
        perceptual_roughness: 0.96,
        ..Default::default()
    };

    let batting_crest_mat = materials.add(texture_mat(
        crate::render::load_team_crest(asset_server, &batting_team.crest_asset()),
    ));
    let fielding_crest_mat = materials.add(texture_mat(
        crate::render::load_team_crest(asset_server, &fielding_team.crest_asset()),
    ));

    let root = commands
        .spawn((
            StadiumRoot,
            Transform::default(),
            Visibility::default(),
            InheritedVisibility::default(),
            ViewVisibility::default(),
        ))
        .id();

    commands.entity(root).with_children(|p| {
        // Outfield grass bands (shared mesh + texture).
        let r = stadium.boundary_radius() + 6.0;
        let span = r * 2.05;
        let band_width = span / MOW_BAND_COUNT as f32;
        let half_span = span / 2.0;
        for band in 0..MOW_BAND_COUNT {
            let x_min = -half_span + band as f32 * band_width;
            let x_center = x_min + band_width / 2.0;
            let grass_mat = StandardMaterial {
                base_color: outfield_grass::tinted_mow_band_color(outfield_base, band),
                base_color_texture: Some(shared.grass_tex.clone()),
                perceptual_roughness: 0.88,
                metallic: 0.0,
                reflectance: 0.42,
                uv_transform: outfield_grass::strip_uv_transform(span, band_width, x_min),
                ..default()
            };
            p.spawn((
                Mesh3d(shared.grass_mesh.clone()),
                MeshMaterial3d(materials.add(grass_mat)),
                Transform::from_translation(Vec3::new(x_center, 0.01, 0.0))
                    .with_scale(Vec3::new(band_width, 1.0, span)),
            ));
        }

        // Pitch
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

        // Creases
        for sign in [-1.0_f32, 1.0] {
            let x = sign * (geo::PITCH_HALF_LEN - geo::CREASE_DEPTH);
            p.spawn((
                Mesh3d(meshes.add(Plane3d::default().mesh().size(0.06, geo::PITCH_WIDTH))),
                MeshMaterial3d(shared.white_mat.clone()),
                Transform::from_translation(Vec3::new(x, 0.07, 0.0)),
            ));
            for z in [-geo::PITCH_WIDTH / 2.0, geo::PITCH_WIDTH / 2.0] {
                p.spawn((
                    Mesh3d(meshes.add(
                        Plane3d::default()
                            .mesh()
                            .size(geo::CREASE_DEPTH * 2.0, 0.06),
                    )),
                    MeshMaterial3d(shared.white_mat.clone()),
                    Transform::from_translation(Vec3::new(x - sign * 1.1, 0.07, z)),
                ));
            }
        }

        // Boundary rope + sponsor boards (ring-oriented).
        let br = stadium.boundary_radius();
        for seg in 0..TIER_SEGMENTS {
            let a0 = seg as f32 / TIER_SEGMENTS as f32 * TAU;
            let a1 = (seg + 1) as f32 / TIER_SEGMENTS as f32 * TAU;
            let mid = (a0 + a1) / 2.0;
            let len = 2.0 * br * (PI / TIER_SEGMENTS as f32);
            p.spawn((
                Mesh3d(shared.rope_mesh.clone()),
                MeshMaterial3d(shared.rope_mat.clone()),
                ring_segment_transform(mid, br, 0.05).with_scale(Vec3::new(len, 1.0, 1.0)),
            ));
            if seg % 2 == 0 {
                let wall_r = br + 1.2;
                let board_width = len * 1.85;
                p.spawn((
                    Mesh3d(shared.unit_cuboid.clone()),
                    MeshMaterial3d(shared.board_frame_mat.clone()),
                    ring_segment_transform(mid, wall_r + 0.02, 0.78)
                        .with_scale(Vec3::new(board_width + 0.14, 1.52, 0.16)),
                ));
                p.spawn((
                    Mesh3d(shared.unit_cuboid.clone()),
                    MeshMaterial3d(shared.sponsor_mats[seg % shared.sponsor_mats.len()].clone()),
                    ring_segment_transform(mid, wall_r, 0.78)
                        .with_scale(Vec3::new(board_width, 1.35, 0.18)),
                ));
            }
            if seg % 12 == 6 {
                let crest_r = br + 1.48;
                let crest_mat = if (seg / 12) % 2 == 0 {
                    batting_crest_mat.clone()
                } else {
                    fielding_crest_mat.clone()
                };
                p.spawn((
                    Mesh3d(shared.unit_cuboid.clone()),
                    MeshMaterial3d(shared.board_frame_mat.clone()),
                    ring_segment_transform(mid, crest_r + 0.03, 1.34)
                        .with_scale(Vec3::new(2.42, 2.42, 0.20)),
                ));
                p.spawn((
                    Mesh3d(shared.unit_cuboid.clone()),
                    MeshMaterial3d(crest_mat),
                    ring_segment_transform(mid, crest_r, 1.34)
                        .with_scale(Vec3::new(2.24, 2.24, 0.23)),
                ));
            }
        }

        // Sight screens
        for sign in [-1.0_f32, 1.0] {
            let x = sign * (br - 2.5);
            p.spawn((
                Mesh3d(shared.unit_cuboid.clone()),
                MeshMaterial3d(shared.sight_screen_mat.clone()),
                Transform::from_translation(Vec3::new(x, 1.65, 0.0))
                    .with_scale(Vec3::new(0.12, 3.2, 7.5)),
            ));
        }

        // ---- Continuous raked seating bowl ----
        let arc_w = 2.0 * PI * bowl.inner_radius / TIER_SEGMENTS as f32 * 1.02;
        let tread_arc = arc_w;
        let tread_radial = bowl.tier_depth * 0.92;

        for tier in 0..TIER_COUNT {
            let mid_r = bowl.tier_mid_radius(tier);
            let h = bowl.tier_height(tier);
            let mat = shared.tier_mats[tier].clone();

            for seg in 0..TIER_SEGMENTS {
                if seg % AISLE_EVERY == 0 {
                    continue;
                }
                let mid = (seg as f32 + 0.5) / TIER_SEGMENTS as f32 * TAU;
                p.spawn((
                    Mesh3d(shared.unit_cuboid.clone()),
                    MeshMaterial3d(mat.clone()),
                    ring_segment_transform(mid, mid_r, h + bowl.tread_thickness * 0.5)
                        .with_scale(Vec3::new(tread_arc, bowl.tread_thickness, tread_radial)),
                ));
            }

            // Riser face at inner edge of each tier (except ground).
            if tier > 0 {
                let inner_r = bowl.inner_radius + tier as f32 * bowl.tier_depth - 0.08;
                for seg in 0..TIER_SEGMENTS {
                    if seg % AISLE_EVERY == 0 {
                        continue;
                    }
                    let mid = (seg as f32 + 0.5) / TIER_SEGMENTS as f32 * TAU;
                    let riser_h = bowl.tier_rise;
                    p.spawn((
                        Mesh3d(shared.unit_cuboid.clone()),
                        MeshMaterial3d(shared.riser_mat.clone()),
                        ring_segment_transform(mid, inner_r, h - riser_h * 0.5)
                            .with_scale(Vec3::new(tread_arc * 0.98, riser_h, 0.14)),
                    ));
                }
            }

            // Guard rails on upper tiers.
            if tier >= 2 {
                let rail_r = mid_r - tread_radial * 0.38;
                for seg in 0..TIER_SEGMENTS {
                    if seg % AISLE_EVERY == 0 {
                        continue;
                    }
                    let mid = (seg as f32 + 0.5) / TIER_SEGMENTS as f32 * TAU;
                    p.spawn((
                        Mesh3d(shared.unit_cuboid.clone()),
                        MeshMaterial3d(shared.rail_mat.clone()),
                        ring_segment_transform(mid, rail_r, h + bowl.tread_thickness + 0.12)
                            .with_scale(Vec3::new(tread_arc * 0.95, 0.16, 0.10)),
                    ));
                }
            }
        }

        // Support columns at aisle junctions.
        let outer = bowl.outer_radius();
        for seg in (0..TIER_SEGMENTS).step_by(AISLE_EVERY) {
            let a = seg as f32 / TIER_SEGMENTS as f32 * TAU;
            let col_r = bowl.inner_radius + bowl.tier_depth * 2.5;
            let col_h = bowl.tier_height(TIER_COUNT - 1) + bowl.tread_thickness + 1.8;
            p.spawn((
                Mesh3d(shared.column_mesh.clone()),
                MeshMaterial3d(shared.column_mat.clone()),
                Transform::from_translation(ring_position(a, col_r, col_h * 0.5))
                    .with_scale(Vec3::new(1.0, col_h, 1.0)),
            ));
        }

        // Modest canopy ring at the top of the bowl.
        let canopy_r = outer + 1.2;
        let canopy_h = bowl.tier_height(TIER_COUNT - 1) + bowl.tread_thickness + 2.6;
        for seg in 0..TIER_SEGMENTS {
            if seg % AISLE_EVERY == 0 {
                continue;
            }
            let mid = (seg as f32 + 0.5) / TIER_SEGMENTS as f32 * TAU;
            p.spawn((
                Mesh3d(shared.unit_cuboid.clone()),
                MeshMaterial3d(shared.canopy_mat.clone()),
                ring_segment_transform(mid, canopy_r, canopy_h)
                    .with_scale(Vec3::new(tread_arc * 1.05, 0.22, 2.4)),
            ));
        }

        // ---- Floodlight towers (visible fixtures + night spotlights) ----
        let tower_r = floodlight_radius(outer);
        let tower_h = 30.0;
        for angle in floodlight_angles() {
            let base = ring_position(angle, tower_r, 0.0);
            let top = ring_position(angle, tower_r, tower_h);

            p.spawn((
                Mesh3d(shared.tower_pole_mesh.clone()),
                MeshMaterial3d(shared.tower_mat.clone()),
                Transform::from_translation(Vec3::new(base.x, tower_h * 0.5, base.z))
                    .with_scale(Vec3::new(1.0, tower_h, 1.0)),
            ));
            p.spawn((
                Mesh3d(shared.tower_truss_mesh.clone()),
                MeshMaterial3d(shared.tower_mat.clone()),
                Transform::from_translation(Vec3::new(top.x, tower_h - 0.8, top.z))
                    .with_scale(Vec3::new(3.6, 1.0, 1.0))
                    .with_rotation(ring_segment_transform(angle, tower_r, tower_h).rotation),
            ));
            for offset in [-1.35_f32, 1.35_f32] {
                let tangent = ring_tangent(angle);
                let lamp_pos = top + tangent * offset;
                p.spawn((
                    FloodlightFixture,
                    Mesh3d(shared.lamp_bank_mesh.clone()),
                    MeshMaterial3d(shared.lamp_day_mat.clone()),
                    Transform::from_translation(lamp_pos)
                        .with_rotation(ring_segment_transform(angle, tower_r, tower_h).rotation)
                        .with_scale(Vec3::new(1.4, 1.0, 1.0)),
                ));
            }
            // SpotLight aimed at pitch centre — hidden by day via NightEnvironmentLight.
            // Four towers × broad beams: televised floodlit readability without flat wash.
            p.spawn((
                NightEnvironmentLight,
                SpotLight {
                    color: Color::srgb(1.0, 0.97, 0.90),
                    intensity: 9_500_000.0,
                    range: 145.0,
                    radius: 2.4,
                    shadows_enabled: true,
                    outer_angle: 0.82,
                    inner_angle: 0.52,
                    ..default()
                },
                Transform::from_translation(Vec3::new(top.x, tower_h - 1.2, top.z))
                    .looking_at(Vec3::new(0.0, 0.5, 0.0), Vec3::Y),
                Visibility::Hidden,
            ));
        }

        // ---- Crowd: ~350–550 seated spectators ----
        let crowd_variants = [
            asset_server.load(GltfAssetLabel::Scene(0).from_asset("crowd/crowd-a.glb")),
            asset_server.load(GltfAssetLabel::Scene(0).from_asset("crowd/crowd-b.glb")),
            asset_server.load(GltfAssetLabel::Scene(0).from_asset("crowd/crowd-c.glb")),
            asset_server.load(GltfAssetLabel::Scene(0).from_asset("crowd/crowd-d.glb")),
        ];
        let crowd_scale = 0.62;
        let mut crowd_count = 0usize;

        for seg in 0..CROWD_SEGMENTS {
            if seg % CROWD_AISLE_EVERY == 0 {
                continue;
            }
            for tier in 0..TIER_COUNT {
                // Stagger each tier's seat ring so figures don't stack in radial columns.
                let tier_phase =
                    ((tier * 19 + 7) % CROWD_SEGMENTS) as f32 / CROWD_SEGMENTS as f32 * TAU;
                let seg_jitter = ((seg * 3 + tier * 13) % 5) as f32 - 2.0;
                let mid = (seg as f32 + 0.5 + seg_jitter * 0.18) / CROWD_SEGMENTS as f32 * TAU
                    + tier_phase;
                let seats = 1 + ((seg * 7 + tier * 11) % 3 == 0) as usize;
                let seat_r = bowl.tier_mid_radius(tier) - 0.15;
                let seat_h = bowl.tier_height(tier) + bowl.tread_thickness - 0.06;
                let tangent = ring_tangent(mid);
                for k in 0..seats {
                    let off = (k as f32 - (seats as f32 - 1.0) * 0.5) * 0.95
                        + ((seg * 13 + tier * 5 + k) % 7) as f32 * 0.04;
                    let pos = ring_position(mid, seat_r, seat_h) + tangent * off;
                    let variant = crowd_variants[(seg * 7 + tier * 11 + k * 5) % 4].clone();
                    let s = 0.94 + ((seg * 11 + tier * 17 + k * 13) % 7) as f32 * 0.014;
                    let rot = ring_face_center_rotation(mid) * Quat::from_rotation_x(-0.26);
                    p.spawn((
                        SceneRoot(variant),
                        Transform::from_translation(pos)
                            .with_rotation(rot)
                            .with_scale(Vec3::splat(s * crowd_scale)),
                        Visibility::default(),
                        InheritedVisibility::default(),
                        ViewVisibility::default(),
                    ));
                    crowd_count += 1;
                }
            }
        }
        info!("Stadium crowd spawned: {crowd_count} spectators");

        // Big screen + dugouts + tents (unchanged layout, shared materials).
        let screen_frame = materials.add(mat(Color::srgb_u8(0x10, 0x12, 0x16)));
        let screen_face = materials.add(texture_mat(images.add(big_screen_image(
            &batting_team.short.to_uppercase(),
            &fielding_team.short.to_uppercase(),
        ))));
        let sx = -(br - 2.5);
        p.spawn((
            Mesh3d(shared.unit_cuboid.clone()),
            MeshMaterial3d(screen_frame.clone()),
            Transform::from_translation(Vec3::new(sx - 1.1, 3.6, 0.0))
                .with_scale(Vec3::new(0.32, 3.8, 8.0)),
        ));
        p.spawn((
            Mesh3d(shared.unit_cuboid.clone()),
            MeshMaterial3d(screen_face),
            Transform::from_translation(Vec3::new(sx - 0.92, 3.6, 0.0))
                .with_scale(Vec3::new(0.18, 3.2, 7.0)),
        ));

        let dugout_roof = materials.add(mat(Color::srgb_u8(0xE8, 0xE6, 0xDF)));
        for sign_z in [-1.0_f32, 1.0] {
            let dz = sign_z * (br - 6.0);
            p.spawn((
                Mesh3d(shared.unit_cuboid.clone()),
                MeshMaterial3d(dugout_roof.clone()),
                Transform::from_translation(Vec3::new(sx + 8.0, 2.6, dz))
                    .with_rotation(Quat::from_rotation_z(-sign_z * 0.08))
                    .with_scale(Vec3::new(6.5, 0.25, 2.6)),
            ));
            p.spawn((
                Mesh3d(shared.unit_cuboid.clone()),
                MeshMaterial3d(screen_frame.clone()),
                Transform::from_translation(Vec3::new(sx + 11.2, 1.15, dz))
                    .with_scale(Vec3::new(6.5, 2.2, 0.22)),
            ));
            p.spawn((
                Mesh3d(shared.unit_cuboid.clone()),
                MeshMaterial3d(dugout_roof.clone()),
                Transform::from_translation(Vec3::new(sx + 5.2, 0.55, dz))
                    .with_scale(Vec3::new(0.9, 1.05, 2.3)),
            ));
        }

        let tent_mats = [
            materials.add(mat(Color::srgb_u8(0xB8, 0x44, 0x38))),
            materials.add(mat(Color::srgb_u8(0xDD, 0xD8, 0xCB))),
            materials.add(mat(Color::srgb_u8(0x2E, 0x4A, 0x62))),
        ];
        for i in 0..3 {
            let tz = (i as f32 - 1.0) * 7.5 + 4.0;
            let tx = br - 5.0;
            p.spawn((
                Mesh3d(shared.unit_cuboid.clone()),
                MeshMaterial3d(tent_mats[i].clone()),
                Transform::from_translation(Vec3::new(tx, 1.15, tz))
                    .with_rotation(Quat::from_rotation_y(0.18 * i as f32))
                    .with_scale(Vec3::new(3.2, 2.1, 3.2)),
            ));
            p.spawn((
                Mesh3d(shared.unit_cuboid.clone()),
                MeshMaterial3d(tent_mats[(i + 1) % 3].clone()),
                Transform::from_translation(Vec3::new(tx, 2.55, tz))
                    .with_scale(Vec3::new(1.4, 0.9, 1.4)),
            ));
        }
    });

    // Stumps
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
                    MeshMaterial3d(shared.stump_mat.clone()),
                    Transform::from_xyz(0.0, geo::STUMP_HEIGHT / 2.0, i as f32 * STUMP_GAP),
                ));
            });
        }
        commands.entity(end_root).with_children(|p| {
            p.spawn((
                Mesh3d(meshes.add(Cuboid::new(0.03, 0.02, STUMP_GAP * 2.0))),
                MeshMaterial3d(shared.stump_mat.clone()),
                Transform::from_xyz(0.0, geo::STUMP_HEIGHT + 0.01, 0.0),
            ));
        });
        commands.entity(root).add_child(end_root);
    }

    commands.insert_resource(FloodlightMaterials {
        day: shared.lamp_day_mat.clone(),
        night: shared.lamp_night_mat.clone(),
    });

    root
}

/// Expected crowd count for a standard bowl (used by tests).
pub fn expected_crowd_count() -> usize {
    let mut count = 0usize;
    for seg in 0..CROWD_SEGMENTS {
        if seg % CROWD_AISLE_EVERY == 0 {
            continue;
        }
        for tier in 0..TIER_COUNT {
            count += 1 + ((seg * 7 + tier * 11) % 3 == 0) as usize;
        }
    }
    count
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

fn create_pitch_image() -> Image {
    use bevy::asset::RenderAssetUsages;
    use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
    let size = 128u32;
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let n = ((x as f32 * 0.09).sin() * (y as f32 * 0.08).cos() * 0.5 + 0.5).clamp(0.0, 1.0);
            let r = 0.71 + n * 0.08;
            let g = 0.58 + n * 0.06;
            let b = 0.38 + n * 0.04;
            data.extend_from_slice(&[
                (r * 255.0) as u8,
                (g * 255.0) as u8,
                (b * 255.0) as u8,
                255,
            ]);
        }
    }
    let mut img = Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.texture_descriptor.usage =
        TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..Default::default()
    });
    img
}

fn sponsor_board_image(style: u32) -> Image {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    const W: u32 = 512;
    const H: u32 = 96;
    let mut data = Vec::with_capacity((W * H * 4) as usize);
    match style {
        0 => {
            for y in 0..H {
                for x in 0..W {
                    let diag = ((x + y * 2) % 128) < 34;
                    let (r, g, b) = if diag {
                        (0.85, 0.68, 0.22)
                    } else {
                        (0.06, 0.10, 0.24)
                    };
                    data.extend_from_slice(&[
                        (r * 255.0) as u8,
                        (g * 255.0) as u8,
                        (b * 255.0) as u8,
                        255,
                    ]);
                }
            }
        }
        _ => {
            for y in 0..H {
                for x in 0..W {
                    let in_bar = y > 30 && y < 66 && (x % 96) > 12 && (x % 96) < 78;
                    let under = y >= 74 && y <= 80 && (x / 6) % 2 == 0;
                    let (r, g, b) = if in_bar {
                        (0.78, 0.16, 0.16)
                    } else if under {
                        (0.14, 0.42, 0.20)
                    } else {
                        (0.93, 0.91, 0.86)
                    };
                    data.extend_from_slice(&[
                        (r * 255.0) as u8,
                        (g * 255.0) as u8,
                        (b * 255.0) as u8,
                        255,
                    ]);
                }
            }
        }
    }
    Image::new(
        Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

fn big_screen_image(home: &str, away: &str) -> Image {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    const W: u32 = 256;
    const H: u32 = 128;

    fn draw_glyph(data: &mut Vec<u8>, ch: u8, ox: u32, oy: u32) {
        const FONT: [[u8; 7]; 26] = [
            [0b11100, 0b10100, 0b11100, 0b10100, 0b00000, 0b00000, 0b00000],
            [0b01000, 0b10100, 0b11100, 0b10100, 0b00000, 0b00000, 0b00000],
            [0b11100, 0b10000, 0b10000, 0b11100, 0b00000, 0b00000, 0b00000],
            [0b11100, 0b10000, 0b01100, 0b10000, 0b00000, 0b00000, 0b00000],
            [0b11100, 0b10100, 0b01100, 0b00100, 0b00000, 0b00000, 0b00000],
            [0b11100, 0b10000, 0b11000, 0b10000, 0b00000, 0b00000, 0b00000],
            [0b11100, 0b10000, 0b10100, 0b11100, 0b00000, 0b00000, 0b00000],
            [0b10100, 0b10100, 0b11100, 0b10100, 0b00000, 0b00000, 0b00000],
            [0b11100, 0b01000, 0b01000, 0b11100, 0b00000, 0b00000, 0b00000],
            [0b00100, 0b00100, 0b00100, 0b11100, 0b00000, 0b00000, 0b00000],
            [0b10100, 0b11000, 0b11000, 0b10100, 0b00000, 0b00000, 0b00000],
            [0b10000, 0b10000, 0b10000, 0b11100, 0b00000, 0b00000, 0b00000],
            [0b10100, 0b11100, 0b10100, 0b10100, 0b00000, 0b00000, 0b00000],
            [0b10100, 0b11100, 0b11100, 0b10100, 0b00000, 0b00000, 0b00000],
            [0b11100, 0b10100, 0b10100, 0b11100, 0b00000, 0b00000, 0b00000],
            [0b11100, 0b10100, 0b11100, 0b10000, 0b00000, 0b00000, 0b00000],
            [0b11100, 0b10100, 0b11100, 0b00100, 0b00000, 0b00000, 0b00000],
            [0b11100, 0b11000, 0b11000, 0b10100, 0b00000, 0b00000, 0b00000],
            [0b11100, 0b10000, 0b11100, 0b00100, 0b00000, 0b00000, 0b00000],
            [0b11100, 0b01000, 0b01000, 0b01000, 0b00000, 0b00000, 0b00000],
            [0b10100, 0b10100, 0b10100, 0b11100, 0b00000, 0b00000, 0b00000],
            [0b10100, 0b10100, 0b10100, 0b01000, 0b00000, 0b00000, 0b00000],
            [0b10100, 0b10100, 0b11100, 0b10100, 0b00000, 0b00000, 0b00000],
            [0b10100, 0b01000, 0b01000, 0b10100, 0b00000, 0b00000, 0b00000],
            [0b10100, 0b01000, 0b01000, 0b01000, 0b00000, 0b00000, 0b00000],
            [0b11100, 0b00100, 0b01000, 0b11100, 0b00000, 0b00000, 0b00000],
        ];
        let i = ch.checked_sub(b'A').unwrap_or(0) as usize % 26;
        for row in 0..7 {
            for col in 0..5 {
                if FONT[i][row] & (1 << (4 - col)) != 0 {
                    let px = ox + col * 2;
                    let py = oy + row as u32 * 2;
                    for dy in 0..2 {
                        for dx in 0..2 {
                            let x = px + dx;
                            let y = py + dy;
                            if x < W && y < H {
                                let idx = ((y * W + x) * 4) as usize;
                                data[idx] = 0x40;
                                data[idx + 1] = 0xE0;
                                data[idx + 2] = 0x60;
                            }
                        }
                    }
                }
            }
        }
    }

    let mut data = vec![0u8; (W * H * 4) as usize];
    for y in 0..H {
        for x in 0..W {
            let idx = ((y * W + x) * 4) as usize;
            let header = y < 18;
            let (r, g, b) = if header {
                (0.98, 0.72, 0.25)
            } else {
                (0.02, 0.04, 0.07)
            };
            data[idx] = (r * 255.0) as u8;
            data[idx + 1] = (g * 255.0) as u8;
            data[idx + 2] = (b * 255.0) as u8;
            data[idx + 3] = 255;
        }
    }
    let chars: Vec<u8> = format!("{home} V {away}").bytes().collect();
    let mut ox = 24;
    for ch in &chars {
        if *ch >= b'A' && *ch <= b'Z' {
            draw_glyph(&mut data, *ch, ox, 44);
        }
        ox += 16;
    }
    Image::new(
        Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crowd_count_in_target_range() {
        let n = expected_crowd_count();
        assert!(n >= 350, "crowd too sparse: {n}");
        assert!(n <= 550, "crowd too dense: {n}");
    }

    #[test]
    fn bowl_outer_exceeds_boundary() {
        let bowl = BowlLayout::from_boundary(65.0);
        assert!(bowl.outer_radius() > 65.0 + 5.0);
    }
}

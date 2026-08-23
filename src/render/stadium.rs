use std::f32::consts::{PI, TAU};

use crate::core::geometry as geo;
use crate::core::stadiums::Stadium;
use crate::core::teams::Team;
use crate::render::crowd::{
    self, CROWD_VARIANTS, outfit_for, posture_y_offset, spectator_seed, variant_index_for_seat,
    yaw_jitter_for_seat,
};
use crate::render::outfield_grass::{self, MOW_BAND_COUNT, append_rgba8_srgb_mip_chain};
use crate::render::ring_geometry::{
    floodlight_angles, floodlight_radius, ring_band_specs, ring_boxes_mesh,
    ring_face_center_rotation, ring_position, ring_segment_transform, ring_tangent,
    ring_tube_mesh, stadium_ground_disc_mesh, stadium_ground_radius,
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
const LOWER_TIER_COUNT: usize = 7;
const UPPER_TIER_COUNT: usize = 5;
const TIER_COUNT: usize = LOWER_TIER_COUNT + UPPER_TIER_COUNT;
const TIER_MAT_COUNT: usize = 8;
const TIER_SEGMENTS: usize = 96;
const FACADE_SEGMENTS: usize = 48;
const AISLE_EVERY: usize = 8;
const CROWD_SEGMENTS: usize = 90;
const CROWD_AISLE_EVERY: usize = 10;
/// Spectator tiers spread across lower and upper decks (keeps crowd count stable).
const CROWD_TIERS: [usize; 5] = [1, 3, 5, 8, 10];

pub(crate) struct BowlLayout {
    inner_radius: f32,
    tier_depth: f32,
    tier_rise: f32,
    tread_thickness: f32,
    base_height: f32,
    upper_deck_setback: f32,
    upper_deck_rise_gap: f32,
}

impl BowlLayout {
    pub(crate) fn from_boundary(boundary: f32) -> Self {
        Self {
            inner_radius: boundary + 4.8,
            tier_depth: 2.25,
            tier_rise: 1.12,
            tread_thickness: 0.55,
            base_height: 0.5,
            upper_deck_setback: 4.2,
            upper_deck_rise_gap: 2.8,
        }
    }

    fn lower_outer_radius(&self) -> f32 {
        self.inner_radius + self.tier_depth * LOWER_TIER_COUNT as f32
    }

    pub(crate) fn outer_radius(&self) -> f32 {
        self.lower_outer_radius()
            + self.upper_deck_setback
            + self.tier_depth * UPPER_TIER_COUNT as f32
    }

    fn upper_inner_radius(&self) -> f32 {
        self.lower_outer_radius() + self.upper_deck_setback
    }

    fn tier_mid_radius(&self, tier: usize) -> f32 {
        if tier < LOWER_TIER_COUNT {
            self.inner_radius + (tier as f32 + 0.5) * self.tier_depth
        } else {
            let upper = tier - LOWER_TIER_COUNT;
            self.upper_inner_radius() + (upper as f32 + 0.5) * self.tier_depth
        }
    }

    fn tier_height(&self, tier: usize) -> f32 {
        if tier < LOWER_TIER_COUNT {
            self.base_height + tier as f32 * self.tier_rise
        } else {
            let upper = tier - LOWER_TIER_COUNT;
            self.base_height
                + LOWER_TIER_COUNT as f32 * self.tier_rise
                + self.upper_deck_rise_gap
                + upper as f32 * self.tier_rise
        }
    }

    fn stand_top_height(&self) -> f32 {
        self.tier_height(TIER_COUNT - 1) + self.tread_thickness
    }

    fn is_upper_deck(&self, tier: usize) -> bool {
        tier >= LOWER_TIER_COUNT
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
    tier_mats: [Handle<StandardMaterial>; TIER_MAT_COUNT],
    riser_mat: Handle<StandardMaterial>,
    rail_mat: Handle<StandardMaterial>,
    column_mat: Handle<StandardMaterial>,
    canopy_mat: Handle<StandardMaterial>,
    facade_mat: Handle<StandardMaterial>,
    concourse_mat: Handle<StandardMaterial>,
    apron_mat: Handle<StandardMaterial>,
    pavilion_mat: Handle<StandardMaterial>,
    media_box_mat: Handle<StandardMaterial>,
    roof_truss_mat: Handle<StandardMaterial>,
    tower_mat: Handle<StandardMaterial>,
    lamp_day_mat: Handle<StandardMaterial>,
    lamp_night_mat: Handle<StandardMaterial>,
    sponsor_mats: Vec<Handle<StandardMaterial>>,
    grass_tex: Handle<Image>,
    grass_mesh: Handle<Mesh>,
    stump_cylinder_mesh: Handle<Mesh>,
    stump_bail_mesh: Handle<Mesh>,
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

    let tier_mats: [Handle<StandardMaterial>; TIER_MAT_COUNT] = std::array::from_fn(|i| {
        let shade = 1.0 - i as f32 * 0.07;
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
        canopy_mat: materials.add(mat(tint(0.38, 0.05))),
        facade_mat: materials.add(mat(Color::srgb_u8(0x6A, 0x6E, 0x74))),
        concourse_mat: materials.add(mat(Color::srgb_u8(0x8A, 0x8E, 0x92))),
        apron_mat: materials.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.94,
            reflectance: 0.28,
            ..default()
        }),
        pavilion_mat: materials.add(mat(Color::srgb_u8(0x5C, 0x60, 0x68))),
        media_box_mat: materials.add(mat(Color::srgb_u8(0x2A, 0x32, 0x3C))),
        roof_truss_mat: materials.add(mat(Color::srgb_u8(0x3C, 0x40, 0x48))),
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
            materials.add(texture_mat(crate::render::load_sponsor_ribbon(
                asset_server,
            ))),
            materials.add(texture_mat(images.add(sponsor_board_image(0)))),
            materials.add(texture_mat(images.add(sponsor_board_image(1)))),
        ],
        grass_tex: images.add(crate::render::create_outfield_grass_image()),
        grass_mesh: meshes.add(Plane3d::default().mesh().size(1.0, 1.0).subdivisions(4)),
        stump_cylinder_mesh: meshes.add(Cylinder::new(0.02, geo::STUMP_HEIGHT)),
        stump_bail_mesh: meshes.add(Cuboid::new(0.03, 0.02, STUMP_GAP * 2.0)),
    }
}

struct StadiumBuildCtx<'a> {
    meshes: &'a mut Assets<Mesh>,
    materials: &'a mut Assets<StandardMaterial>,
    images: &'a mut Assets<Image>,
    asset_server: &'a AssetServer,
    stadium: &'a Stadium,
    shared: &'a SharedStadiumAssets,
    bowl: BowlLayout,
    outfield_base: Color,
    batting_crest_mat: Handle<StandardMaterial>,
    fielding_crest_mat: Handle<StandardMaterial>,
    apron_disc_mesh: Handle<Mesh>,
    rope_ring_mesh: Handle<Mesh>,
    pitch_mesh: Handle<Mesh>,
    pitch_worn_mesh: Handle<Mesh>,
    crease_line_mesh: Handle<Mesh>,
    crease_cross_mesh: Handle<Mesh>,
    pitch_mat: Handle<StandardMaterial>,
    pitch_worn_mat: Handle<StandardMaterial>,
    mow_band_mats: Vec<Handle<StandardMaterial>>,
}

fn track_spawn(spawn_count: &mut usize) {
    *spawn_count += 1;
}

fn crowd_segment_skipped(seg: usize) -> bool {
    seg.is_multiple_of(CROWD_AISLE_EVERY)
}

fn crowd_seats_at(seg: usize, tier: usize) -> usize {
    1 + (seg * 7 + tier * 11).is_multiple_of(3) as usize
}

/// Mid-angle on the crowd seat ring for a segment/tier (shared by spawner and gap test).
fn crowd_seat_mid_angle(seg: usize, tier: usize) -> f32 {
    let tier_phase = ((tier * 19 + 7) % CROWD_SEGMENTS) as f32 / CROWD_SEGMENTS as f32 * TAU;
    let seg_jitter = ((seg * 3 + tier * 13) % 5) as f32 - 2.0;
    (seg as f32 + 0.5 + seg_jitter * 0.18) / CROWD_SEGMENTS as f32 * TAU + tier_phase
}

/// Structural tread segment index for a crowd seat's mid-angle.
fn crowd_seat_structural_segment(seg: usize, tier: usize) -> usize {
    let mid = crowd_seat_mid_angle(seg, tier);
    (mid.rem_euclid(TAU) / TAU * TIER_SEGMENTS as f32) as usize
}

/// True when a crowd seat's angle falls in one of the structural aisle gaps
/// cut by `ring_band_specs(TIER_SEGMENTS, AISLE_EVERY, ..)` — there is no
/// tread there, so a spectator placed on it would float.
fn crowd_seat_over_aisle_gap(seg: usize, tier: usize) -> bool {
    let mid = crowd_seat_mid_angle(seg, tier);
    let seg_f = mid.rem_euclid(TAU) / TAU * TIER_SEGMENTS as f32;
    let structural_seg = seg_f as usize;
    if structural_seg.is_multiple_of(AISLE_EVERY) {
        return true;
    }
    // Rows are 1–2 seats wide (±~0.5 m tangentially); widen gap edges by ±0.25
    // structural segments so edge seats beside an aisle are excluded too.
    let pos_in_aisle_cycle = seg_f % AISLE_EVERY as f32;
    pos_in_aisle_cycle < 0.25 || pos_in_aisle_cycle > AISLE_EVERY as f32 - 0.25
}

fn spawn_stadium_apron(
    p: &mut ChildSpawnerCommands,
    ctx: &StadiumBuildCtx<'_>,
    spawn_count: &mut usize,
) {
    // Circular apron with radial fade — reads as continuous ground, not a finite slab.
    p.spawn((
        Mesh3d(ctx.apron_disc_mesh.clone()),
        MeshMaterial3d(ctx.shared.apron_mat.clone()),
        Transform::from_translation(Vec3::new(0.0, -0.002, 0.0)),
    ));
    track_spawn(spawn_count);
}

fn spawn_outfield_bands(
    p: &mut ChildSpawnerCommands,
    ctx: &StadiumBuildCtx<'_>,
    spawn_count: &mut usize,
) {
    // Outfield grass bands (shared mesh + texture).
    let r = ctx.stadium.boundary_radius() + 6.0;
    let span = r * 2.05;
    let band_width = span / MOW_BAND_COUNT as f32;
    let half_span = span / 2.0;
    for band in 0..MOW_BAND_COUNT {
        let x_min = -half_span + band as f32 * band_width;
        let x_center = x_min + band_width / 2.0;
        p.spawn((
            Mesh3d(ctx.shared.grass_mesh.clone()),
            MeshMaterial3d(ctx.mow_band_mats[band as usize].clone()),
            Transform::from_translation(Vec3::new(x_center, 0.01, 0.0))
                .with_scale(Vec3::new(band_width, 1.0, span)),
        ));
        track_spawn(spawn_count);
    }
}

fn spawn_pitch_and_creases(
    p: &mut ChildSpawnerCommands,
    ctx: &StadiumBuildCtx<'_>,
    spawn_count: &mut usize,
) {
    // Pitch
    p.spawn((
        Mesh3d(ctx.pitch_mesh.clone()),
        MeshMaterial3d(ctx.pitch_mat.clone()),
        Transform::from_translation(Vec3::Y * 0.05),
    ));
    track_spawn(spawn_count);
    p.spawn((
        Mesh3d(ctx.pitch_worn_mesh.clone()),
        MeshMaterial3d(ctx.pitch_worn_mat.clone()),
        Transform::from_translation(Vec3::Y * 0.06),
    ));
    track_spawn(spawn_count);

    // Creases
    for sign in [-1.0_f32, 1.0] {
        let x = sign * (geo::PITCH_HALF_LEN - geo::CREASE_DEPTH);
        p.spawn((
            Mesh3d(ctx.crease_line_mesh.clone()),
            MeshMaterial3d(ctx.shared.white_mat.clone()),
            Transform::from_translation(Vec3::new(x, 0.07, 0.0)),
        ));
        track_spawn(spawn_count);
        for z in [-geo::PITCH_WIDTH / 2.0, geo::PITCH_WIDTH / 2.0] {
            p.spawn((
                Mesh3d(ctx.crease_cross_mesh.clone()),
                MeshMaterial3d(ctx.shared.white_mat.clone()),
                Transform::from_translation(Vec3::new(x - sign * 1.1, 0.07, z)),
            ));
            track_spawn(spawn_count);
        }
    }
}

fn spawn_boundary_ring_and_boards(
    p: &mut ChildSpawnerCommands,
    ctx: &StadiumBuildCtx<'_>,
    spawn_count: &mut usize,
) {
    // Boundary rope (single merged ring) + sponsor boards.
    p.spawn((
        Mesh3d(ctx.rope_ring_mesh.clone()),
        MeshMaterial3d(ctx.shared.rope_mat.clone()),
        Transform::IDENTITY,
    ));
    track_spawn(spawn_count);

    let br = ctx.stadium.boundary_radius();
    for seg in 0..TIER_SEGMENTS {
        let a0 = seg as f32 / TIER_SEGMENTS as f32 * TAU;
        let a1 = (seg + 1) as f32 / TIER_SEGMENTS as f32 * TAU;
        let mid = (a0 + a1) / 2.0;
        let len = 2.0 * br * (PI / TIER_SEGMENTS as f32);
        if seg % 2 == 0 {
            let wall_r = br + 1.2;
            let board_width = len * 1.85;
            p.spawn((
                Mesh3d(ctx.shared.unit_cuboid.clone()),
                MeshMaterial3d(ctx.shared.board_frame_mat.clone()),
                ring_segment_transform(mid, wall_r + 0.02, 0.78).with_scale(Vec3::new(
                    board_width + 0.14,
                    1.52,
                    0.16,
                )),
            ));
            track_spawn(spawn_count);
            p.spawn((
                Mesh3d(ctx.shared.unit_cuboid.clone()),
                MeshMaterial3d(
                    ctx.shared.sponsor_mats[seg % ctx.shared.sponsor_mats.len()].clone(),
                ),
                ring_segment_transform(mid, wall_r, 0.78).with_scale(Vec3::new(
                    board_width,
                    1.35,
                    0.18,
                )),
            ));
            track_spawn(spawn_count);
        }
        if seg % 12 == 6 {
            let crest_r = br + 1.48;
            let crest_mat = if (seg / 12) % 2 == 0 {
                ctx.batting_crest_mat.clone()
            } else {
                ctx.fielding_crest_mat.clone()
            };
            p.spawn((
                Mesh3d(ctx.shared.unit_cuboid.clone()),
                MeshMaterial3d(ctx.shared.board_frame_mat.clone()),
                ring_segment_transform(mid, crest_r + 0.03, 1.34)
                    .with_scale(Vec3::new(2.42, 2.42, 0.20)),
            ));
            track_spawn(spawn_count);
            p.spawn((
                Mesh3d(ctx.shared.unit_cuboid.clone()),
                MeshMaterial3d(crest_mat),
                ring_segment_transform(mid, crest_r, 1.34).with_scale(Vec3::new(2.24, 2.24, 0.23)),
            ));
            track_spawn(spawn_count);
        }
    }
}

fn spawn_sight_screens(
    p: &mut ChildSpawnerCommands,
    ctx: &StadiumBuildCtx<'_>,
    spawn_count: &mut usize,
) {
    // Sight screens
    let br = ctx.stadium.boundary_radius();
    for sign in [-1.0_f32, 1.0] {
        let x = sign * (br - 2.5);
        p.spawn((
            Mesh3d(ctx.shared.unit_cuboid.clone()),
            MeshMaterial3d(ctx.shared.sight_screen_mat.clone()),
            Transform::from_translation(Vec3::new(x, 1.65, 0.0))
                .with_scale(Vec3::new(0.12, 3.2, 7.5)),
        ));
        track_spawn(spawn_count);
    }
}

fn spawn_tiers_and_roof(
    p: &mut ChildSpawnerCommands,
    ctx: &mut StadiumBuildCtx<'_>,
    spawn_count: &mut usize,
) {
    // ---- Multi-deck raked seating bowl (lower bowl + set-back upper deck) ----
    let bowl = &ctx.bowl;
    let arc_w = 2.0 * PI * bowl.inner_radius / TIER_SEGMENTS as f32 * 1.02;
    let tread_arc = arc_w;
    let tread_radial = bowl.tier_depth * 0.92;

    for tier in 0..TIER_COUNT {
        let mid_r = bowl.tier_mid_radius(tier);
        let h = bowl.tier_height(tier);
        let mat = ctx.shared.tier_mats[tier % TIER_MAT_COUNT].clone();

        let tread_specs = ring_band_specs(
            TIER_SEGMENTS,
            AISLE_EVERY,
            mid_r,
            h + bowl.tread_thickness * 0.5,
            tread_arc,
            bowl.tread_thickness,
            tread_radial,
        );
        p.spawn((
            Mesh3d(ctx.meshes.add(ring_boxes_mesh(&tread_specs))),
            MeshMaterial3d(mat),
            Transform::IDENTITY,
        ));
        track_spawn(spawn_count);

        // Riser face at inner edge of each tier (except ground).
        if tier > 0 {
            let inner_r = if bowl.is_upper_deck(tier) && tier == LOWER_TIER_COUNT {
                bowl.upper_inner_radius() - 0.08
            } else if bowl.is_upper_deck(tier) {
                bowl.upper_inner_radius()
                    + (tier - LOWER_TIER_COUNT) as f32 * bowl.tier_depth
                    - 0.08
            } else {
                bowl.inner_radius + tier as f32 * bowl.tier_depth - 0.08
            };
            let riser_h = bowl.tier_rise;
            let riser_specs = ring_band_specs(
                TIER_SEGMENTS,
                AISLE_EVERY,
                inner_r,
                h - riser_h * 0.5,
                tread_arc * 0.98,
                riser_h,
                0.16,
            );
            p.spawn((
                Mesh3d(ctx.meshes.add(ring_boxes_mesh(&riser_specs))),
                MeshMaterial3d(ctx.shared.riser_mat.clone()),
                Transform::IDENTITY,
            ));
            track_spawn(spawn_count);
        }

        // Guard rails on mid and upper tiers.
        if tier >= 2 && (tier % 2 == 0 || tier >= LOWER_TIER_COUNT) {
            let rail_r = mid_r - tread_radial * 0.38;
            let rail_specs = ring_band_specs(
                TIER_SEGMENTS,
                AISLE_EVERY,
                rail_r,
                h + bowl.tread_thickness + 0.14,
                tread_arc * 0.95,
                0.18,
                0.12,
            );
            p.spawn((
                Mesh3d(ctx.meshes.add(ring_boxes_mesh(&rail_specs))),
                MeshMaterial3d(ctx.shared.rail_mat.clone()),
                Transform::IDENTITY,
            ));
            track_spawn(spawn_count);
        }
    }

    // Concourse ring between lower bowl and upper deck (vomitory level).
    let concourse_r = bowl.lower_outer_radius() + bowl.upper_deck_setback * 0.42;
    let concourse_h =
        bowl.base_height + LOWER_TIER_COUNT as f32 * bowl.tier_rise + bowl.upper_deck_rise_gap * 0.35;
    let concourse_arc = 2.0 * PI * concourse_r / TIER_SEGMENTS as f32 * 1.04;
    let concourse_specs = ring_band_specs(
        TIER_SEGMENTS,
        AISLE_EVERY,
        concourse_r,
        concourse_h,
        concourse_arc,
        0.28,
        bowl.upper_deck_setback * 0.88,
    );
    p.spawn((
        Mesh3d(ctx.meshes.add(ring_boxes_mesh(&concourse_specs))),
        MeshMaterial3d(ctx.shared.concourse_mat.clone()),
        Transform::IDENTITY,
    ));
    track_spawn(spawn_count);

    // Concrete facade bulk behind the lower bowl (visible architectural mass).
    let facade_r = bowl.lower_outer_radius() + 2.6;
    let facade_h = bowl.base_height + LOWER_TIER_COUNT as f32 * bowl.tier_rise * 0.55;
    let facade_arc = 2.0 * PI * facade_r / FACADE_SEGMENTS as f32 * 1.05;
    let lower_facade_specs = ring_band_specs(
        FACADE_SEGMENTS,
        AISLE_EVERY / 2,
        facade_r,
        facade_h * 0.5,
        facade_arc,
        facade_h,
        3.8,
    );
    p.spawn((
        Mesh3d(ctx.meshes.add(ring_boxes_mesh(&lower_facade_specs))),
        MeshMaterial3d(ctx.shared.facade_mat.clone()),
        Transform::IDENTITY,
    ));
    track_spawn(spawn_count);

    // Upper-deck rear facade (taller wall set back from the pitch).
    let upper_facade_r = bowl.outer_radius() + 1.8;
    let upper_facade_base =
        bowl.base_height + LOWER_TIER_COUNT as f32 * bowl.tier_rise + bowl.upper_deck_rise_gap;
    let upper_facade_h = UPPER_TIER_COUNT as f32 * bowl.tier_rise + 4.5;
    let upper_facade_arc = 2.0 * PI * upper_facade_r / FACADE_SEGMENTS as f32 * 1.04;
    let upper_facade_specs = ring_band_specs(
        FACADE_SEGMENTS,
        AISLE_EVERY / 2,
        upper_facade_r,
        upper_facade_base + upper_facade_h * 0.5,
        upper_facade_arc,
        upper_facade_h,
        4.2,
    );
    p.spawn((
        Mesh3d(ctx.meshes.add(ring_boxes_mesh(&upper_facade_specs))),
        MeshMaterial3d(ctx.shared.facade_mat.clone()),
        Transform::IDENTITY,
    ));
    track_spawn(spawn_count);

    // Support columns at aisle junctions (lower + upper deck).
    for seg in (0..TIER_SEGMENTS).step_by(AISLE_EVERY) {
        let a = seg as f32 / TIER_SEGMENTS as f32 * TAU;
        let col_r = bowl.inner_radius + bowl.tier_depth * 3.5;
        let lower_col_h = bowl.base_height + LOWER_TIER_COUNT as f32 * bowl.tier_rise + 1.2;
        p.spawn((
            Mesh3d(ctx.shared.column_mesh.clone()),
            MeshMaterial3d(ctx.shared.column_mat.clone()),
            Transform::from_translation(ring_position(a, col_r, lower_col_h * 0.5))
                .with_scale(Vec3::new(1.0, lower_col_h, 1.0)),
        ));
        track_spawn(spawn_count);
        let upper_col_r = bowl.upper_inner_radius() + bowl.tier_depth * 2.0;
        let upper_col_base =
            bowl.base_height + LOWER_TIER_COUNT as f32 * bowl.tier_rise + bowl.upper_deck_rise_gap;
        let upper_col_h = UPPER_TIER_COUNT as f32 * bowl.tier_rise + 2.4;
        p.spawn((
            Mesh3d(ctx.shared.column_mesh.clone()),
            MeshMaterial3d(ctx.shared.column_mat.clone()),
            Transform::from_translation(ring_position(
                a,
                upper_col_r,
                upper_col_base + upper_col_h * 0.5,
            ))
            .with_scale(Vec3::new(1.15, upper_col_h, 1.15)),
        ));
        track_spawn(spawn_count);
    }

    // Roof canopy over upper deck with supporting trusses.
    let canopy_r = bowl.outer_radius() + 2.4;
    let canopy_h = bowl.stand_top_height() + 2.2;
    let canopy_arc = 2.0 * PI * canopy_r / TIER_SEGMENTS as f32 * 1.06;
    let canopy_specs = ring_band_specs(
        TIER_SEGMENTS,
        AISLE_EVERY,
        canopy_r,
        canopy_h,
        canopy_arc,
        0.32,
        4.8,
    );
    p.spawn((
        Mesh3d(ctx.meshes.add(ring_boxes_mesh(&canopy_specs))),
        MeshMaterial3d(ctx.shared.canopy_mat.clone()),
        Transform::IDENTITY,
    ));
    track_spawn(spawn_count);
    // Truss ribs under the canopy at aisle spokes.
    for seg in (0..TIER_SEGMENTS).step_by(AISLE_EVERY) {
        let a = seg as f32 / TIER_SEGMENTS as f32 * TAU;
        let truss_r = bowl.outer_radius() + 1.0;
        let truss_len = canopy_r - truss_r;
        let truss_mid_r = (truss_r + canopy_r) * 0.5;
        let truss_base = bowl.stand_top_height() + 0.4;
        let truss_h = canopy_h - truss_base;
        p.spawn((
            Mesh3d(ctx.shared.unit_cuboid.clone()),
            MeshMaterial3d(ctx.shared.roof_truss_mat.clone()),
            ring_segment_transform(a, truss_mid_r, truss_base + truss_h * 0.5)
                .with_scale(Vec3::new(0.42, truss_h, truss_len)),
        ));
        track_spawn(spawn_count);
    }

    spawn_pavilions_and_media_box(p, ctx, spawn_count);
}

fn spawn_pavilions_and_media_box(
    p: &mut ChildSpawnerCommands,
    ctx: &StadiumBuildCtx<'_>,
    spawn_count: &mut usize,
) {
    let bowl = &ctx.bowl;
    let top = bowl.stand_top_height();

    // Pavilion blocks rising above the stand line at four quadrants + two ends.
    let pavilion_angles: [f32; 6] = [
        0.0,
        PI * 0.5,
        PI,
        PI * 1.5,
        PI * 0.25,
        PI * 1.25,
    ];
    for (i, &angle) in pavilion_angles.iter().enumerate() {
        let r = bowl.outer_radius() + 3.5 + (i % 2) as f32 * 1.2;
        let h = top + 4.5 + (i % 3) as f32 * 2.8;
        let width = 8.5 + (i % 2) as f32 * 3.0;
        let depth = 6.0 + (i % 3) as f32 * 1.5;
        p.spawn((
            Mesh3d(ctx.shared.unit_cuboid.clone()),
            MeshMaterial3d(ctx.shared.pavilion_mat.clone()),
            ring_segment_transform(angle, r, h * 0.5).with_scale(Vec3::new(width, h, depth)),
        ));
        track_spawn(spawn_count);
        // Small roof cap on each pavilion.
        p.spawn((
            Mesh3d(ctx.shared.unit_cuboid.clone()),
            MeshMaterial3d(ctx.shared.canopy_mat.clone()),
            ring_segment_transform(angle, r, h + 0.6).with_scale(Vec3::new(width * 1.08, 0.35, depth * 1.1)),
        ));
        track_spawn(spawn_count);
    }

    // Media / commentary box on the main-stand side (behind bowler's end, -X).
    let media_angle = PI;
    let media_r = bowl.outer_radius() + 5.2;
    let media_h = top + 6.8;
    p.spawn((
        Mesh3d(ctx.shared.unit_cuboid.clone()),
        MeshMaterial3d(ctx.shared.media_box_mat.clone()),
        ring_segment_transform(media_angle, media_r, media_h * 0.5)
            .with_scale(Vec3::new(22.0, media_h, 5.5)),
    ));
    track_spawn(spawn_count);
    // Glazed front strip.
    p.spawn((
        Mesh3d(ctx.shared.unit_cuboid.clone()),
        MeshMaterial3d(ctx.shared.board_frame_mat.clone()),
        ring_segment_transform(media_angle, media_r - 2.2, media_h * 0.55)
            .with_scale(Vec3::new(20.0, media_h * 0.45, 0.28)),
    ));
    track_spawn(spawn_count);
}

fn spawn_floodlights(
    p: &mut ChildSpawnerCommands,
    ctx: &StadiumBuildCtx<'_>,
    spawn_count: &mut usize,
) {
    // ---- Floodlight towers integrated with stand perimeter ----
    let outer = ctx.bowl.outer_radius();
    let stand_top = ctx.bowl.stand_top_height();
    let tower_r = floodlight_radius(outer);
    let tower_h = stand_top + 22.0;
    for (tower_idx, angle) in floodlight_angles().into_iter().enumerate() {
        let base = ring_position(angle, tower_r, 0.0);
        let top = ring_position(angle, tower_r, tower_h);

        // Pylon rises from stand roofline with a short tie-back truss to the bowl.
        let tie_r = outer + 1.5;
        let tie_top = ring_position(angle, tie_r, stand_top + 1.8);
        p.spawn((
            Mesh3d(ctx.shared.tower_truss_mesh.clone()),
            MeshMaterial3d(ctx.shared.roof_truss_mat.clone()),
            Transform::from_translation(
                (top + tie_top) * 0.5 + Vec3::Y * (tower_h - stand_top) * 0.15,
            )
            .with_scale(Vec3::new(2.8, 1.0, 1.0))
            .with_rotation(ring_segment_transform(angle, tower_r, tower_h).rotation),
        ));
        track_spawn(spawn_count);

        p.spawn((
            Mesh3d(ctx.shared.tower_pole_mesh.clone()),
            MeshMaterial3d(ctx.shared.tower_mat.clone()),
            Transform::from_translation(Vec3::new(base.x, tower_h * 0.5, base.z))
                .with_scale(Vec3::new(1.0, tower_h, 1.0)),
        ));
        track_spawn(spawn_count);
        p.spawn((
            Mesh3d(ctx.shared.tower_truss_mesh.clone()),
            MeshMaterial3d(ctx.shared.tower_mat.clone()),
            Transform::from_translation(Vec3::new(top.x, tower_h - 1.0, top.z))
                .with_scale(Vec3::new(4.2, 1.0, 1.0))
                .with_rotation(ring_segment_transform(angle, tower_r, tower_h).rotation),
        ));
        track_spawn(spawn_count);
        for offset in [-1.6_f32, 0.0, 1.6] {
            let tangent = ring_tangent(angle);
            let lamp_pos = top + tangent * offset;
            p.spawn((
                FloodlightFixture,
                Mesh3d(ctx.shared.lamp_bank_mesh.clone()),
                MeshMaterial3d(ctx.shared.lamp_day_mat.clone()),
                Transform::from_translation(lamp_pos)
                    .with_rotation(ring_segment_transform(angle, tower_r, tower_h).rotation)
                    .with_scale(Vec3::new(1.5, 1.0, 1.0)),
            ));
            track_spawn(spawn_count);
        }
        // SpotLight aimed at pitch centre — hidden by day via NightEnvironmentLight.
        p.spawn((
            NightEnvironmentLight,
            SpotLight {
                color: Color::srgb(1.0, 0.97, 0.90),
                intensity: 10_500_000.0,
                range: 165.0,
                radius: 2.6,
                // Only the key tower casts shadows. Switching to night turns
                // all four on in one frame, and four shadow maps over the whole
                // bowl was long enough to trip the surface-timeout panic.
                shadows_enabled: tower_idx == 0,
                outer_angle: 0.85,
                inner_angle: 0.54,
                ..default()
            },
            Transform::from_translation(Vec3::new(top.x, tower_h - 1.4, top.z))
                .looking_at(Vec3::new(0.0, 0.5, 0.0), Vec3::Y),
            Visibility::Hidden,
        ));
        track_spawn(spawn_count);
    }
}

fn spawn_crowd(
    p: &mut ChildSpawnerCommands,
    ctx: &StadiumBuildCtx<'_>,
    spawn_count: &mut usize,
) -> usize {
    // ---- Crowd: ~350–550 posed Quaternius spectators ----
    let crowd_scenes: [Handle<Scene>; 15] = std::array::from_fn(|i| {
        ctx.asset_server
            .load(GltfAssetLabel::Scene(0).from_asset(CROWD_VARIANTS[i].path))
    });
    let mut crowd_count = 0usize;
    let bowl = &ctx.bowl;

    for seg in 0..CROWD_SEGMENTS {
        if crowd_segment_skipped(seg) {
            continue;
        }
        for &tier in &CROWD_TIERS {
            if crowd_seat_over_aisle_gap(seg, tier) {
                continue;
            }
            let mid = crowd_seat_mid_angle(seg, tier);
            let seats = crowd_seats_at(seg, tier);
            let seat_r = bowl.tier_mid_radius(tier) - 0.15;
            let seat_h = bowl.tier_height(tier) + bowl.tread_thickness - 0.06;
            let tangent = ring_tangent(mid);
            for k in 0..seats {
                let off = (k as f32 - (seats as f32 - 1.0) * 0.5) * 0.95
                    + ((seg * 13 + tier * 5 + k) % 7) as f32 * 0.04;
                let variant_idx = variant_index_for_seat(seg, tier, k);
                let y_off = posture_y_offset(variant_idx);
                let pos = ring_position(mid, seat_r, seat_h + y_off) + tangent * off;
                let seed = spectator_seed(seg, tier, k);
                let scale = crowd::height_scale_for_seat(seg, tier, k);
                let yaw_jitter = yaw_jitter_for_seat(seg, tier, k);
                let rot = ring_face_center_rotation(mid) * Quat::from_rotation_y(yaw_jitter);
                p.spawn((
                    SceneRoot(crowd_scenes[variant_idx].clone()),
                    outfit_for(seed),
                    Transform::from_translation(pos)
                        .with_rotation(rot)
                        .with_scale(Vec3::splat(scale)),
                    Visibility::default(),
                    InheritedVisibility::default(),
                    ViewVisibility::default(),
                ));
                track_spawn(spawn_count);
                crowd_count += 1;
            }
        }
    }
    crowd_count
}

fn spawn_big_screen_and_dugouts(
    p: &mut ChildSpawnerCommands,
    ctx: &mut StadiumBuildCtx<'_>,
    batting_team: &Team,
    fielding_team: &Team,
    spawn_count: &mut usize,
) {
    // Big screen + dugouts + tents (unchanged layout, shared materials).
    let br = ctx.stadium.boundary_radius();
    let screen_frame = ctx.materials.add(mat(Color::srgb_u8(0x10, 0x12, 0x16)));
    let screen_face = ctx
        .materials
        .add(texture_mat(ctx.images.add(big_screen_image(
            &batting_team.short.to_uppercase(),
            &fielding_team.short.to_uppercase(),
        ))));
    let sx = -(br - 2.5);
    p.spawn((
        Mesh3d(ctx.shared.unit_cuboid.clone()),
        MeshMaterial3d(screen_frame.clone()),
        Transform::from_translation(Vec3::new(sx - 1.1, 3.6, 0.0))
            .with_scale(Vec3::new(0.32, 3.8, 8.0)),
    ));
    track_spawn(spawn_count);
    p.spawn((
        Mesh3d(ctx.shared.unit_cuboid.clone()),
        MeshMaterial3d(screen_face),
        Transform::from_translation(Vec3::new(sx - 0.92, 3.6, 0.0))
            .with_scale(Vec3::new(0.18, 3.2, 7.0)),
    ));
    track_spawn(spawn_count);

    let dugout_roof = ctx.materials.add(mat(Color::srgb_u8(0xE8, 0xE6, 0xDF)));
    for sign_z in [-1.0_f32, 1.0] {
        let dz = sign_z * (br - 6.0);
        p.spawn((
            Mesh3d(ctx.shared.unit_cuboid.clone()),
            MeshMaterial3d(dugout_roof.clone()),
            Transform::from_translation(Vec3::new(sx + 8.0, 2.6, dz))
                .with_rotation(Quat::from_rotation_z(-sign_z * 0.08))
                .with_scale(Vec3::new(6.5, 0.25, 2.6)),
        ));
        track_spawn(spawn_count);
        p.spawn((
            Mesh3d(ctx.shared.unit_cuboid.clone()),
            MeshMaterial3d(screen_frame.clone()),
            Transform::from_translation(Vec3::new(sx + 11.2, 1.15, dz))
                .with_scale(Vec3::new(6.5, 2.2, 0.22)),
        ));
        track_spawn(spawn_count);
        p.spawn((
            Mesh3d(ctx.shared.unit_cuboid.clone()),
            MeshMaterial3d(dugout_roof.clone()),
            Transform::from_translation(Vec3::new(sx + 5.2, 0.55, dz))
                .with_scale(Vec3::new(0.9, 1.05, 2.3)),
        ));
        track_spawn(spawn_count);
    }

    let tent_mats = [
        ctx.materials.add(mat(Color::srgb_u8(0xB8, 0x44, 0x38))),
        ctx.materials.add(mat(Color::srgb_u8(0xDD, 0xD8, 0xCB))),
        ctx.materials.add(mat(Color::srgb_u8(0x2E, 0x4A, 0x62))),
    ];
    for i in 0..3 {
        let tz = (i as f32 - 1.0) * 7.5 + 4.0;
        let tx = br - 5.0;
        p.spawn((
            Mesh3d(ctx.shared.unit_cuboid.clone()),
            MeshMaterial3d(tent_mats[i].clone()),
            Transform::from_translation(Vec3::new(tx, 1.15, tz))
                .with_rotation(Quat::from_rotation_y(0.18 * i as f32))
                .with_scale(Vec3::new(3.2, 2.1, 3.2)),
        ));
        track_spawn(spawn_count);
        p.spawn((
            Mesh3d(ctx.shared.unit_cuboid.clone()),
            MeshMaterial3d(tent_mats[(i + 1) % 3].clone()),
            Transform::from_translation(Vec3::new(tx, 2.55, tz))
                .with_scale(Vec3::new(1.4, 0.9, 1.4)),
        ));
        track_spawn(spawn_count);
    }
}

fn spawn_stumps(
    commands: &mut Commands,
    root: Entity,
    ctx: &StadiumBuildCtx<'_>,
    spawn_count: &mut usize,
) {
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
        track_spawn(spawn_count);
        for i in -1..=1_i32 {
            commands.entity(end_root).with_children(|p| {
                p.spawn((
                    Mesh3d(ctx.shared.stump_cylinder_mesh.clone()),
                    MeshMaterial3d(ctx.shared.stump_mat.clone()),
                    Transform::from_xyz(0.0, geo::STUMP_HEIGHT / 2.0, i as f32 * STUMP_GAP),
                ));
                track_spawn(spawn_count);
            });
        }
        commands.entity(end_root).with_children(|p| {
            p.spawn((
                Mesh3d(ctx.shared.stump_bail_mesh.clone()),
                MeshMaterial3d(ctx.shared.stump_mat.clone()),
                Transform::from_xyz(0.0, geo::STUMP_HEIGHT + 0.01, 0.0),
            ));
            track_spawn(spawn_count);
        });
        commands.entity(root).add_child(end_root);
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
    let profile = std::env::var("CRICKET_STADIUM_PROFILE").is_ok();
    let t0 = profile.then(std::time::Instant::now);
    let mesh0 = meshes.len();
    let mat0 = materials.len();
    let mut spawn_count = 0usize;

    let shared = build_shared_assets(meshes, materials, images, asset_server, stadium);
    let bowl = BowlLayout::from_boundary(stadium.boundary_radius());
    let outfield_base = stadium.outfield_color;
    let boundary_r = stadium.boundary_radius();

    let ground_radius = stadium_ground_radius(bowl.outer_radius());
    let apron_disc_mesh = meshes.add(stadium_ground_disc_mesh(ground_radius, 96));
    let rope_ring_mesh = meshes.add(ring_tube_mesh(boundary_r, 0.05, TIER_SEGMENTS, 0.04));
    let pitch_mesh = meshes.add(
        Plane3d::default()
            .mesh()
            .size(geo::PITCH_LENGTH + 2.0, geo::PITCH_WIDTH),
    );
    let pitch_worn_mesh = meshes.add(
        Plane3d::default()
            .mesh()
            .size(geo::PITCH_LENGTH + 1.0, geo::PITCH_WIDTH * 0.35),
    );
    let crease_line_mesh = meshes.add(Plane3d::default().mesh().size(0.06, geo::PITCH_WIDTH));
    let crease_cross_mesh =
        meshes.add(Plane3d::default().mesh().size(geo::CREASE_DEPTH * 2.0, 0.06));

    let pitch_img = images.add(create_pitch_image());
    // Convention: `base_color` is white; the procedural texture carries the full wicket
    // albedo so we do not double-stack a tan tint on top of an already-bright map.
    let pitch_mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(pitch_img.clone()),
        perceptual_roughness: 0.96,
        reflectance: 0.22,
        ..Default::default()
    });
    let pitch_worn_mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(pitch_img),
        perceptual_roughness: 0.98,
        reflectance: 0.18,
        ..Default::default()
    });

    let r = boundary_r + 6.0;
    let span = r * 2.05;
    let band_width = span / MOW_BAND_COUNT as f32;
    let half_span = span / 2.0;
    let mow_band_mats: Vec<Handle<StandardMaterial>> = (0..MOW_BAND_COUNT)
        .map(|band| {
            let x_min = -half_span + band as f32 * band_width;
            materials.add(StandardMaterial {
                base_color: outfield_grass::tinted_mow_band_color(outfield_base, band),
                base_color_texture: Some(shared.grass_tex.clone()),
                perceptual_roughness: 0.88,
                metallic: 0.0,
                reflectance: 0.36,
                uv_transform: outfield_grass::strip_uv_transform(span, band_width, x_min),
                ..default()
            })
        })
        .collect();

    let batting_crest_mat = materials.add(texture_mat(crate::render::load_team_crest(
        asset_server,
        &batting_team.crest_asset(),
    )));
    let fielding_crest_mat = materials.add(texture_mat(crate::render::load_team_crest(
        asset_server,
        &fielding_team.crest_asset(),
    )));

    let root = commands
        .spawn((
            StadiumRoot,
            Transform::default(),
            Visibility::default(),
            InheritedVisibility::default(),
            ViewVisibility::default(),
        ))
        .id();

    let mut ctx = StadiumBuildCtx {
        meshes,
        materials,
        images,
        asset_server,
        stadium,
        shared: &shared,
        bowl,
        outfield_base,
        batting_crest_mat,
        fielding_crest_mat,
        apron_disc_mesh,
        rope_ring_mesh,
        pitch_mesh,
        pitch_worn_mesh,
        crease_line_mesh,
        crease_cross_mesh,
        pitch_mat,
        pitch_worn_mat,
        mow_band_mats,
    };

    commands.entity(root).with_children(|p| {
        spawn_stadium_apron(p, &ctx, &mut spawn_count);
        spawn_outfield_bands(p, &ctx, &mut spawn_count);
        spawn_pitch_and_creases(p, &ctx, &mut spawn_count);
        spawn_boundary_ring_and_boards(p, &ctx, &mut spawn_count);
        spawn_sight_screens(p, &ctx, &mut spawn_count);
        spawn_tiers_and_roof(p, &mut ctx, &mut spawn_count);
        spawn_floodlights(p, &ctx, &mut spawn_count);
        let crowd_count = spawn_crowd(p, &ctx, &mut spawn_count);
        info!("Stadium crowd spawned: {crowd_count} spectators");
        spawn_big_screen_and_dugouts(p, &mut ctx, batting_team, fielding_team, &mut spawn_count);
    });

    spawn_stumps(commands, root, &ctx, &mut spawn_count);

    commands.insert_resource(FloodlightMaterials {
        day: shared.lamp_day_mat.clone(),
        night: shared.lamp_night_mat.clone(),
    });

    if profile {
        let elapsed = t0.unwrap().elapsed();
        info!(
            "build_stadium profile: {:.1}ms, meshes +{}, materials +{}, entities {}",
            elapsed.as_secs_f64() * 1000.0,
            meshes.len() - mesh0,
            materials.len() - mat0,
            spawn_count,
        );
    }

    root
}

/// Expected crowd count for a standard bowl (used by tests).
pub fn expected_crowd_count() -> usize {
    let mut count = 0usize;
    for seg in 0..CROWD_SEGMENTS {
        if crowd_segment_skipped(seg) {
            continue;
        }
        for &tier in &CROWD_TIERS {
            if crowd_seat_over_aisle_gap(seg, tier) {
                continue;
            }
            count += crowd_seats_at(seg, tier);
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

/// Pitch albedo width in texels (maps once along `PITCH_LENGTH + 2` metres).
pub const PITCH_TEX_LENGTH_PX: u32 = 1024;
/// Pitch albedo height in texels (maps once along `PITCH_WIDTH` metres).
pub const PITCH_TEX_WIDTH_PX: u32 = 144;

fn create_pitch_image() -> Image {
    use bevy::asset::RenderAssetUsages;
    use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};

    let mut data =
        Vec::with_capacity((PITCH_TEX_LENGTH_PX * PITCH_TEX_WIDTH_PX * 4) as usize);
    for y in 0..PITCH_TEX_WIDTH_PX {
        for x in 0..PITCH_TEX_LENGTH_PX {
            let u = (x as f32 + 0.5) / PITCH_TEX_LENGTH_PX as f32;
            let v = (y as f32 + 0.5) / PITCH_TEX_WIDTH_PX as f32;
            let rgb = pitch_albedo_at(u, v);
            data.extend_from_slice(&[
                (rgb[0] * 255.0).round() as u8,
                (rgb[1] * 255.0).round() as u8,
                (rgb[2] * 255.0).round() as u8,
                255,
            ]);
        }
    }
    let mut img = Image::new(
        Extent3d {
            width: PITCH_TEX_LENGTH_PX,
            height: PITCH_TEX_WIDTH_PX,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.texture_descriptor.usage =
        TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::COPY_SRC;
    let mut sampler = ImageSamplerDescriptor::linear();
    sampler.address_mode_u = ImageAddressMode::ClampToEdge;
    sampler.address_mode_v = ImageAddressMode::ClampToEdge;
    sampler.mag_filter = ImageFilterMode::Linear;
    sampler.min_filter = ImageFilterMode::Linear;
    sampler.mipmap_filter = ImageFilterMode::Linear;
    sampler.set_anisotropic_filter(8);
    img.sampler = ImageSampler::Descriptor(sampler);
    append_rgba8_srgb_mip_chain(&mut img);
    img
}

/// Deterministic hash in `[0, 1)` for pitch procedural detail.
fn pitch_hash(x: u32, y: u32, seed: u32) -> f32 {
    let mut n = x
        .wrapping_mul(374_761_393)
        .wrapping_add(y.wrapping_mul(668_265_263))
        .wrapping_add(seed.wrapping_mul(982_451_653));
    n = (n ^ (n >> 13)).wrapping_mul(1_274_126_177);
    (n & 0x00FF_FFFF) as f32 / 16_777_215.0
}

fn pitch_value_noise(u: f32, v: f32, scale: f32, seed: u32) -> f32 {
    let x = u * scale;
    let y = v * scale;
    let ix = x.floor() as i32;
    let iy = y.floor() as i32;
    let fx = x - x.floor();
    let fy = y - y.floor();
    let smooth = |t: f32| t * t * (3.0 - 2.0 * t);
    let sfx = smooth(fx);
    let sfy = smooth(fy);
    let sample = |xi: i32, yi: i32| pitch_hash(xi as u32, yi as u32, seed);
    let a = sample(ix, iy);
    let b = sample(ix + 1, iy);
    let c = sample(ix, iy + 1);
    let d = sample(ix + 1, iy + 1);
    let ab = a + (b - a) * sfx;
    let cd = c + (d - c) * sfx;
    ab + (cd - ab) * sfy
}

fn pitch_fbm(u: f32, v: f32, seed: u32) -> f32 {
    let mut sum = 0.0;
    let mut amp = 0.55;
    let mut freq = 5.5;
    for octave in 0..3 {
        sum += pitch_value_noise(u, v, freq, seed.wrapping_add(octave * 41)) * amp;
        amp *= 0.5;
        freq *= 2.1;
    }
    sum
}

/// Normalised pitch length coordinate (`0` = bowler end, `1` = striker end).
fn pitch_length_u(world_x: f32) -> f32 {
    let span = geo::PITCH_LENGTH + 2.0;
    ((world_x + span / 2.0) / span).clamp(0.0, 1.0)
}

/// Darkening from footmarks and crease wear at normalised UV `(u, v)`.
fn pitch_wear_mask(u: f32, v: f32) -> f32 {
    let span = geo::PITCH_LENGTH + 2.0;
    let crease_x = geo::PITCH_HALF_LEN - geo::CREASE_DEPTH;
    let crease_u = pitch_length_u(crease_x);
    let bowler_crease_u = pitch_length_u(-crease_x);
    let bowler_land_u = pitch_length_u(-geo::PITCH_HALF_LEN + 1.8);

    let mut wear = 0.0_f32;
    for anchor in [crease_u, bowler_crease_u, bowler_land_u] {
        let along = ((u - anchor).abs() / 0.045).min(1.0);
        let across = (v - 0.5).abs() / (geo::PITCH_WIDTH / span * 0.55);
        let patch = (1.0 - along).max(0.0) * (1.0 - across.min(1.0));
        wear = wear.max(patch * 0.55);
    }
    // Central worn roller lane.
    let centre = (1.0 - ((v - 0.5).abs() / 0.16).min(1.0)) * 0.12;
    wear + centre
}

/// Pitch mesh extent in metres (matches the `Plane3d` size in `build_stadium`).
const PITCH_LENGTH_M: f32 = geo::PITCH_LENGTH + 2.0;
const PITCH_WIDTH_M: f32 = geo::PITCH_WIDTH;

/// Full sine cycles across normalised `u` for a feature spacing in metres along pitch length.
#[inline]
fn pitch_u_cycles(spacing_m: f32) -> f32 {
    PITCH_LENGTH_M / spacing_m
}

/// Full sine cycles across normalised `v` for a feature spacing in metres across pitch width.
#[inline]
fn pitch_v_cycles(spacing_m: f32) -> f32 {
    PITCH_WIDTH_M / spacing_m
}

/// Procedural pitch albedo at normalised UV `(u, v)` — length along `u`, width along `v`.
///
/// Frequencies are derived from real feature sizes on the ~22 m × 3 m wicket so every
/// band spans several texels at [`PITCH_TEX_LENGTH_PX`]×[`PITCH_TEX_WIDTH_PX`].
pub fn pitch_albedo_at(u: f32, v: f32) -> [f32; 3] {
    let u = u.clamp(0.0, 1.0);
    let v = v.clamp(0.0, 1.0);

    // ~10 cm roller compaction bands along the pitch (≈5 texels/cycle at 1024 px).
    let roller = (u * pitch_u_cycles(0.10) * TAU).sin() * 0.5 + 0.5;
    // Cross-pitch straw streaks from linear rolling (~30 cm).
    let streak = pitch_value_noise(
        u * pitch_u_cycles(0.55),
        v * pitch_v_cycles(0.30),
        3.0,
        71,
    ) * 0.5
        + 0.5;
    // Decimetre-scale soil mottling.
    let mottle = pitch_fbm(
        u * pitch_u_cycles(0.38),
        v * pitch_v_cycles(0.50),
        29,
    ) * 0.5
        + 0.5;
    // Fine grit (~9 cm) — visible but safely below Nyquist at this resolution.
    let fine = pitch_value_noise(
        u * pitch_u_cycles(0.09),
        v * pitch_v_cycles(0.11),
        4.0,
        17,
    ) * 0.5
        + 0.5;
    let scuff = pitch_fbm(
        u * pitch_u_cycles(0.14),
        v * pitch_v_cycles(0.18),
        53,
    ) * 0.5
        + 0.5;

    // Warm tan/khaki prepared wicket — clearly darker than white crease paint.
    let mut r = 0.46 + roller * 0.10 + fine * 0.06 + mottle * 0.07 + streak * 0.045;
    let mut g = 0.36 + roller * 0.085 + fine * 0.055 + mottle * 0.06 + streak * 0.038;
    let mut b = 0.24 + roller * 0.05 + fine * 0.04 + mottle * 0.045 + streak * 0.028;

    let wear = pitch_wear_mask(u, v);
    r -= wear * 0.22 + scuff * 0.11;
    g -= wear * 0.19 + scuff * 0.10;
    b -= wear * 0.16 + scuff * 0.09;

    [r.clamp(0.22, 0.62), g.clamp(0.18, 0.54), b.clamp(0.12, 0.40)]
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
                    let under = (74..=80).contains(&y) && (x / 6) % 2 == 0;
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

    fn draw_glyph(data: &mut [u8], ch: u8, ox: u32, oy: u32) {
        const FONT: [[u8; 7]; 26] = [
            [
                0b11100, 0b10100, 0b11100, 0b10100, 0b00000, 0b00000, 0b00000,
            ],
            [
                0b01000, 0b10100, 0b11100, 0b10100, 0b00000, 0b00000, 0b00000,
            ],
            [
                0b11100, 0b10000, 0b10000, 0b11100, 0b00000, 0b00000, 0b00000,
            ],
            [
                0b11100, 0b10000, 0b01100, 0b10000, 0b00000, 0b00000, 0b00000,
            ],
            [
                0b11100, 0b10100, 0b01100, 0b00100, 0b00000, 0b00000, 0b00000,
            ],
            [
                0b11100, 0b10000, 0b11000, 0b10000, 0b00000, 0b00000, 0b00000,
            ],
            [
                0b11100, 0b10000, 0b10100, 0b11100, 0b00000, 0b00000, 0b00000,
            ],
            [
                0b10100, 0b10100, 0b11100, 0b10100, 0b00000, 0b00000, 0b00000,
            ],
            [
                0b11100, 0b01000, 0b01000, 0b11100, 0b00000, 0b00000, 0b00000,
            ],
            [
                0b00100, 0b00100, 0b00100, 0b11100, 0b00000, 0b00000, 0b00000,
            ],
            [
                0b10100, 0b11000, 0b11000, 0b10100, 0b00000, 0b00000, 0b00000,
            ],
            [
                0b10000, 0b10000, 0b10000, 0b11100, 0b00000, 0b00000, 0b00000,
            ],
            [
                0b10100, 0b11100, 0b10100, 0b10100, 0b00000, 0b00000, 0b00000,
            ],
            [
                0b10100, 0b11100, 0b11100, 0b10100, 0b00000, 0b00000, 0b00000,
            ],
            [
                0b11100, 0b10100, 0b10100, 0b11100, 0b00000, 0b00000, 0b00000,
            ],
            [
                0b11100, 0b10100, 0b11100, 0b10000, 0b00000, 0b00000, 0b00000,
            ],
            [
                0b11100, 0b10100, 0b11100, 0b00100, 0b00000, 0b00000, 0b00000,
            ],
            [
                0b11100, 0b11000, 0b11000, 0b10100, 0b00000, 0b00000, 0b00000,
            ],
            [
                0b11100, 0b10000, 0b11100, 0b00100, 0b00000, 0b00000, 0b00000,
            ],
            [
                0b11100, 0b01000, 0b01000, 0b01000, 0b00000, 0b00000, 0b00000,
            ],
            [
                0b10100, 0b10100, 0b10100, 0b11100, 0b00000, 0b00000, 0b00000,
            ],
            [
                0b10100, 0b10100, 0b10100, 0b01000, 0b00000, 0b00000, 0b00000,
            ],
            [
                0b10100, 0b10100, 0b11100, 0b10100, 0b00000, 0b00000, 0b00000,
            ],
            [
                0b10100, 0b01000, 0b01000, 0b10100, 0b00000, 0b00000, 0b00000,
            ],
            [
                0b10100, 0b01000, 0b01000, 0b01000, 0b00000, 0b00000, 0b00000,
            ],
            [
                0b11100, 0b00100, 0b01000, 0b11100, 0b00000, 0b00000, 0b00000,
            ],
        ];
        let i = ch.saturating_sub(b'A') as usize % 26;
        for (row, &glyph_row) in FONT[i].iter().enumerate() {
            for col in 0..5 {
                if glyph_row & (1 << (4 - col)) != 0 {
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
    use super::{stadium_ground_radius, *};

    #[test]
    fn crowd_count_in_target_range() {
        let n = expected_crowd_count();
        assert!(n >= 350, "crowd too sparse: {n}");
        assert!(n <= 550, "crowd too dense: {n}");
    }

    fn simulated_crowd_spawn_count() -> usize {
        let mut count = 0usize;
        for seg in 0..CROWD_SEGMENTS {
            if crowd_segment_skipped(seg) {
                continue;
            }
            for &tier in &CROWD_TIERS {
                if crowd_seat_over_aisle_gap(seg, tier) {
                    continue;
                }
                count += crowd_seats_at(seg, tier);
            }
        }
        count
    }

    #[test]
    fn crowd_seats_not_over_structural_aisle_gaps() {
        for seg in 0..CROWD_SEGMENTS {
            if crowd_segment_skipped(seg) {
                continue;
            }
            for &tier in &CROWD_TIERS {
                if crowd_seat_over_aisle_gap(seg, tier) {
                    continue;
                }
                let structural_seg = crowd_seat_structural_segment(seg, tier);
                assert!(
                    !structural_seg.is_multiple_of(AISLE_EVERY),
                    "seg {seg} tier {tier} maps to structural aisle segment {structural_seg}"
                );
            }
        }
    }

    #[test]
    fn crowd_aisle_gap_filter_is_material_but_not_excessive() {
        let mut unfiltered = 0usize;
        let mut filtered = 0usize;
        for seg in 0..CROWD_SEGMENTS {
            if crowd_segment_skipped(seg) {
                continue;
            }
            for &tier in &CROWD_TIERS {
                let seats = crowd_seats_at(seg, tier);
                unfiltered += seats;
                if crowd_seat_over_aisle_gap(seg, tier) {
                    filtered += seats;
                }
            }
        }
        assert!(filtered > 0, "aisle gap filter removed no seats");
        assert!(
            filtered * 4 < unfiltered,
            "aisle gap filter removed too many seats: {filtered} of {unfiltered}"
        );
    }

    #[test]
    fn expected_crowd_count_matches_spawn_loop() {
        assert_eq!(expected_crowd_count(), simulated_crowd_spawn_count());
    }

    #[test]
    fn bowl_outer_exceeds_boundary() {
        let bowl = BowlLayout::from_boundary(65.0);
        // Lower + upper deck extends well beyond the rope.
        assert!(bowl.outer_radius() > 65.0 + 25.0);
        assert!(bowl.stand_top_height() > 14.0);
    }

    #[test]
    fn upper_deck_set_back_from_lower() {
        let bowl = BowlLayout::from_boundary(65.0);
        let lower_top = bowl.tier_mid_radius(LOWER_TIER_COUNT - 1);
        let upper_bottom = bowl.tier_mid_radius(LOWER_TIER_COUNT);
        assert!(upper_bottom > lower_top + 2.0);
    }

    #[test]
    fn apron_ground_extends_beyond_bowl() {
        let bowl = BowlLayout::from_boundary(65.0);
        let radius = stadium_ground_radius(bowl.outer_radius());
        assert!(radius > bowl.outer_radius());
        // Mown outfield must stay inside the apron (no regression to floating bowl).
        let outfield_half = (65.0 + 6.0) * 2.05 / 2.0;
        assert!(radius > outfield_half);
    }

    #[test]
    fn pitch_albedo_has_lengthwise_variation() {
        let mid = pitch_albedo_at(0.5, 0.5);
        let shifted = pitch_albedo_at(0.52, 0.5);
        assert_ne!(mid, shifted);
    }

    #[test]
    fn pitch_crease_zones_are_darker_than_centre() {
        let span = geo::PITCH_LENGTH + 2.0;
        let crease_x = geo::PITCH_HALF_LEN - geo::CREASE_DEPTH;
        let crease_u = (crease_x + span / 2.0) / span;
        let centre = pitch_albedo_at(0.5, 0.5);
        let crease = pitch_albedo_at(crease_u, 0.5);
        let centre_luma = centre[0] * 0.299 + centre[1] * 0.587 + centre[2] * 0.114;
        let crease_luma = crease[0] * 0.299 + crease[1] * 0.587 + crease[2] * 0.114;
        assert!(crease_luma < centre_luma - 0.02);
    }

    #[test]
    fn pitch_texture_is_deterministic() {
        let a = pitch_albedo_at(0.33, 0.67);
        let b = pitch_albedo_at(0.33, 0.67);
        assert_eq!(a, b);
    }

    #[test]
    fn pitch_texture_has_mip_chain_and_aspect() {
        let image = create_pitch_image();
        assert_eq!(image.texture_descriptor.size.width, PITCH_TEX_LENGTH_PX);
        assert_eq!(image.texture_descriptor.size.height, PITCH_TEX_WIDTH_PX);
        assert!(
            image.texture_descriptor.mip_level_count > 1,
            "pitch albedo needs mips for grazing-angle views"
        );
        let data = image.data.as_ref().expect("pitch image must have CPU data");
        assert_eq!(
            data.len(),
            outfield_grass::expected_rgba8_mip_data_len(PITCH_TEX_LENGTH_PX, PITCH_TEX_WIDTH_PX)
        );
        let aspect = PITCH_TEX_LENGTH_PX as f32 / PITCH_TEX_WIDTH_PX as f32;
        let world_aspect = PITCH_LENGTH_M / PITCH_WIDTH_M;
        assert!(
            (aspect - world_aspect).abs() < 0.35,
            "texture aspect {aspect:.2} should track wicket aspect {world_aspect:.2}"
        );
    }

    #[test]
    fn pitch_mean_albedo_is_darker_than_crease_white() {
        let mut sum = [0.0f32; 3];
        let steps = 32usize;
        let mut count = 0.0f32;
        for yi in 0..steps {
            for xi in 0..steps {
                let u = (xi as f32 + 0.5) / steps as f32;
                let v = (yi as f32 + 0.5) / steps as f32;
                let rgb = pitch_albedo_at(u, v);
                sum[0] += rgb[0];
                sum[1] += rgb[1];
                sum[2] += rgb[2];
                count += 1.0;
            }
        }
        let mean = [sum[0] / count, sum[1] / count, sum[2] / count];
        let pitch_luma = mean[0] * 0.299 + mean[1] * 0.587 + mean[2] * 0.114;
        let crease_luma = 1.0;
        assert!(
            pitch_luma < crease_luma - 0.40,
            "pitch mean {mean:?} luma {pitch_luma:.3} must sit well below crease white"
        );
        assert!(
            mean[0] < 0.58 && mean[1] < 0.48,
            "pitch mean {mean:?} should read as tan, not cream"
        );
    }
}

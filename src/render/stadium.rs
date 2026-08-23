use std::f32::consts::{PI, TAU};

use crate::core::geometry as geo;
use crate::core::stadiums::Stadium;
use crate::core::teams::Team;
use crate::render::crowd;
use crate::render::environment;
use crate::render::outfield_grass::{self, MOW_BAND_COUNT, append_rgba8_srgb_mip_chain};
use crate::render::ring_geometry::{
    floodlight_angles, floodlight_radius, ring_band_specs, ring_boxes_mesh, ring_position,
    ring_segment_transform, ring_tangent, ring_tube_mesh, stadium_ground_disc_mesh,
    stadium_ground_radius,
};
use crate::render::stand_geometry as sg;
use crate::render::{FloodlightFixture, FloodlightMaterials, NightEnvironmentLight};
use bevy::prelude::*;

#[derive(Component)]
pub struct StadiumRoot;

#[derive(Component)]
pub struct Stumps {
    /// true = striker's (batsman) end.
    pub striker_end: bool,
}

const STUMP_GAP: f32 = 0.114;
pub(crate) const LOWER_TIER_COUNT: usize = 7;
pub(crate) const UPPER_TIER_COUNT: usize = 5;
pub(crate) const TIER_COUNT: usize = LOWER_TIER_COUNT + UPPER_TIER_COUNT;
const TIER_MAT_COUNT: usize = 8;
const TIER_SEGMENTS: usize = 96;
const FACADE_SEGMENTS: usize = 48;
const AISLE_EVERY: usize = 8;
/// Radial trusses in the cantilever roof. Two per aisle bay reads as a real
/// structure from the establishing crane without ballooning the vertex count.
const ROOF_TRUSS_COUNT: usize = 24;
/// Light and speaker clusters slung from the roof soffit.
const ROOF_CLUSTER_COUNT: usize = 18;
/// Flags ranged along the roof crown.
const ROOF_FLAG_COUNT: usize = 24;
/// Every second aisle carries a tunnel mouth; the rest stay stair-only.
const VOMITORY_EVERY_NTH_AISLE: usize = 2;
/// Vomitories pierce the bowl at these tiers (lower deck, upper deck).
const VOMITORY_TIERS: [usize; 2] = [2, LOWER_TIER_COUNT + 1];
const GATE_COUNT: usize = 8;
/// Broadcast camera gantry positions, in radians around the bowl.
const GANTRY_ANGLES: [f32; 4] = [PI * 0.5, PI * 1.5, PI * 0.85, PI * 1.15];

pub(crate) struct BowlLayout {
    pub(crate) inner_radius: f32,
    pub(crate) tier_depth: f32,
    pub(crate) tier_rise: f32,
    pub(crate) tread_thickness: f32,
    pub(crate) base_height: f32,
    pub(crate) upper_deck_setback: f32,
    pub(crate) upper_deck_rise_gap: f32,
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

    pub(crate) fn lower_outer_radius(&self) -> f32 {
        self.inner_radius + self.tier_depth * LOWER_TIER_COUNT as f32
    }

    pub(crate) fn outer_radius(&self) -> f32 {
        self.lower_outer_radius()
            + self.upper_deck_setback
            + self.tier_depth * UPPER_TIER_COUNT as f32
    }

    pub(crate) fn upper_inner_radius(&self) -> f32 {
        self.lower_outer_radius() + self.upper_deck_setback
    }

    pub(crate) fn tier_mid_radius(&self, tier: usize) -> f32 {
        if tier < LOWER_TIER_COUNT {
            self.inner_radius + (tier as f32 + 0.5) * self.tier_depth
        } else {
            let upper = tier - LOWER_TIER_COUNT;
            self.upper_inner_radius() + (upper as f32 + 0.5) * self.tier_depth
        }
    }

    pub(crate) fn tier_height(&self, tier: usize) -> f32 {
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

    pub(crate) fn stand_top_height(&self) -> f32 {
        self.tier_height(TIER_COUNT - 1) + self.tread_thickness
    }

    pub(crate) fn is_upper_deck(&self, tier: usize) -> bool {
        tier >= LOWER_TIER_COUNT
    }

    /// Walking surface of a tier tread: where seats stand and spectators sit.
    /// `tier_height` is the *underside* of the tread slab.
    pub(crate) fn tread_top(&self, tier: usize) -> f32 {
        self.tier_height(tier) + self.tread_thickness
    }

    /// Radial depth of the tread slab, matching the spawned tread geometry.
    pub(crate) fn tread_depth(&self) -> f32 {
        self.tier_depth * 0.92
    }

    /// Pitch-side radius of a tier's tread (where its riser face stands).
    pub(crate) fn tier_inner_radius(&self, tier: usize) -> f32 {
        if tier < LOWER_TIER_COUNT {
            self.inner_radius + tier as f32 * self.tier_depth
        } else {
            self.upper_inner_radius() + (tier - LOWER_TIER_COUNT) as f32 * self.tier_depth
        }
    }

    /// Deck level where the lower bowl's vomitories and concourse sit.
    pub(crate) fn upper_deck_base_height(&self) -> f32 {
        self.base_height + LOWER_TIER_COUNT as f32 * self.tier_rise + self.upper_deck_rise_gap
    }

    pub(crate) fn concourse_radius(&self) -> f32 {
        self.lower_outer_radius() + self.upper_deck_setback * 0.42
    }

    pub(crate) fn concourse_height(&self) -> f32 {
        self.base_height
            + LOWER_TIER_COUNT as f32 * self.tier_rise
            + self.upper_deck_rise_gap * 0.35
    }

    /// Cantilever roof: tips reach in over the back of the upper deck, rear
    /// supports land on the columns behind it.
    pub(crate) fn roof_spec(&self) -> sg::RoofSpec {
        let top = self.stand_top_height();
        sg::RoofSpec {
            truss_count: ROOF_TRUSS_COUNT,
            inner_radius: self.upper_inner_radius() + self.tier_depth * 0.6,
            outer_radius: self.outer_radius() + 2.6,
            inner_y: top + 6.4,
            outer_y: top + 9.2,
            depth: 2.4,
            member: 0.22,
            camber: 1.1,
            web_panels: 6,
        }
    }
}

pub(crate) struct SharedStadiumAssets {
    pub(crate) unit_cuboid: Handle<Mesh>,
    pub(crate) rope_mesh: Handle<Mesh>,
    pub(crate) column_mesh: Handle<Mesh>,
    pub(crate) tower_pole_mesh: Handle<Mesh>,
    pub(crate) tower_truss_mesh: Handle<Mesh>,
    pub(crate) lamp_bank_mesh: Handle<Mesh>,
    pub(crate) rope_mat: Handle<StandardMaterial>,
    pub(crate) white_mat: Handle<StandardMaterial>,
    pub(crate) stump_mat: Handle<StandardMaterial>,
    pub(crate) sight_screen_mat: Handle<StandardMaterial>,
    pub(crate) board_frame_mat: Handle<StandardMaterial>,
    pub(crate) tier_mats: [Handle<StandardMaterial>; TIER_MAT_COUNT],
    pub(crate) riser_mat: Handle<StandardMaterial>,
    pub(crate) rail_mat: Handle<StandardMaterial>,
    pub(crate) column_mat: Handle<StandardMaterial>,
    pub(crate) canopy_mat: Handle<StandardMaterial>,
    pub(crate) facade_mat: Handle<StandardMaterial>,
    pub(crate) concourse_mat: Handle<StandardMaterial>,
    pub(crate) apron_mat: Handle<StandardMaterial>,
    pub(crate) pavilion_mat: Handle<StandardMaterial>,
    pub(crate) media_box_mat: Handle<StandardMaterial>,
    pub(crate) roof_truss_mat: Handle<StandardMaterial>,
    pub(crate) tower_mat: Handle<StandardMaterial>,
    pub(crate) lamp_day_mat: Handle<StandardMaterial>,
    pub(crate) lamp_night_mat: Handle<StandardMaterial>,
    pub(crate) sponsor_mats: Vec<Handle<StandardMaterial>>,
    /// Moulded seat plastic. Vertex colours carry the palette, so every seat in
    /// the bowl shares this one handle.
    pub(crate) seat_mat: Handle<StandardMaterial>,
    /// Painted structural steel: roof trusses, gantries, screen supports.
    pub(crate) steel_mat: Handle<StandardMaterial>,
    /// Weathered concrete: facade ribs, parapet, vomitory surrounds, gates.
    pub(crate) concrete_mat: Handle<StandardMaterial>,
    /// Architectural glazing on the facade and media box.
    pub(crate) glass_mat: Handle<StandardMaterial>,
    /// Opaque roof decking (coated metal).
    pub(crate) roof_panel_mat: Handle<StandardMaterial>,
    /// Translucent polycarbonate roof bays that let daylight through.
    pub(crate) roof_glazing_mat: Handle<StandardMaterial>,
    /// Banners, flags and awnings.
    pub(crate) fabric_mat: Handle<StandardMaterial>,
    /// Matte rubber: stair nosings, tunnel floors, dugout mats.
    pub(crate) rubber_mat: Handle<StandardMaterial>,
    /// Unlit interiors seen through vomitories and gates.
    pub(crate) tunnel_mat: Handle<StandardMaterial>,
    /// Self-lit concourse soffits read through the facade openings.
    pub(crate) concourse_glow_mat: Handle<StandardMaterial>,
    pub(crate) gate_number_mats: Vec<Handle<StandardMaterial>>,
    pub(crate) seat_palette: sg::SeatPalette,
    pub(crate) team_tones: Vec<[f32; 3]>,
    pub(crate) grass_tex: Handle<Image>,
    pub(crate) grass_mesh: Handle<Mesh>,
    pub(crate) stump_cylinder_mesh: Handle<Mesh>,
    pub(crate) stump_bail_mesh: Handle<Mesh>,
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
        materials.add(concrete_mat(tint(shade, 0.04)))
    });

    // Seats take the stand tint but pushed brighter and more saturated than the
    // concrete around them — moulded polypropylene, not painted structure.
    let seat_tone = |mul: f32, add: f32| {
        let c = tint(mul, add).to_srgba();
        [c.red, c.green, c.blue]
    };
    let seat_palette = sg::SeatPalette {
        tones: [
            seat_tone(1.28, 0.06),
            seat_tone(1.12, 0.10),
            seat_tone(1.42, 0.03),
        ],
        // Real grounds pick the mosaic out in a near-white or cream seat.
        accent: seat_tone(0.55, 0.52),
    };

    SharedStadiumAssets {
        unit_cuboid: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        rope_mesh: meshes.add(Cuboid::new(1.0, 0.08, 0.08)),
        column_mesh: meshes.add(Cylinder::new(0.22, 1.0)),
        tower_pole_mesh: meshes.add(Cylinder::new(0.38, 1.0)),
        tower_truss_mesh: meshes.add(Cuboid::new(1.0, 0.35, 0.35)),
        lamp_bank_mesh: meshes.add(Cuboid::new(1.0, 0.55, 0.42)),
        rope_mat: materials.add(cloth_mat(Color::srgb_u8(0xEE, 0xEE, 0xEE))),
        white_mat: materials.add(matte_paint_mat(Color::WHITE)),
        stump_mat: materials.add(lacquered_wood_mat(Color::srgb_u8(0xF5, 0xE9, 0xC8))),
        sight_screen_mat: materials.add(matte_paint_mat(Color::srgb_u8(0x1A, 0x1A, 0x1E))),
        board_frame_mat: materials.add(matte_paint_mat(Color::srgb_u8(0x08, 0x12, 0x1C))),
        tier_mats,
        riser_mat: materials.add(concrete_mat(tint(0.62, 0.02))),
        rail_mat: materials.add(painted_steel_mat(tint(0.48, 0.03))),
        column_mat: materials.add(concrete_mat(tint(0.55, 0.06))),
        canopy_mat: materials.add(painted_steel_mat(tint(0.38, 0.05))),
        facade_mat: materials.add(concrete_mat(Color::srgb_u8(0x6A, 0x6E, 0x74))),
        concourse_mat: materials.add(concrete_mat(Color::srgb_u8(0x8A, 0x8E, 0x92))),
        apron_mat: materials.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.94,
            reflectance: 0.28,
            ..default()
        }),
        pavilion_mat: materials.add(concrete_mat(Color::srgb_u8(0x5C, 0x60, 0x68))),
        media_box_mat: materials.add(painted_steel_mat(Color::srgb_u8(0x2A, 0x32, 0x3C))),
        roof_truss_mat: materials.add(painted_steel_mat(Color::srgb_u8(0x3C, 0x40, 0x48))),
        tower_mat: materials.add(painted_steel_mat(Color::srgb_u8(0x48, 0x4C, 0x52))),
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
        // Vertex-coloured merged geometry: base_color stays white so the mesh
        // colours come through unmodulated.
        seat_mat: materials.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.44,
            metallic: 0.0,
            reflectance: 0.46,
            ..default()
        }),
        steel_mat: materials.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.34,
            metallic: 0.82,
            reflectance: 0.62,
            ..default()
        }),
        concrete_mat: materials.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.96,
            metallic: 0.0,
            reflectance: 0.14,
            ..default()
        }),
        glass_mat: materials.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.07,
            metallic: 0.0,
            reflectance: 0.92,
            alpha_mode: AlphaMode::Blend,
            double_sided: true,
            cull_mode: None,
            ..default()
        }),
        roof_panel_mat: materials.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.30,
            metallic: 0.55,
            reflectance: 0.55,
            double_sided: true,
            cull_mode: None,
            ..default()
        }),
        roof_glazing_mat: materials.add(StandardMaterial {
            base_color: Color::WHITE.with_alpha(0.34),
            perceptual_roughness: 0.16,
            metallic: 0.0,
            reflectance: 0.72,
            alpha_mode: AlphaMode::Blend,
            double_sided: true,
            cull_mode: None,
            ..default()
        }),
        fabric_mat: materials.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.90,
            metallic: 0.0,
            reflectance: 0.06,
            double_sided: true,
            cull_mode: None,
            ..default()
        }),
        rubber_mat: materials.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.74,
            metallic: 0.0,
            reflectance: 0.05,
            ..default()
        }),
        tunnel_mat: materials.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.88,
            metallic: 0.0,
            reflectance: 0.02,
            ..default()
        }),
        concourse_glow_mat: materials.add(StandardMaterial {
            base_color: Color::WHITE,
            emissive: LinearRgba::from(Color::srgb(0.30, 0.28, 0.22)),
            perceptual_roughness: 0.80,
            reflectance: 0.10,
            ..default()
        }),
        gate_number_mats: (0..GATE_COUNT)
            .map(|i| materials.add(texture_mat(images.add(gate_number_image(i + 1)))))
            .collect(),
        seat_palette,
        team_tones: vec![
            [0.78, 0.16, 0.16],
            [0.94, 0.78, 0.24],
            [0.16, 0.34, 0.68],
            [0.92, 0.92, 0.88],
            [0.14, 0.46, 0.28],
        ],
        grass_tex: images.add(crate::render::create_outfield_grass_image()),
        grass_mesh: meshes.add(Plane3d::default().mesh().size(1.0, 1.0).subdivisions(4)),
        stump_cylinder_mesh: meshes.add(Cylinder::new(0.02, geo::STUMP_HEIGHT)),
        stump_bail_mesh: meshes.add(Cuboid::new(0.03, 0.02, STUMP_GAP * 2.0)),
    }
}

pub(crate) struct StadiumBuildCtx<'a> {
    pub(crate) meshes: &'a mut Assets<Mesh>,
    pub(crate) materials: &'a mut Assets<StandardMaterial>,
    pub(crate) images: &'a mut Assets<Image>,
    pub(crate) asset_server: &'a AssetServer,
    pub(crate) stadium: &'a Stadium,
    pub(crate) shared: &'a SharedStadiumAssets,
    pub(crate) bowl: BowlLayout,
    pub(crate) outfield_base: Color,
    pub(crate) batting_crest_mat: Handle<StandardMaterial>,
    pub(crate) fielding_crest_mat: Handle<StandardMaterial>,
    pub(crate) apron_disc_mesh: Handle<Mesh>,
    pub(crate) rope_ring_mesh: Handle<Mesh>,
    pub(crate) pitch_mesh: Handle<Mesh>,
    pub(crate) pitch_worn_mesh: Handle<Mesh>,
    pub(crate) crease_line_mesh: Handle<Mesh>,
    pub(crate) crease_cross_mesh: Handle<Mesh>,
    pub(crate) pitch_mat: Handle<StandardMaterial>,
    pub(crate) pitch_worn_mat: Handle<StandardMaterial>,
    pub(crate) mow_band_mats: Vec<Handle<StandardMaterial>>,
}

pub(crate) fn track_spawn(spawn_count: &mut usize) {
    *spawn_count += 1;
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

/// Spawn one merged mesh, skipping the entity when the builder came back empty.
fn spawn_merged(
    p: &mut ChildSpawnerCommands,
    meshes: &mut Assets<Mesh>,
    mesh: Mesh,
    material: Handle<StandardMaterial>,
    spawn_count: &mut usize,
) {
    if sg::mesh_is_empty(&mesh) {
        return;
    }
    p.spawn((
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(material),
        Transform::IDENTITY,
    ));
    track_spawn(spawn_count);
}

fn spawn_tiers_and_roof(
    p: &mut ChildSpawnerCommands,
    ctx: &mut StadiumBuildCtx<'_>,
    spawn_count: &mut usize,
) {
    spawn_seating_bowl(p, ctx, spawn_count);
    spawn_vomitories_and_stairs(p, ctx, spawn_count);
    spawn_concourse_and_facade(p, ctx, spawn_count);
    spawn_roof(p, ctx, spawn_count);
    spawn_pavilions_and_media_box(p, ctx, spawn_count);
    spawn_bowl_detail(p, ctx, spawn_count);
}

/// Raked seating bowl: tread slabs, risers, guard rails and — the part that
/// makes it read as a stadium rather than a greybox — a merged mesh of real
/// seats on every tread.
fn spawn_seating_bowl(
    p: &mut ChildSpawnerCommands,
    ctx: &mut StadiumBuildCtx<'_>,
    spawn_count: &mut usize,
) {
    let bowl = &ctx.bowl;
    let tread_arc = 2.0 * PI * bowl.inner_radius / TIER_SEGMENTS as f32 * 1.02;
    let tread_radial = bowl.tread_depth();

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
            let inner_r = bowl.tier_inner_radius(tier) - 0.08;
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

        // One merged mesh per tread carries every seat in that row. Tens of
        // thousands of seat entities would dominate frame time, so the palette
        // rides on `Mesh::ATTRIBUTE_COLOR` and the whole bowl shares one
        // material handle.
        let band = sg::SeatBand {
            segments: TIER_SEGMENTS,
            aisle_every: AISLE_EVERY,
            radius: mid_r,
            tread_top: bowl.tread_top(tier),
            row: tier,
            // Only the upper deck carries the block mosaic, the way real grounds
            // pick out a pattern across the tier that empties first.
            mosaic: bowl.is_upper_deck(tier),
        };
        let seats = sg::seat_band_mesh(&band, &ctx.shared.seat_palette);
        spawn_merged(
            p,
            ctx.meshes,
            seats,
            ctx.shared.seat_mat.clone(),
            spawn_count,
        );
    }
}

/// Tunnel mouths cut through the bowl at the aisles, and the stair flights that
/// climb every aisle between them.
fn spawn_vomitories_and_stairs(
    p: &mut ChildSpawnerCommands,
    ctx: &mut StadiumBuildCtx<'_>,
    spawn_count: &mut usize,
) {
    let bowl = &ctx.bowl;
    let vom_angles = sg::vomitory_angles(TIER_SEGMENTS, AISLE_EVERY, VOMITORY_EVERY_NTH_AISLE);
    let mut voms = Vec::with_capacity(vom_angles.len() * VOMITORY_TIERS.len());
    for &tier in &VOMITORY_TIERS {
        let mouth_r = bowl.tier_inner_radius(tier);
        for &angle in &vom_angles {
            voms.push(sg::Vomitory {
                angle,
                mouth_radius: mouth_r,
                // Bores back under roughly two rows of seating.
                depth: bowl.tier_depth * 1.9,
                width: TAU * mouth_r / TIER_SEGMENTS as f32 * 0.86,
                height: 2.1,
                floor_y: bowl.tread_top(tier),
            });
        }
    }
    let interior = sg::vomitory_interior_mesh(&voms);
    spawn_merged(
        p,
        ctx.meshes,
        interior,
        ctx.shared.tunnel_mat.clone(),
        spawn_count,
    );
    let frame = sg::vomitory_frame_mesh(&voms);
    spawn_merged(
        p,
        ctx.meshes,
        frame,
        ctx.shared.concrete_mat.clone(),
        spawn_count,
    );

    // Aisles are gaps in every tread ring, so without stairs they read as slots
    // cut through the bowl. Fill each one with a solid flight, stepping aside
    // where a tunnel mouth breaks through.
    let mut flights = Vec::new();
    for (aisle, angle) in sg::aisle_angles(TIER_SEGMENTS, AISLE_EVERY)
        .into_iter()
        .enumerate()
    {
        let pierced = aisle.is_multiple_of(VOMITORY_EVERY_NTH_AISLE);
        for tier in 0..TIER_COUNT {
            if pierced && VOMITORY_TIERS.iter().any(|&t| tier == t || tier == t + 1) {
                continue;
            }
            let inner = bowl.tier_inner_radius(tier);
            flights.push(sg::StairFlight {
                angle,
                inner_radius: inner,
                run: bowl.tier_depth,
                rise: bowl.tier_rise,
                base_y: bowl.tread_top(tier),
                foot_y: bowl.tier_height(tier) - 0.08,
                width: TAU * inner / TIER_SEGMENTS as f32 * 0.94,
                steps: 3,
            });
        }
    }
    let stairs = sg::stair_flights_mesh(&flights);
    spawn_merged(
        p,
        ctx.meshes,
        stairs,
        ctx.shared.concrete_mat.clone(),
        spawn_count,
    );
}

/// Concourse deck, the structured outer wall, and the ground-level entry gates.
fn spawn_concourse_and_facade(
    p: &mut ChildSpawnerCommands,
    ctx: &mut StadiumBuildCtx<'_>,
    spawn_count: &mut usize,
) {
    let bowl = &ctx.bowl;
    let concourse_r = bowl.concourse_radius();
    let concourse_h = bowl.concourse_height();
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

    // Lit soffit above the concourse slab, so the gap between the decks reads as
    // an occupied level through the facade openings instead of a black slot.
    let reveal = sg::concourse_reveal_mesh(
        FACADE_SEGMENTS,
        concourse_r + bowl.upper_deck_setback * 0.30,
        concourse_h,
        bowl.upper_deck_setback * 0.70,
        2.3,
    );
    spawn_merged(
        p,
        ctx.meshes,
        reveal,
        ctx.shared.concourse_glow_mat.clone(),
        spawn_count,
    );

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
    let upper_facade_base = bowl.upper_deck_base_height();
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

    // Ground storey carrying the upper facade down to the apron. Without it the
    // outer wall floats and the gates have nothing to sit in.
    let plinth_arc = 2.0 * PI * upper_facade_r / FACADE_SEGMENTS as f32 * 1.04;
    let plinth_specs = ring_band_specs(
        FACADE_SEGMENTS,
        0,
        upper_facade_r,
        upper_facade_base * 0.5,
        plinth_arc,
        upper_facade_base,
        3.2,
    );
    p.spawn((
        Mesh3d(ctx.meshes.add(ring_boxes_mesh(&plinth_specs))),
        MeshMaterial3d(ctx.shared.facade_mat.clone()),
        Transform::IDENTITY,
    ));
    track_spawn(spawn_count);

    // Outer face of the wall, where the ribs and glazing live.
    let facade = sg::FacadeSpec {
        segments: FACADE_SEGMENTS,
        radius: upper_facade_r + 2.1,
        base_y: upper_facade_base,
        height: upper_facade_h,
        rib_width: 1.15,
        rib_depth: 0.85,
        glazing_bands: 6,
    };
    let ribs = sg::facade_rib_mesh(&facade);
    spawn_merged(
        p,
        ctx.meshes,
        ribs,
        ctx.shared.concrete_mat.clone(),
        spawn_count,
    );
    let glazing = sg::facade_glazing_mesh(&facade);
    spawn_merged(
        p,
        ctx.meshes,
        glazing,
        ctx.shared.glass_mat.clone(),
        spawn_count,
    );
    let parapet = sg::facade_parapet_mesh(&facade);
    spawn_merged(
        p,
        ctx.meshes,
        parapet,
        ctx.shared.concrete_mat.clone(),
        spawn_count,
    );

    // Numbered entry gates at ground level, cut into the plinth.
    const GATE_HEIGHT: f32 = 4.2;
    let gate_r = upper_facade_r + 1.6;
    let gates = sg::gate_angles(GATE_COUNT);
    let portals = sg::gate_portal_mesh(&gates, gate_r, 5.4, GATE_HEIGHT);
    spawn_merged(
        p,
        ctx.meshes,
        portals,
        ctx.shared.concrete_mat.clone(),
        spawn_count,
    );
    for (i, &angle) in gates.iter().enumerate() {
        p.spawn((
            Mesh3d(ctx.shared.unit_cuboid.clone()),
            MeshMaterial3d(ctx.shared.gate_number_mats[i].clone()),
            ring_segment_transform(angle, gate_r + 1.6, GATE_HEIGHT + 0.55)
                .with_scale(Vec3::new(1.15, 1.15, 0.10)),
        ));
        track_spawn(spawn_count);
    }

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
        let upper_col_h = UPPER_TIER_COUNT as f32 * bowl.tier_rise + 2.4;
        p.spawn((
            Mesh3d(ctx.shared.column_mesh.clone()),
            MeshMaterial3d(ctx.shared.column_mat.clone()),
            Transform::from_translation(ring_position(
                a,
                upper_col_r,
                upper_facade_base + upper_col_h * 0.5,
            ))
            .with_scale(Vec3::new(1.15, upper_col_h, 1.15)),
        ));
        track_spawn(spawn_count);
    }
}

/// Cantilever roof: radial trusses off the rear columns, a tension ring at the
/// free tips, mixed opaque and glazed decking, a shaded soffit and the services
/// slung beneath it.
fn spawn_roof(
    p: &mut ChildSpawnerCommands,
    ctx: &mut StadiumBuildCtx<'_>,
    spawn_count: &mut usize,
) {
    let spec = ctx.bowl.roof_spec();

    let trusses = sg::roof_truss_mesh(&spec);
    spawn_merged(
        p,
        ctx.meshes,
        trusses,
        ctx.shared.steel_mat.clone(),
        spawn_count,
    );
    let deck = sg::roof_panel_mesh(&spec, false);
    spawn_merged(
        p,
        ctx.meshes,
        deck,
        ctx.shared.roof_panel_mat.clone(),
        spawn_count,
    );
    // Glazed bays let daylight down onto the back rows.
    let glazed = sg::roof_panel_mesh(&spec, true);
    spawn_merged(
        p,
        ctx.meshes,
        glazed,
        ctx.shared.roof_glazing_mat.clone(),
        spawn_count,
    );
    let soffit = sg::roof_soffit_mesh(&spec);
    spawn_merged(
        p,
        ctx.meshes,
        soffit,
        ctx.shared.concrete_mat.clone(),
        spawn_count,
    );
    let edge = sg::roof_edge_mesh(&spec);
    spawn_merged(
        p,
        ctx.meshes,
        edge,
        ctx.shared.steel_mat.clone(),
        spawn_count,
    );
    let clusters = sg::roof_cluster_mesh(&spec, ROOF_CLUSTER_COUNT);
    spawn_merged(
        p,
        ctx.meshes,
        clusters,
        ctx.shared.steel_mat.clone(),
        spawn_count,
    );
    let flags = sg::roof_flag_mesh(&spec, ROOF_FLAG_COUNT, &ctx.shared.team_tones);
    spawn_merged(
        p,
        ctx.meshes,
        flags,
        ctx.shared.fabric_mat.clone(),
        spawn_count,
    );
}

fn spawn_pavilions_and_media_box(
    p: &mut ChildSpawnerCommands,
    ctx: &mut StadiumBuildCtx<'_>,
    spawn_count: &mut usize,
) {
    let bowl = &ctx.bowl;
    let top = bowl.stand_top_height();

    // Pavilion blocks rising above the stand line at four quadrants + two ends.
    let pavilion_angles: [f32; 6] = [0.0, PI * 0.5, PI, PI * 1.5, PI * 0.25, PI * 1.25];
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
            ring_segment_transform(angle, r, h + 0.6).with_scale(Vec3::new(
                width * 1.08,
                0.35,
                depth * 1.1,
            )),
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

    // Continuous glazed front with mullions between the commentary positions,
    // and a brow above it to kill the reflection of the sky.
    let glass_y = media_h * 0.62;
    let glass_h = media_h * 0.30;
    let mut glazing = sg::StandMesh::new();
    let mut frame = sg::StandMesh::new();
    const BAYS: usize = 7;
    const MEDIA_WIDTH: f32 = 20.4;
    let bay_w = MEDIA_WIDTH / BAYS as f32;
    let front = ring_segment_transform(media_angle, media_r - 2.6, glass_y);
    for bay in 0..BAYS {
        let x = (bay as f32 - (BAYS as f32 - 1.0) * 0.5) * bay_w;
        // Tinted broadcast glazing, each bay reflecting a slightly different sky.
        let t = 0.42 + sg::stand_unit(bay as u32, 1, 0x9EDA) * 0.22;
        glazing.push_box(
            front * Transform::from_xyz(x, 0.0, 0.0),
            Vec3::new(bay_w * 0.44, glass_h * 0.5, 0.06),
            [t * 0.66, t * 0.80, t * 0.94, 0.74],
        );
        frame.push_box(
            front * Transform::from_xyz(x + bay_w * 0.5, 0.0, 0.10),
            Vec3::new(0.09, glass_h * 0.55, 0.16),
            [0.22, 0.23, 0.26, 1.0],
        );
    }
    frame.push_ring_box(
        media_angle,
        media_r - 3.1,
        glass_y + glass_h * 0.5 + 0.35,
        Vec3::new(MEDIA_WIDTH + 0.8, 0.30, 1.9),
        [0.18, 0.19, 0.22, 1.0],
    );
    let glazing = glazing.build();
    spawn_merged(
        p,
        ctx.meshes,
        glazing,
        ctx.shared.glass_mat.clone(),
        spawn_count,
    );
    let frame = frame.build();
    spawn_merged(
        p,
        ctx.meshes,
        frame,
        ctx.shared.steel_mat.clone(),
        spawn_count,
    );
}

/// Hoardings, gantries and the player tunnel — the dressing that tells the eye
/// this is a broadcast venue and not an empty bowl.
fn spawn_bowl_detail(
    p: &mut ChildSpawnerCommands,
    ctx: &mut StadiumBuildCtx<'_>,
    spawn_count: &mut usize,
) {
    let bowl = &ctx.bowl;
    let br = ctx.stadium.boundary_radius();

    // Advertising at three heights: a second course above the rope-side boards,
    // the lower bowl's front fascia, and the upper deck's front fascia.
    let hoardings: [(f32, f32, f32, usize); 3] = [
        (br + 1.25, 2.55, 0.90, 3),
        (bowl.inner_radius - 0.35, bowl.base_height + 0.80, 1.25, 2),
        (
            bowl.upper_inner_radius() - 0.35,
            bowl.upper_deck_base_height() - 1.10,
            1.35,
            2,
        ),
    ];
    for (i, &(radius, y, height, every)) in hoardings.iter().enumerate() {
        let backing = sg::hoarding_backing_mesh(TIER_SEGMENTS, radius, y, height, every);
        spawn_merged(
            p,
            ctx.meshes,
            backing,
            ctx.shared.rubber_mat.clone(),
            spawn_count,
        );
        let boards = sg::hoarding_ring_mesh(TIER_SEGMENTS, radius, y, height, every);
        let sponsor = ctx.shared.sponsor_mats[i % ctx.shared.sponsor_mats.len()].clone();
        spawn_merged(p, ctx.meshes, boards, sponsor, spawn_count);
    }

    // Broadcast gantries cantilevered into the gap between the two decks.
    let gantries = sg::camera_gantry_mesh(
        &GANTRY_ANGLES,
        bowl.lower_outer_radius() + 1.2,
        bowl.upper_deck_base_height() + 0.4,
    );
    spawn_merged(
        p,
        ctx.meshes,
        gantries,
        ctx.shared.steel_mat.clone(),
        spawn_count,
    );

    // Covered walk from the pavilion out onto the field, at the bowler's end.
    let tunnel_start = br + 1.5;
    let tunnel = sg::player_tunnel_mesh(PI, tunnel_start, bowl.inner_radius - tunnel_start + 2.5);
    spawn_merged(
        p,
        ctx.meshes,
        tunnel,
        ctx.shared.concrete_mat.clone(),
        spawn_count,
    );
}

fn spawn_floodlights(
    p: &mut ChildSpawnerCommands,
    ctx: &mut StadiumBuildCtx<'_>,
    spawn_count: &mut usize,
) {
    // ---- Floodlight towers integrated with stand perimeter ----
    let outer = ctx.bowl.outer_radius();
    let stand_top = ctx.bowl.stand_top_height();
    let roof = ctx.bowl.roof_spec();
    let tower_r = floodlight_radius(outer);
    let tower_h = stand_top + 22.0;

    // Braced frames tying each pylon back into the roof's rear support line, so
    // the towers read as part of the structure instead of four poles nearby.
    let mut ties = sg::StandMesh::new();
    const STEEL: [f32; 4] = [0.68, 0.69, 0.72, 1.0];
    for angle in floodlight_angles() {
        let foot = ring_position(angle, tower_r, roof.outer_y - 3.0);
        let shoulder = ring_position(angle, tower_r, tower_h * 0.72);
        for off in [-2.4_f32, 2.4] {
            let tangent = ring_tangent(angle);
            let anchor = ring_position(angle, roof.outer_radius, roof.outer_y) + tangent * off;
            ties.push_strut(anchor, foot + tangent * off * 0.4, 0.20, STEEL);
            ties.push_strut(anchor + Vec3::Y * 1.2, shoulder, 0.13, STEEL);
        }
        // Horizontal collar where the frame meets the pylon.
        ties.push_ring_box(
            angle,
            tower_r,
            roof.outer_y - 3.0,
            Vec3::new(5.6, 0.42, 0.42),
            STEEL,
        );
    }
    let ties = ties.build();
    spawn_merged(
        p,
        ctx.meshes,
        ties,
        ctx.shared.steel_mat.clone(),
        spawn_count,
    );

    for (tower_idx, angle) in floodlight_angles().into_iter().enumerate() {
        let base = ring_position(angle, tower_r, 0.0);
        let top = ring_position(angle, tower_r, tower_h);

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

fn spawn_big_screen_and_dugouts(
    p: &mut ChildSpawnerCommands,
    ctx: &mut StadiumBuildCtx<'_>,
    batting_team: &Team,
    fielding_team: &Team,
    spawn_count: &mut usize,
) {
    let br = ctx.stadium.boundary_radius();
    let screen_frame = ctx
        .materials
        .add(painted_steel_mat(Color::srgb_u8(0x10, 0x12, 0x16)));
    let screen_img = ctx.images.add(big_screen_image(
        &batting_team.short.to_uppercase(),
        &fielding_team.short.to_uppercase(),
    ));
    // The panel is a light source, not a printed board: an emissive texture is
    // what makes it read as switched on once the floodlights come up.
    let screen_face = ctx.materials.add(StandardMaterial {
        base_color: Color::srgb(0.16, 0.16, 0.18),
        base_color_texture: Some(screen_img.clone()),
        emissive: LinearRgba::new(2.6, 2.6, 2.6, 1.0),
        emissive_texture: Some(screen_img),
        perceptual_roughness: 0.34,
        reflectance: 0.30,
        ..default()
    });
    let sx = -(br - 2.5);
    let screen_center = Vec3::new(sx - 1.1, 5.6, 0.0);
    let screen_half_w = 8.0;

    // Bezel: a deep surround so the panel sits inside a box rather than floating.
    for (offset, size) in [
        (Vec3::new(0.0, 3.1, 0.0), Vec3::new(0.72, 0.55, 17.4)),
        (Vec3::new(0.0, -3.1, 0.0), Vec3::new(0.72, 0.55, 17.4)),
        (Vec3::new(0.0, 0.0, 8.4), Vec3::new(0.72, 6.7, 0.55)),
        (Vec3::new(0.0, 0.0, -8.4), Vec3::new(0.72, 6.7, 0.55)),
    ] {
        p.spawn((
            Mesh3d(ctx.shared.unit_cuboid.clone()),
            MeshMaterial3d(screen_frame.clone()),
            Transform::from_translation(screen_center + offset).with_scale(size),
        ));
        track_spawn(spawn_count);
    }
    p.spawn((
        Mesh3d(ctx.shared.unit_cuboid.clone()),
        MeshMaterial3d(screen_frame.clone()),
        Transform::from_translation(screen_center + Vec3::X * 0.3)
            .with_scale(Vec3::new(0.30, 6.2, 16.6)),
    ));
    track_spawn(spawn_count);
    p.spawn((
        Mesh3d(ctx.shared.unit_cuboid.clone()),
        MeshMaterial3d(screen_face),
        Transform::from_translation(screen_center + Vec3::X * 0.18)
            .with_scale(Vec3::new(0.20, 5.6, 15.8)),
    ));
    track_spawn(spawn_count);
    let screen_truss = sg::screen_support_mesh(screen_center, screen_half_w, 0.0);
    spawn_merged(
        p,
        ctx.meshes,
        screen_truss,
        ctx.shared.steel_mat.clone(),
        spawn_count,
    );

    let dugout_roof = ctx
        .materials
        .add(painted_steel_mat(Color::srgb_u8(0xE8, 0xE6, 0xDF)));
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
        // Bench seating and kit bags inside the enclosure.
        let fitout =
            sg::dugout_fitout_mesh(Vec3::new(sx + 9.6, 0.0, dz), &ctx.shared.team_tones.clone());
        spawn_merged(
            p,
            ctx.meshes,
            fitout,
            ctx.shared.seat_mat.clone(),
            spawn_count,
        );
    }

    let tent_mats = [
        ctx.materials
            .add(cloth_mat(Color::srgb_u8(0xB8, 0x44, 0x38))),
        ctx.materials
            .add(cloth_mat(Color::srgb_u8(0xDD, 0xD8, 0xCB))),
        ctx.materials
            .add(cloth_mat(Color::srgb_u8(0x2E, 0x4A, 0x62))),
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
    let crease_cross_mesh = meshes.add(
        Plane3d::default()
            .mesh()
            .size(geo::CREASE_DEPTH * 2.0, 0.06),
    );

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
        spawn_floodlights(p, &mut ctx, &mut spawn_count);
        let crowd_count = crowd::spawn_crowd(p, &mut ctx, &mut spawn_count);
        info!("Stadium crowd spawned: {crowd_count} spectators");
        environment::spawn_environment(p, &mut ctx, &mut spawn_count);
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

/// Woven cloth — boundary rope, marquee canvas. Diffuse to the point of having
/// no highlight at all, which is what separates it from painted metal nearby.
fn cloth_mat(color: Color) -> StandardMaterial {
    StandardMaterial {
        base_color: color,
        perceptual_roughness: 0.94,
        metallic: 0.0,
        reflectance: 0.04,
        ..Default::default()
    }
}

/// Matte paint over board or turf: crease lines, sight screens, board frames.
/// Sight screens in particular are painted dead flat on purpose so the ball
/// stays readable against them, so this must not pick up a sheen.
fn matte_paint_mat(color: Color) -> StandardMaterial {
    StandardMaterial {
        base_color: color,
        perceptual_roughness: 0.84,
        metallic: 0.0,
        reflectance: 0.08,
        ..Default::default()
    }
}

/// Lacquered ash — the stumps and bails. The one varnished surface on the
/// ground, and close enough to camera that a tight highlight is worth it.
fn lacquered_wood_mat(color: Color) -> StandardMaterial {
    StandardMaterial {
        base_color: color,
        perceptual_roughness: 0.33,
        metallic: 0.0,
        reflectance: 0.44,
        ..Default::default()
    }
}

/// Cast concrete: matte, barely reflective, with a touch of colour noise from
/// the caller's tint so adjacent surfaces never match exactly.
fn concrete_mat(color: Color) -> StandardMaterial {
    StandardMaterial {
        base_color: color,
        perceptual_roughness: 0.95,
        metallic: 0.0,
        reflectance: 0.15,
        ..Default::default()
    }
}

/// Painted structural steel: tight highlights, genuinely metallic, so the
/// floodlights rake across it instead of flattening it out.
fn painted_steel_mat(color: Color) -> StandardMaterial {
    StandardMaterial {
        base_color: color,
        perceptual_roughness: 0.36,
        metallic: 0.78,
        reflectance: 0.60,
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

    let mut data = Vec::with_capacity((PITCH_TEX_LENGTH_PX * PITCH_TEX_WIDTH_PX * 4) as usize);
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
    let streak =
        pitch_value_noise(u * pitch_u_cycles(0.55), v * pitch_v_cycles(0.30), 3.0, 71) * 0.5 + 0.5;
    // Decimetre-scale soil mottling.
    let mottle = pitch_fbm(u * pitch_u_cycles(0.38), v * pitch_v_cycles(0.50), 29) * 0.5 + 0.5;
    // Fine grit (~9 cm) — visible but safely below Nyquist at this resolution.
    let fine =
        pitch_value_noise(u * pitch_u_cycles(0.09), v * pitch_v_cycles(0.11), 4.0, 17) * 0.5 + 0.5;
    let scuff = pitch_fbm(u * pitch_u_cycles(0.14), v * pitch_v_cycles(0.18), 53) * 0.5 + 0.5;

    // Warm tan/khaki prepared wicket — clearly darker than white crease paint.
    let mut r = 0.46 + roller * 0.10 + fine * 0.06 + mottle * 0.07 + streak * 0.045;
    let mut g = 0.36 + roller * 0.085 + fine * 0.055 + mottle * 0.06 + streak * 0.038;
    let mut b = 0.24 + roller * 0.05 + fine * 0.04 + mottle * 0.045 + streak * 0.028;

    let wear = pitch_wear_mask(u, v);
    r -= wear * 0.22 + scuff * 0.11;
    g -= wear * 0.19 + scuff * 0.10;
    b -= wear * 0.16 + scuff * 0.09;

    [
        r.clamp(0.22, 0.62),
        g.clamp(0.18, 0.54),
        b.clamp(0.12, 0.40),
    ]
}

/// Numbered plaque above an entry gate. Real grounds number every gate, and the
/// digit is the cheapest possible cue that the facade is a public entrance.
fn gate_number_image(number: usize) -> Image {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    /// 3x5 digit cells, one bit per column, most significant bit leftmost.
    const DIGITS: [[u8; 5]; 10] = [
        [0b111, 0b101, 0b101, 0b101, 0b111],
        [0b010, 0b110, 0b010, 0b010, 0b111],
        [0b111, 0b001, 0b111, 0b100, 0b111],
        [0b111, 0b001, 0b111, 0b001, 0b111],
        [0b101, 0b101, 0b111, 0b001, 0b001],
        [0b111, 0b100, 0b111, 0b001, 0b111],
        [0b111, 0b100, 0b111, 0b101, 0b111],
        [0b111, 0b001, 0b001, 0b001, 0b001],
        [0b111, 0b101, 0b111, 0b101, 0b111],
        [0b111, 0b101, 0b111, 0b001, 0b111],
    ];
    const W: u32 = 64;
    const H: u32 = 64;
    const CELL: u32 = 9;

    let digits: Vec<usize> = if number >= 10 {
        vec![(number / 10) % 10, number % 10]
    } else {
        vec![number % 10]
    };
    // Signage green on a dark plate, as used for stadium wayfinding.
    let mut data = Vec::with_capacity((W * H * 4) as usize);
    for _ in 0..W * H {
        data.extend_from_slice(&[0x0C, 0x14, 0x10, 0xFF]);
    }
    let glyph_w = 3 * CELL;
    let total_w = glyph_w * digits.len() as u32 + CELL * (digits.len() as u32 - 1);
    let ox = (W - total_w) / 2;
    let oy = (H - 5 * CELL) / 2;
    for (i, &d) in digits.iter().enumerate() {
        let gx = ox + i as u32 * (glyph_w + CELL);
        for (row, &bits) in DIGITS[d].iter().enumerate() {
            for col in 0..3u32 {
                if bits & (1 << (2 - col)) == 0 {
                    continue;
                }
                for py in 0..CELL {
                    for px in 0..CELL {
                        let x = gx + col * CELL + px;
                        let y = oy + row as u32 * CELL + py;
                        if x >= W || y >= H {
                            continue;
                        }
                        let idx = ((y * W + x) * 4) as usize;
                        data[idx] = 0xE8;
                        data[idx + 1] = 0xF0;
                        data[idx + 2] = 0xDC;
                    }
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

    /// The camera rig and the crowd module both derive seat positions from these,
    /// so the detail pass must not have shifted them.
    #[test]
    fn published_bowl_dimensions_are_unchanged() {
        let bowl = BowlLayout::from_boundary(65.0);
        assert!((bowl.inner_radius - 69.8).abs() < 1e-4);
        assert!((bowl.lower_outer_radius() - 85.55).abs() < 1e-3);
        assert!((bowl.upper_inner_radius() - 89.75).abs() < 1e-3);
        assert!((bowl.outer_radius() - 101.0).abs() < 1e-3);
        assert!((bowl.tier_height(0) - 0.5).abs() < 1e-4);
        assert!((bowl.stand_top_height() - 16.17).abs() < 1e-3);
    }

    #[test]
    fn tread_top_matches_the_surface_the_crowd_sits_on() {
        let bowl = BowlLayout::from_boundary(65.0);
        for tier in 0..TIER_COUNT {
            // `crowd::spawn_crowd` uses `tier_height + tread_thickness`.
            let expected = bowl.tier_height(tier) + bowl.tread_thickness;
            assert!(
                (bowl.tread_top(tier) - expected).abs() < 1e-5,
                "tier {tier}"
            );
            // Each tread's inner radius must sit half a tread depth inside its mid.
            let inner = bowl.tier_inner_radius(tier);
            assert!((bowl.tier_mid_radius(tier) - inner - bowl.tier_depth * 0.5).abs() < 1e-4);
        }
    }

    #[test]
    fn every_tier_carries_a_full_row_of_seats() {
        let bowl = BowlLayout::from_boundary(65.0);
        let mut total = 0usize;
        for tier in 0..TIER_COUNT {
            let band = sg::SeatBand {
                segments: TIER_SEGMENTS,
                aisle_every: AISLE_EVERY,
                radius: bowl.tier_mid_radius(tier),
                tread_top: bowl.tread_top(tier),
                row: tier,
                mosaic: bowl.is_upper_deck(tier),
            };
            let seats = sg::seat_band_count(&band);
            assert!(seats > 600, "tier {tier} only holds {seats} seats");
            total += seats;
        }
        // A real ground of this size seats tens of thousands; the merged-mesh
        // approach is the only way to draw them, so guard the order of magnitude.
        assert!(
            (9_000..20_000).contains(&total),
            "bowl capacity {total} out of range"
        );
    }

    #[test]
    fn upper_deck_alone_carries_the_seat_mosaic() {
        let bowl = BowlLayout::from_boundary(65.0);
        for tier in 0..TIER_COUNT {
            assert_eq!(bowl.is_upper_deck(tier), tier >= LOWER_TIER_COUNT);
        }
        assert!(!bowl.is_upper_deck(LOWER_TIER_COUNT - 1));
        assert!(bowl.is_upper_deck(TIER_COUNT - 1));
    }

    #[test]
    fn vomitories_land_on_aisles_and_inside_the_bowl() {
        let bowl = BowlLayout::from_boundary(65.0);
        let aisles = sg::aisle_angles(TIER_SEGMENTS, AISLE_EVERY);
        let voms = sg::vomitory_angles(TIER_SEGMENTS, AISLE_EVERY, VOMITORY_EVERY_NTH_AISLE);
        assert_eq!(aisles.len(), TIER_SEGMENTS / AISLE_EVERY);
        assert_eq!(voms.len(), aisles.len() / VOMITORY_EVERY_NTH_AISLE);
        for angle in &voms {
            assert!(aisles.iter().any(|a| (a - angle).abs() < 1e-5));
        }
        // Each pierced tier must leave the bore fully inside the seating deck.
        for &tier in &VOMITORY_TIERS {
            let mouth = bowl.tier_inner_radius(tier);
            let back = mouth + bowl.tier_depth * 1.9;
            assert!(mouth >= bowl.inner_radius, "tier {tier} mouth {mouth}");
            assert!(
                back < bowl.outer_radius(),
                "tier {tier} bore exits at {back}"
            );
            assert!(
                bowl.tread_top(tier) > 0.0,
                "tier {tier} floor is below the apron"
            );
        }
    }

    #[test]
    fn roof_covers_the_upper_deck_and_clears_the_back_row() {
        let bowl = BowlLayout::from_boundary(65.0);
        let roof = bowl.roof_spec();
        assert_eq!(roof.truss_count, ROOF_TRUSS_COUNT);
        assert_eq!(
            sg::roof_truss_angles(roof.truss_count).len(),
            ROOF_TRUSS_COUNT
        );
        // Cantilever reaches in over the upper deck without touching the lower bowl.
        assert!(roof.inner_radius > bowl.upper_inner_radius());
        assert!(roof.inner_radius < bowl.outer_radius());
        assert!(roof.outer_radius > bowl.outer_radius());
        // Headroom over the back row, and the rear support above the tip.
        assert!(roof.inner_y > bowl.stand_top_height() + 4.0);
        assert!(roof.outer_y > roof.inner_y);
        // Towers still rise clear of the roof so the lamp banks light the field.
        let tower_top = bowl.stand_top_height() + 22.0;
        assert!(tower_top > roof.outer_y + 8.0, "towers buried in the roof");
        assert!(floodlight_radius(bowl.outer_radius()) > roof.outer_radius);
    }

    /// The detail pass is only affordable because it is merged. Ten thousand
    /// seats as entities would cost more than the whole rest of the frame, so
    /// guard both halves of the bargain: the geometry arrives in a couple of
    /// dozen draws, and the vertex total stays inside a sane budget.
    #[test]
    fn merged_bowl_geometry_stays_inside_its_budget() {
        fn vertices(mesh: &Mesh) -> usize {
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
                .map(|p| p.len())
                .unwrap_or(0)
        }

        let bowl = BowlLayout::from_boundary(65.0);
        let palette = sg::SeatPalette {
            tones: [[0.3, 0.4, 0.5], [0.32, 0.42, 0.52], [0.28, 0.38, 0.48]],
            accent: [0.9, 0.9, 0.85],
        };

        let mut parts: Vec<Mesh> = Vec::new();
        let mut seats = 0usize;
        for tier in 0..TIER_COUNT {
            let band = sg::SeatBand {
                segments: TIER_SEGMENTS,
                aisle_every: AISLE_EVERY,
                radius: bowl.tier_mid_radius(tier),
                tread_top: bowl.tread_top(tier),
                row: tier,
                mosaic: bowl.is_upper_deck(tier),
            };
            seats += sg::seat_band_count(&band);
            parts.push(sg::seat_band_mesh(&band, &palette));
        }
        let seat_verts: usize = parts.iter().map(vertices).sum();

        let roof = bowl.roof_spec();
        parts.push(sg::roof_truss_mesh(&roof));
        parts.push(sg::roof_panel_mesh(&roof, false));
        parts.push(sg::roof_panel_mesh(&roof, true));
        parts.push(sg::roof_soffit_mesh(&roof));
        parts.push(sg::roof_edge_mesh(&roof));
        parts.push(sg::roof_cluster_mesh(&roof, ROOF_CLUSTER_COUNT));
        let facade = sg::FacadeSpec {
            segments: FACADE_SEGMENTS,
            radius: bowl.outer_radius() + 3.9,
            base_y: bowl.upper_deck_base_height(),
            height: UPPER_TIER_COUNT as f32 * bowl.tier_rise + 4.5,
            rib_width: 1.15,
            rib_depth: 0.85,
            glazing_bands: 6,
        };
        parts.push(sg::facade_rib_mesh(&facade));
        parts.push(sg::facade_glazing_mesh(&facade));
        parts.push(sg::facade_parapet_mesh(&facade));

        // One draw per tier of seats plus one per roof and facade layer. If this
        // ever climbs into the hundreds, something stopped merging.
        assert_eq!(parts.len(), TIER_COUNT + 9);
        // Pan and backrest, five faces each, four vertices per face.
        assert_eq!(seat_verts, seats * 40);
        let total: usize = parts.iter().map(vertices).sum();
        assert!(
            total < 600_000,
            "merged bowl geometry blew its vertex budget: {total}"
        );
        for mesh in &parts {
            assert!(
                !sg::mesh_is_empty(mesh),
                "a merged bowl layer came back empty"
            );
        }
    }

    /// The bowl used to be one roughness value everywhere, which is what made it
    /// read as plastic under the floodlights. Each finish must stay a distinct
    /// point in roughness/metallic/reflectance space.
    #[test]
    fn surface_finishes_are_physically_distinct() {
        let finishes = [
            ("concrete", concrete_mat(Color::WHITE)),
            ("steel", painted_steel_mat(Color::WHITE)),
            ("cloth", cloth_mat(Color::WHITE)),
            ("paint", matte_paint_mat(Color::WHITE)),
            ("wood", lacquered_wood_mat(Color::WHITE)),
        ];
        for (i, (name, a)) in finishes.iter().enumerate() {
            for (other, b) in finishes.iter().skip(i + 1) {
                let spread = (a.perceptual_roughness - b.perceptual_roughness).abs()
                    + (a.metallic - b.metallic).abs()
                    + (a.reflectance - b.reflectance).abs();
                assert!(
                    spread > 0.08,
                    "{name} and {other} are the same finish (spread {spread})"
                );
            }
        }
        // Only the steel is genuinely metallic; everything else is a dielectric.
        assert!(painted_steel_mat(Color::WHITE).metallic > 0.5);
        for (name, m) in &finishes[2..] {
            assert_eq!(m.metallic, 0.0, "{name} must not be metallic");
        }
    }

    #[test]
    fn gate_plaque_numbering_covers_every_gate() {
        let gates = sg::gate_angles(GATE_COUNT);
        assert_eq!(gates.len(), GATE_COUNT);
        // One plaque image per gate, and gate 1 must differ from gate 2.
        let a = gate_number_image(1);
        let b = gate_number_image(2);
        assert_ne!(a.data.as_ref().unwrap(), b.data.as_ref().unwrap());
        assert_eq!(
            gate_number_image(1).data.as_ref().unwrap(),
            a.data.as_ref().unwrap(),
            "gate plaques must be deterministic"
        );
    }
}

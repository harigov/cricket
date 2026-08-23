//! Stadium crowd population.
//!
//! The crowd uses two render tiers:
//! - near-camera lower tiers use real glTF figures for readable silhouette detail
//! - distant tiers are merged into a tiny number of vertex-coloured meshes
//!
//! This keeps the bowl looking full without exploding entity count.

use std::f32::consts::TAU;

use bevy::asset::RenderAssetUsages;
use bevy::gltf::GltfAssetLabel;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::core::geometry;
use crate::render::ring_geometry::{ring_face_center_rotation, ring_position, ring_tangent};
use crate::render::stadium::{
    BowlLayout, LOWER_TIER_COUNT, StadiumBuildCtx, TIER_COUNT, track_spawn,
};

const CROWD_SEGMENTS: usize = 240;
const CROWD_AISLE_EVERY: usize = 10;
const CROWD_VOMITORY_EVERY_LOWER: usize = 18;
const CROWD_VOMITORY_EVERY_UPPER: usize = 16;
const CROWD_SEAT_TARGET_WIDTH: f32 = 0.26;
const DETAILED_CROWD_SCALE: f32 = 0.62;
const DETAILED_POSE_BASE_PITCH: f32 = -0.26;
const DETAILED_STANDING_LIFT: f32 = 0.42;

const CROWD_VARIANTS: [&str; 14] = [
    "crowd/crowd-a.glb",
    "crowd/crowd-b.glb",
    "crowd/crowd-c.glb",
    "crowd/crowd-d.glb",
    "crowd/crowd-e.glb",
    "crowd/crowd-f.glb",
    "crowd/crowd-g.glb",
    "crowd/crowd-h.glb",
    "crowd/crowd-i.glb",
    "crowd/crowd-j.glb",
    "crowd/crowd-k.glb",
    "crowd/crowd-l.glb",
    "crowd/crowd-m.glb",
    "crowd/crowd-n.glb",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CrowdBand {
    Detailed,
    LowerMerged,
    UpperMerged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SupportSection {
    Batting,
    Fielding,
    Neutral,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CrowdSeatPlan {
    tier: u8,
    seg: u16,
    seat: u8,
    seats_in_segment: u8,
    band: CrowdBand,
    section: SupportSection,
    standing: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CrowdLayout {
    seats: Vec<CrowdSeatPlan>,
    tier_capacity: [usize; TIER_COUNT],
    tier_filled: [usize; TIER_COUNT],
}

impl CrowdLayout {
    fn total_count(&self) -> usize {
        self.seats.len()
    }

    fn detailed_count(&self) -> usize {
        self.seats
            .iter()
            .filter(|seat| seat.band == CrowdBand::Detailed)
            .count()
    }

    fn occupancy_for_tier(&self, tier: usize) -> f32 {
        let cap = self.tier_capacity[tier];
        if cap == 0 {
            0.0
        } else {
            self.tier_filled[tier] as f32 / cap as f32
        }
    }
}

fn crowd_hash_u32(a: u32, b: u32, c: u32, seed: u32) -> u32 {
    let mut n = a
        .wrapping_mul(374_761_393)
        .wrapping_add(b.wrapping_mul(668_265_263))
        .wrapping_add(c.wrapping_mul(2_147_483_647))
        .wrapping_add(seed.wrapping_mul(982_451_653));
    n = (n ^ (n >> 13)).wrapping_mul(1_274_126_177);
    n ^ (n >> 16)
}

fn crowd_hash(a: u32, b: u32, c: u32, seed: u32) -> f32 {
    (crowd_hash_u32(a, b, c, seed) & 0x00FF_FFFF) as f32 / 16_777_215.0
}

fn crowd_segment_skipped(seg: usize) -> bool {
    seg.is_multiple_of(CROWD_AISLE_EVERY)
}

fn crowd_vomitory_segment_skipped(seg: usize, tier: usize) -> bool {
    if tier < LOWER_TIER_COUNT {
        (seg + 4).is_multiple_of(CROWD_VOMITORY_EVERY_LOWER)
    } else {
        let upper_mod = (seg + 7) % CROWD_VOMITORY_EVERY_UPPER;
        upper_mod == 0 || upper_mod == 1
    }
}

fn crowd_tier_phase(tier: usize) -> f32 {
    ((tier * 19 + 7) % CROWD_SEGMENTS) as f32 / CROWD_SEGMENTS as f32 * TAU
}

fn crowd_segment_mid(seg: usize, tier: usize) -> f32 {
    let base = (seg as f32 + 0.5) / CROWD_SEGMENTS as f32 * TAU;
    let jitter =
        (crowd_hash(seg as u32, tier as u32, 0, 97) - 0.5) * TAU / CROWD_SEGMENTS as f32 * 0.32;
    base + crowd_tier_phase(tier) + jitter
}

fn crowd_seats_per_segment(bowl: &BowlLayout, tier: usize) -> usize {
    let arc_len = TAU * (bowl.tier_mid_radius(tier) - 0.2) / CROWD_SEGMENTS as f32;
    (arc_len / CROWD_SEAT_TARGET_WIDTH).floor().clamp(4.0, 10.0) as usize
}

fn crowd_support_section(seg: usize) -> SupportSection {
    const SUPPORT_BLOCK_SEGMENTS: usize = 12;
    const SUPPORT_PATTERN: [SupportSection; 10] = [
        SupportSection::Batting,
        SupportSection::Batting,
        SupportSection::Neutral,
        SupportSection::Fielding,
        SupportSection::Fielding,
        SupportSection::Neutral,
        SupportSection::Fielding,
        SupportSection::Neutral,
        SupportSection::Batting,
        SupportSection::Neutral,
    ];
    let block = ((seg + 5) / SUPPORT_BLOCK_SEGMENTS) % SUPPORT_PATTERN.len();
    SUPPORT_PATTERN[block]
}

fn crowd_band_for_seat(tier: usize, angle: f32) -> CrowdBand {
    if tier >= LOWER_TIER_COUNT {
        return CrowdBand::UpperMerged;
    }

    let detailed_threshold = match tier {
        0 => Some(0.55),
        1 => Some(0.62),
        2 => Some(0.72),
        3 => Some(0.80),
        _ => None,
    };
    if detailed_threshold.is_some_and(|th| angle.cos().abs() >= th) {
        CrowdBand::Detailed
    } else {
        CrowdBand::LowerMerged
    }
}

fn crowd_target_occupancy(tier: usize, seg: usize, angle: f32, section: SupportSection) -> f32 {
    let tier_t = tier as f32 / (TIER_COUNT as f32 - 1.0);
    let mut occ = 0.96 - tier_t * 0.24;
    if tier >= LOWER_TIER_COUNT {
        occ -= 0.08;
    }

    let axis_focus = angle.cos().abs();
    occ += axis_focus.powf(1.4) * 0.09;
    occ -= (1.0 - axis_focus) * if tier >= LOWER_TIER_COUNT { 0.22 } else { 0.12 };

    occ += match section {
        SupportSection::Batting => 0.04,
        SupportSection::Fielding => 0.02,
        SupportSection::Neutral => -0.06,
    };

    occ += (crowd_hash(seg as u32, tier as u32, 0, 211) - 0.5) * 0.10;
    occ.clamp(0.28, 0.98)
}

fn crowd_standing_rate(tier: usize, section: SupportSection, angle: f32) -> f32 {
    let mut rate = if tier < 2 {
        0.17
    } else if tier < LOWER_TIER_COUNT {
        0.11
    } else {
        0.07
    };
    rate += angle.cos().abs() * 0.04;
    if !matches!(section, SupportSection::Neutral) {
        rate += 0.03;
    }
    rate.clamp(0.04, 0.30)
}

fn build_crowd_layout(bowl: &BowlLayout) -> CrowdLayout {
    let mut seats = Vec::with_capacity(16_000);
    let mut tier_capacity = [0usize; TIER_COUNT];
    let mut tier_filled = [0usize; TIER_COUNT];

    for tier in 0..TIER_COUNT {
        let seats_per_segment = crowd_seats_per_segment(bowl, tier);
        for seg in 0..CROWD_SEGMENTS {
            if crowd_segment_skipped(seg) || crowd_vomitory_segment_skipped(seg, tier) {
                continue;
            }

            tier_capacity[tier] += seats_per_segment;
            let section = crowd_support_section(seg);
            let mid = crowd_segment_mid(seg, tier);
            let occ = crowd_target_occupancy(tier, seg, mid, section);
            let standing_rate = crowd_standing_rate(tier, section, mid);

            for seat in 0..seats_per_segment {
                if crowd_hash(seg as u32, tier as u32, seat as u32, 353) > occ {
                    continue;
                }

                let standing =
                    crowd_hash(seg as u32, tier as u32, seat as u32, 907) < standing_rate;
                let band = crowd_band_for_seat(tier, mid);
                seats.push(CrowdSeatPlan {
                    tier: tier as u8,
                    seg: seg as u16,
                    seat: seat as u8,
                    seats_in_segment: seats_per_segment as u8,
                    band,
                    section,
                    standing,
                });
                tier_filled[tier] += 1;
            }
        }
    }

    CrowdLayout {
        seats,
        tier_capacity,
        tier_filled,
    }
}

fn crowd_seat_pose(bowl: &BowlLayout, seat: CrowdSeatPlan) -> (Vec3, f32, f32, f32, f32) {
    let tier = seat.tier as usize;
    let seg = seat.seg as usize;
    let seat_idx = seat.seat as usize;
    let seats_in_segment = seat.seats_in_segment as usize;

    let mid = crowd_segment_mid(seg, tier);
    let base_r = bowl.tier_mid_radius(tier) - 0.12;
    let seat_r = base_r + (crowd_hash(seg as u32, tier as u32, seat_idx as u32, 733) - 0.5) * 0.08;
    let pitch_arc = TAU * seat_r / CROWD_SEGMENTS as f32;
    let seat_pitch = (pitch_arc / seats_in_segment as f32).clamp(0.34, 0.72);
    let center = (seats_in_segment as f32 - 1.0) * 0.5;
    let tangent_jitter =
        (crowd_hash(seg as u32, tier as u32, seat_idx as u32, 503) - 0.5) * seat_pitch * 0.24;
    let tangent_offset = (seat_idx as f32 - center) * seat_pitch + tangent_jitter;

    let base_h = bowl.tier_height(tier) + bowl.tread_thickness - 0.07;
    let seat_h = base_h
        + if seat.standing {
            DETAILED_STANDING_LIFT
        } else {
            0.0
        };
    let pos = ring_position(mid, seat_r, seat_h) + ring_tangent(mid) * tangent_offset;

    let lean = (crowd_hash(seg as u32, tier as u32, seat_idx as u32, 1289) - 0.5) * 0.22;
    let shoulder = (crowd_hash(seg as u32, tier as u32, seat_idx as u32, 1499) - 0.5) * 0.30;
    let pitch = DETAILED_POSE_BASE_PITCH
        + if seat.standing { 0.16 } else { 0.0 }
        + (crowd_hash(seg as u32, tier as u32, seat_idx as u32, 2089) - 0.5) * 0.11;
    let scale = 0.90 + crowd_hash(seg as u32, tier as u32, seat_idx as u32, 3067) * 0.20;

    (pos, mid, lean, shoulder, pitch * scale)
}

fn crowd_variant_index(seat: CrowdSeatPlan) -> usize {
    crowd_hash_u32(seat.seg as u32, seat.tier as u32, seat.seat as u32, 1811) as usize
        % CROWD_VARIANTS.len()
}

fn crowd_scale_factor(seat: CrowdSeatPlan) -> f32 {
    let base = 0.91 + crowd_hash(seat.seg as u32, seat.tier as u32, seat.seat as u32, 3079) * 0.18;
    if seat.standing { base * 1.05 } else { base }
}

fn crowd_fallback_team_colors(ctx: &StadiumBuildCtx<'_>) -> (Color, Color) {
    let stand = ctx.stadium.stand_color.to_srgba();
    let outfield = ctx.outfield_base.to_srgba();

    // Synthesise two clearly-separated fan blocks from stadium palette cues.
    let batting = Color::srgb(
        (stand.red * 0.45 + 0.55).clamp(0.0, 1.0),
        (stand.green * 0.20 + 0.12).clamp(0.0, 1.0),
        (stand.blue * 0.15 + 0.10).clamp(0.0, 1.0),
    );
    let fielding = Color::srgb(
        (outfield.red * 0.28 + 0.11).clamp(0.0, 1.0),
        (outfield.green * 0.52 + 0.20).clamp(0.0, 1.0),
        (outfield.blue * 0.34 + 0.48).clamp(0.0, 1.0),
    );
    (batting, fielding)
}

fn crowd_shirt_color(
    seat: CrowdSeatPlan,
    batting_color: Color,
    fielding_color: Color,
    neutral_idx: usize,
) -> Color {
    const NEUTRALS: [Color; 3] = [
        Color::srgb_u8(0xCF, 0xD2, 0xDA),
        Color::srgb_u8(0x9F, 0xA5, 0xB2),
        Color::srgb_u8(0x73, 0x7A, 0x89),
    ];
    let n = NEUTRALS[neutral_idx % NEUTRALS.len()];
    match seat.section {
        SupportSection::Batting => {
            let v = crowd_hash(seat.seg as u32, seat.tier as u32, seat.seat as u32, 4153);
            if v < 0.78 {
                batting_color
            } else if v < 0.92 {
                lerp_color(batting_color, n, 0.35)
            } else {
                n
            }
        }
        SupportSection::Fielding => {
            let v = crowd_hash(seat.seg as u32, seat.tier as u32, seat.seat as u32, 4931);
            if v < 0.78 {
                fielding_color
            } else if v < 0.92 {
                lerp_color(fielding_color, n, 0.35)
            } else {
                n
            }
        }
        SupportSection::Neutral => {
            let v = crowd_hash(seat.seg as u32, seat.tier as u32, seat.seat as u32, 6763);
            if v < 0.55 {
                n
            } else if v < 0.75 {
                lerp_color(batting_color, n, 0.55)
            } else {
                lerp_color(fielding_color, n, 0.55)
            }
        }
    }
}

fn crowd_skin_color(idx: usize) -> Color {
    const SKIN_TONES: [Color; 6] = [
        Color::srgb_u8(0xF2, 0xD0, 0xB4),
        Color::srgb_u8(0xDF, 0xB2, 0x8E),
        Color::srgb_u8(0xBF, 0x8A, 0x66),
        Color::srgb_u8(0x9A, 0x67, 0x45),
        Color::srgb_u8(0x76, 0x4E, 0x37),
        Color::srgb_u8(0x58, 0x39, 0x28),
    ];
    SKIN_TONES[idx % SKIN_TONES.len()]
}

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    let sa = a.to_srgba();
    let sb = b.to_srgba();
    let k = t.clamp(0.0, 1.0);
    Color::srgb(
        sa.red + (sb.red - sa.red) * k,
        sa.green + (sb.green - sa.green) * k,
        sa.blue + (sb.blue - sa.blue) * k,
    )
}

fn crowd_variant_handles(ctx: &StadiumBuildCtx<'_>) -> Vec<Handle<Scene>> {
    CROWD_VARIANTS
        .iter()
        .map(|path| {
            ctx.asset_server
                .load(GltfAssetLabel::Scene(0).from_asset(*path))
        })
        .collect()
}

fn spawn_detailed_crowd(
    p: &mut ChildSpawnerCommands,
    ctx: &StadiumBuildCtx<'_>,
    layout: &CrowdLayout,
    crowd_variants: &[Handle<Scene>],
    spawn_count: &mut usize,
) {
    for seat in layout
        .seats
        .iter()
        .copied()
        .filter(|seat| seat.band == CrowdBand::Detailed)
    {
        let (pos, mid, lean, shoulder, pitch) = crowd_seat_pose(&ctx.bowl, seat);
        let rot = ring_face_center_rotation(mid)
            * Quat::from_rotation_y(shoulder)
            * Quat::from_rotation_z(lean)
            * Quat::from_rotation_x(pitch);
        let variant = crowd_variants[crowd_variant_index(seat)].clone();
        let scale = crowd_scale_factor(seat) * DETAILED_CROWD_SCALE;
        p.spawn((
            SceneRoot(variant),
            Transform::from_translation(pos)
                .with_rotation(rot)
                .with_scale(Vec3::splat(scale)),
            Visibility::default(),
            InheritedVisibility::default(),
            ViewVisibility::default(),
        ));
        track_spawn(spawn_count);
    }
}

fn spawn_merged_crowd_band(
    p: &mut ChildSpawnerCommands,
    ctx: &StadiumBuildCtx<'_>,
    layout: &CrowdLayout,
    band: CrowdBand,
    batting_color: Color,
    fielding_color: Color,
    spawn_count: &mut usize,
) {
    let Some(mesh) = build_merged_crowd_mesh(
        &ctx.bowl,
        &layout.seats,
        band,
        batting_color,
        fielding_color,
    ) else {
        return;
    };

    let (meshes, materials) = crowd_asset_stores_mut(ctx);
    let merged_mesh = meshes.add(mesh);
    let merged_mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.96,
        reflectance: 0.08,
        cull_mode: None,
        ..default()
    });
    p.spawn((
        Mesh3d(merged_mesh),
        MeshMaterial3d(merged_mat),
        Transform::default(),
    ));
    track_spawn(spawn_count);
}

fn crowd_asset_stores_mut<'a>(
    ctx: &'a StadiumBuildCtx<'a>,
) -> (&'a mut Assets<Mesh>, &'a mut Assets<StandardMaterial>) {
    // SAFETY: `StadiumBuildCtx` is built with unique `&mut Assets` references for
    // one stadium build pass. Crowd spawning runs synchronously inside that pass,
    // so there is no concurrent aliasing of either store.
    unsafe {
        let meshes = &mut *(ctx.meshes as *const Assets<Mesh> as *mut Assets<Mesh>);
        let materials = &mut *(ctx.materials as *const Assets<StandardMaterial>
            as *mut Assets<StandardMaterial>);
        (meshes, materials)
    }
}

fn build_merged_crowd_mesh(
    bowl: &BowlLayout,
    seats: &[CrowdSeatPlan],
    band: CrowdBand,
    batting_color: Color,
    fielding_color: Color,
) -> Option<Mesh> {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();

    for seat in seats.iter().copied().filter(|seat| seat.band == band) {
        let (pos, mid, lean, shoulder, pitch) = crowd_seat_pose(bowl, seat);
        let rot = ring_face_center_rotation(mid)
            * Quat::from_rotation_y(shoulder * 0.7)
            * Quat::from_rotation_z(lean * 0.85)
            * Quat::from_rotation_x(pitch * 0.55);
        let scale = crowd_scale_factor(seat) * if seat.standing { 1.08 } else { 0.92 };
        let body_h = if seat.standing { 0.96 } else { 0.66 };
        let body_half = Vec3::new(0.17, body_h * 0.5, 0.12) * scale;
        let head_half = Vec3::new(0.10, 0.11, 0.10) * scale;

        let neutral_idx =
            crowd_hash_u32(seat.seg as u32, seat.tier as u32, seat.seat as u32, 8089) as usize;
        let shirt = crowd_shirt_color(seat, batting_color, fielding_color, neutral_idx);
        let skin = crowd_skin_color(crowd_hash_u32(
            seat.seg as u32,
            seat.tier as u32,
            seat.seat as u32,
            1129,
        ) as usize);

        let body_tf =
            Transform::from_translation(pos + Vec3::Y * (body_half.y + 0.02)).with_rotation(rot);
        let head_tf =
            Transform::from_translation(pos + Vec3::Y * (body_h * scale + head_half.y + 0.04))
                .with_rotation(rot);

        append_colored_box(
            &mut positions,
            &mut normals,
            &mut uvs,
            &mut colors,
            &mut indices,
            body_tf,
            body_half,
            color_to_vec4(shirt),
        );
        append_colored_box(
            &mut positions,
            &mut normals,
            &mut uvs,
            &mut colors,
            &mut indices,
            head_tf,
            head_half,
            color_to_vec4(skin),
        );
    }

    if positions.is_empty() {
        return None;
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    Some(mesh)
}

fn color_to_vec4(color: Color) -> [f32; 4] {
    let c = color.to_srgba();
    [c.red, c.green, c.blue, c.alpha]
}

fn append_colored_box(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    uvs: &mut Vec<[f32; 2]>,
    colors: &mut Vec<[f32; 4]>,
    indices: &mut Vec<u32>,
    transform: Transform,
    half: Vec3,
    color: [f32; 4],
) {
    const FACE_CORNERS: [[(f32, f32, f32); 4]; 6] = [
        [(1., 1., -1.), (-1., 1., -1.), (-1., 1., 1.), (1., 1., 1.)],
        [
            (1., -1., 1.),
            (-1., -1., 1.),
            (-1., -1., -1.),
            (1., -1., -1.),
        ],
        [(1., 1., 1.), (-1., 1., 1.), (-1., -1., 1.), (1., -1., 1.)],
        [
            (-1., 1., -1.),
            (1., 1., -1.),
            (1., -1., -1.),
            (-1., -1., -1.),
        ],
        [
            (-1., 1., 1.),
            (-1., 1., -1.),
            (-1., -1., -1.),
            (-1., -1., 1.),
        ],
        [(1., 1., -1.), (1., 1., 1.), (1., -1., 1.), (1., -1., -1.)],
    ];
    const FACE_NORMALS: [Vec3; 6] = [
        Vec3::Y,
        Vec3::NEG_Y,
        Vec3::Z,
        Vec3::NEG_Z,
        Vec3::NEG_X,
        Vec3::X,
    ];

    let base = positions.len() as u32;
    for (face, corners) in FACE_CORNERS.iter().enumerate() {
        let n = (transform.rotation * FACE_NORMALS[face])
            .normalize()
            .to_array();
        for (i, &(lx, ly, lz)) in corners.iter().enumerate() {
            let local = Vec3::new(lx * half.x, ly * half.y, lz * half.z);
            positions.push(transform.transform_point(local).to_array());
            normals.push(n);
            uvs.push([i as f32 % 2.0, (i as f32 * 0.5).fract()]);
            colors.push(color);
        }
        let f = base + face as u32 * 4;
        indices.extend_from_slice(&[f, f + 1, f + 2, f, f + 2, f + 3]);
    }
}

pub(crate) fn spawn_crowd(
    p: &mut ChildSpawnerCommands,
    ctx: &StadiumBuildCtx<'_>,
    spawn_count: &mut usize,
) -> usize {
    let (batting_color, fielding_color) = crowd_fallback_team_colors(ctx);
    spawn_crowd_with_team_colors(p, ctx, spawn_count, batting_color, fielding_color)
}

/// Alternate entry point so stadium assembly can pass real team colours later
/// without changing crowd layout maths.
pub(crate) fn spawn_crowd_with_team_colors(
    p: &mut ChildSpawnerCommands,
    ctx: &StadiumBuildCtx<'_>,
    spawn_count: &mut usize,
    batting_color: Color,
    fielding_color: Color,
) -> usize {
    let layout = build_crowd_layout(&ctx.bowl);
    let crowd_variants = crowd_variant_handles(ctx);

    spawn_detailed_crowd(p, ctx, &layout, &crowd_variants, spawn_count);
    spawn_merged_crowd_band(
        p,
        ctx,
        &layout,
        CrowdBand::LowerMerged,
        batting_color,
        fielding_color,
        spawn_count,
    );
    spawn_merged_crowd_band(
        p,
        ctx,
        &layout,
        CrowdBand::UpperMerged,
        batting_color,
        fielding_color,
        spawn_count,
    );

    layout.total_count()
}

/// Expected crowd count for a standard bowl (used by tests and capacity checks).
pub fn expected_crowd_count() -> usize {
    let bowl = BowlLayout::from_boundary(geometry::DEFAULT_BOUNDARY_RADIUS);
    build_crowd_layout(&bowl).total_count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn standard_layout() -> CrowdLayout {
        let bowl = BowlLayout::from_boundary(geometry::DEFAULT_BOUNDARY_RADIUS);
        build_crowd_layout(&bowl)
    }

    #[test]
    fn crowd_count_in_target_range() {
        let n = expected_crowd_count();
        assert!(n >= 12_000, "crowd too sparse: {n}");
        assert!(n <= 22_000, "crowd too dense: {n}");
    }

    #[test]
    fn detailed_band_stays_within_budget() {
        let layout = standard_layout();
        let n = layout.detailed_count();
        assert!(n >= 1_500, "detailed crowd too sparse: {n}");
        assert!(n <= 3_000, "detailed crowd too dense: {n}");
    }

    #[test]
    fn aisle_and_vomitory_segments_are_empty() {
        let layout = standard_layout();
        let mut occ = [[0usize; CROWD_SEGMENTS]; TIER_COUNT];
        for seat in &layout.seats {
            occ[seat.tier as usize][seat.seg as usize] += 1;
        }

        for (tier, row) in occ.iter().enumerate().take(TIER_COUNT) {
            for (seg, seat_count) in row.iter().enumerate().take(CROWD_SEGMENTS) {
                if crowd_segment_skipped(seg) || crowd_vomitory_segment_skipped(seg, tier) {
                    assert_eq!(
                        *seat_count, 0,
                        "gap segment filled: tier={tier}, seg={seg}, count={seat_count}"
                    );
                }
            }
        }
    }

    #[test]
    fn all_tiers_receive_crowd() {
        let layout = standard_layout();
        for tier in 0..TIER_COUNT {
            assert!(
                layout.tier_filled[tier] > 220,
                "tier {tier} too empty: {}",
                layout.tier_filled[tier]
            );
        }
    }

    #[test]
    fn occupancy_profile_is_plausible() {
        let layout = standard_layout();

        let mut lower_sum = 0.0;
        let mut upper_sum = 0.0;
        for tier in 0..TIER_COUNT {
            let occ = layout.occupancy_for_tier(tier);
            assert!(occ >= 0.28, "tier {tier} occupancy too low: {occ:.3}");
            assert!(occ <= 0.98, "tier {tier} occupancy too high: {occ:.3}");
            if tier < LOWER_TIER_COUNT {
                lower_sum += occ;
            } else {
                upper_sum += occ;
            }
        }

        let lower_avg = lower_sum / LOWER_TIER_COUNT as f32;
        let upper_avg = upper_sum / (TIER_COUNT - LOWER_TIER_COUNT) as f32;
        assert!(
            lower_avg > upper_avg + 0.07,
            "lower deck should read denser (lower={lower_avg:.3}, upper={upper_avg:.3})"
        );
    }

    #[test]
    fn crowd_layout_is_deterministic() {
        let a = standard_layout();
        let b = standard_layout();
        assert_eq!(a, b);
    }
}

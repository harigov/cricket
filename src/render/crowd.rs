//! Stadium crowd population.
//!
//! Spectators are spawned as children of the stadium root by
//! [`spawn_crowd`], which is called once per match from
//! `crate::render::stadium::build_stadium`.

use std::f32::consts::TAU;

use bevy::gltf::GltfAssetLabel;
use bevy::prelude::*;

use crate::render::ring_geometry::{ring_face_center_rotation, ring_position, ring_tangent};
use crate::render::stadium::{StadiumBuildCtx, track_spawn};

const CROWD_SEGMENTS: usize = 90;
const CROWD_AISLE_EVERY: usize = 10;
/// Spectator tiers spread across lower and upper decks (keeps crowd count stable).
const CROWD_TIERS: [usize; 5] = [1, 3, 5, 8, 10];

fn crowd_segment_skipped(seg: usize) -> bool {
    seg.is_multiple_of(CROWD_AISLE_EVERY)
}

fn crowd_seats_at(seg: usize, tier: usize) -> usize {
    1 + (seg * 7 + tier * 11).is_multiple_of(3) as usize
}

pub(crate) fn spawn_crowd(
    p: &mut ChildSpawnerCommands,
    ctx: &StadiumBuildCtx<'_>,
    spawn_count: &mut usize,
) -> usize {
    let crowd_variants = [
        ctx.asset_server
            .load(GltfAssetLabel::Scene(0).from_asset("crowd/crowd-a.glb")),
        ctx.asset_server
            .load(GltfAssetLabel::Scene(0).from_asset("crowd/crowd-b.glb")),
        ctx.asset_server
            .load(GltfAssetLabel::Scene(0).from_asset("crowd/crowd-c.glb")),
        ctx.asset_server
            .load(GltfAssetLabel::Scene(0).from_asset("crowd/crowd-d.glb")),
    ];
    let crowd_scale = 0.62;
    let mut crowd_count = 0usize;
    let bowl = &ctx.bowl;

    for seg in 0..CROWD_SEGMENTS {
        if crowd_segment_skipped(seg) {
            continue;
        }
        for &tier in &CROWD_TIERS {
            // Stagger each tier's seat ring so figures don't stack in radial columns.
            let tier_phase =
                ((tier * 19 + 7) % CROWD_SEGMENTS) as f32 / CROWD_SEGMENTS as f32 * TAU;
            let seg_jitter = ((seg * 3 + tier * 13) % 5) as f32 - 2.0;
            let mid =
                (seg as f32 + 0.5 + seg_jitter * 0.18) / CROWD_SEGMENTS as f32 * TAU + tier_phase;
            let seats = crowd_seats_at(seg, tier);
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
                track_spawn(spawn_count);
                crowd_count += 1;
            }
        }
    }
    crowd_count
}

/// Expected crowd count for a standard bowl (used by tests).
pub fn expected_crowd_count() -> usize {
    let mut count = 0usize;
    for seg in 0..CROWD_SEGMENTS {
        if crowd_segment_skipped(seg) {
            continue;
        }
        for &tier in &CROWD_TIERS {
            count += crowd_seats_at(seg, tier);
        }
    }
    count
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
}

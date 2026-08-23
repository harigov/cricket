//! Fielder entities, placement and chase behaviour.

use crate::core::geometry::{self, FieldPos};
use crate::core::teams::Team;
use crate::game::ball::CricketBall;
use crate::render::player::{Anim, AnimState, face_target, yaw_to_face};
use bevy::prelude::*;

#[derive(Component)]
pub struct Fielder {
    pub slot: usize,
    pub is_keeper: bool,
    pub label: &'static str,
    /// Assigned post in world XZ (metres).
    pub post: Vec2,
}

/// Movement brain for a fielder.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub enum Brain {
    /// Standing at the assigned post.
    AtPost,
    /// Chasing the ball's projected position.
    Chase,
    /// Bending to pick up the ball at the feet.
    Collect,
    /// Walking back to post after collecting.
    Return,
}

pub const FIELDER_SPEED: f32 = 8.2;
const KEEPER_SPEED: f32 = 9.0;
/// Horizontal reach for ground collection (metres) — a fielder's pick-up radius.
pub const COLLECTION_RADIUS: f32 = 1.35;
/// Duration of the bend-and-gather pickup beat.
const COLLECT_SECS: f32 = 0.55;
/// Walk-back speed after collecting.
const RETURN_SPEED: f32 = 5.5;
/// Close enough to the post to stand down.
const POST_ARRIVAL_RADIUS: f32 = 0.4;
/// Exponential smoothing rate for fielders tracking the ball with their gaze.
const FACE_TRACK_RATE: f32 = 8.0;

/// Shortest signed delta from `from` to `to` on the yaw circle.
fn shortest_yaw_delta(from: f32, to: f32) -> f32 {
    let mut d = to - from;
    while d > std::f32::consts::PI {
        d -= std::f32::consts::TAU;
    }
    while d < -std::f32::consts::PI {
        d += std::f32::consts::TAU;
    }
    d
}

/// Frame-rate-independent yaw blend toward `target_yaw`.
fn smooth_yaw_toward(current_yaw: f32, target_yaw: f32, dt: f32) -> f32 {
    let blend = 1.0 - (-FACE_TRACK_RATE * dt).exp();
    current_yaw + shortest_yaw_delta(current_yaw, target_yaw) * blend
}

fn smooth_face_target(tf: &mut Transform, from: Vec2, to: Vec2, dt: f32) {
    let target_yaw = face_target(from, to);
    let current_yaw = tf.rotation.to_euler(EulerRot::YXZ).0;
    tf.rotation = Quat::from_rotation_y(smooth_yaw_toward(current_yaw, target_yaw, dt));
}

/// When a chaser closes within collection range, transition out of chase.
pub(crate) fn brain_on_chase_arrival(dist_to_ball: f32, brain: Brain) -> Brain {
    if matches!(brain, Brain::Chase) && dist_to_ball <= COLLECTION_RADIUS {
        Brain::Collect
    } else {
        brain
    }
}

/// Build slot-indexed world positions for fielders (index = `Fielder.slot`).
pub fn positions_by_slot(
    fielders: impl IntoIterator<Item = (usize, Vec2)>,
    slot_count: usize,
) -> Vec<Vec2> {
    let mut out = vec![Vec2::ZERO; slot_count];
    for (slot, pos) in fielders {
        if slot < slot_count {
            out[slot] = pos;
        }
    }
    out
}

/// Spawn the fielding side (keeper + 9 outfielders). The bowler figure is
/// managed separately by the match flow.
pub fn spawn_field_side(
    commands: &mut Commands,
    asset_server: &AssetServer,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    layout: &[FieldPos],
    team: &Team,
) -> Vec<Entity> {
    let mut out = Vec::new();
    for (slot, fp) in layout.iter().enumerate() {
        let pos = fp.world_pos(geometry::BATSMAN_POS);
        let facing = face_target(pos, geometry::BATSMAN_POS);
        let is_keeper = slot == 0;
        let e = crate::render::player::spawn_figure(
            commands,
            asset_server,
            meshes,
            materials,
            images,
            Vec3::new(pos.x, 0.0, pos.y),
            facing,
            team,
            if is_keeper {
                crate::render::player::FigureKind::Keeper
            } else {
                crate::render::player::FigureKind::Fielder(slot)
            },
        );
        commands.entity(e).insert((
            Fielder {
                slot,
                is_keeper,
                label: fp.name,
                post: pos,
            },
            Brain::AtPost,
        ));
        out.push(e);
    }
    out
}

/// Snap everyone back to their posts (start of delivery).
pub fn reset_brains(mut brains: Query<&mut Brain>) {
    for mut b in &mut brains {
        if !matches!(*b, Brain::AtPost) {
            *b = Brain::AtPost;
        }
    }
}

/// Move chasing fielders toward the live ball, predicting its path. Everyone
/// else continuously tracks the ball (or the striker when play is parked).
#[allow(clippy::type_complexity)]
pub fn chase_system(
    time: Res<Time>,
    ball_q: Query<&crate::game::ball::BallState, With<CricketBall>>,
    mut fielders: Query<(&Fielder, &mut Brain, &mut Transform, &mut Anim)>,
) {
    let Ok(ball) = ball_q.single() else { return };
    let dt = time.delta_secs();
    let ball_parked = ball.dead && ball.vel.length_squared() < 0.01;
    let ball_xz = Vec2::new(ball.pos.x, ball.pos.z);
    let look_target = if ball_parked {
        geometry::BATSMAN_POS
    } else {
        ball_xz
    };

    for (f, mut brain, mut tf, mut anim) in &mut fielders {
        let pos = Vec2::new(tf.translation.x, tf.translation.z);

        match *brain {
            Brain::Chase if !ball_parked => {
                let to_ball =
                    Vec2::new(ball.pos.x - tf.translation.x, ball.pos.z - tf.translation.z);
                let dist = to_ball.length();
                if dist <= COLLECTION_RADIUS {
                    *brain = Brain::Collect;
                    anim.state = AnimState::Throw { p: 0.0 };
                    smooth_face_target(&mut tf, pos, ball_xz, dt);
                    continue;
                }

                // Sprint animation while chasing.
                if !matches!(anim.state, AnimState::Run { .. }) {
                    anim.state = AnimState::Run { t: 0.0 };
                } else if let AnimState::Run { t } = &mut anim.state {
                    *t += dt;
                }
                let speed = if f.is_keeper {
                    KEEPER_SPEED
                } else {
                    FIELDER_SPEED
                };
                // Predict where the ball will be when we get there (simple lead)
                let t_intercept = (dist / speed).clamp(0.0, 0.65);
                // Only predict horizontal motion; vertical is irrelevant for ground chase
                let pred = Vec2::new(
                    ball.pos.x + ball.vel.x * t_intercept * 0.55,
                    ball.pos.z + ball.vel.z * t_intercept * 0.55,
                );
                let to_pred = Vec2::new(pred.x - tf.translation.x, pred.y - tf.translation.z);
                let d2 = to_pred.length();
                if d2 < 1e-4 {
                    smooth_face_target(&mut tf, pos, look_target, dt);
                    continue;
                }
                let step = (speed * dt).min(d2);
                let dir = to_pred / d2;
                tf.translation.x += dir.x * step;
                tf.translation.z += dir.y * step;
                tf.rotation = Quat::from_rotation_y(yaw_to_face(dir));
            }
            Brain::Collect => {
                smooth_face_target(&mut tf, pos, ball_xz, dt);
                let p = match &mut anim.state {
                    AnimState::Throw { p } => p,
                    _ => {
                        anim.state = AnimState::Throw { p: 0.0 };
                        if let AnimState::Throw { p } = &mut anim.state {
                            p
                        } else {
                            continue;
                        }
                    }
                };
                *p = (*p + dt / COLLECT_SECS).min(1.0);
                if *p >= 1.0 {
                    *brain = Brain::Return;
                    anim.state = AnimState::Idle;
                }
            }
            Brain::Return => {
                let to_post = f.post - pos;
                let dist = to_post.length();
                if dist <= POST_ARRIVAL_RADIUS {
                    *brain = Brain::AtPost;
                    anim.state = AnimState::Idle;
                    smooth_face_target(&mut tf, pos, geometry::BATSMAN_POS, dt);
                } else {
                    if !matches!(anim.state, AnimState::Run { .. }) {
                        anim.state = AnimState::Run { t: 0.0 };
                    } else if let AnimState::Run { t } = &mut anim.state {
                        *t += dt;
                    }
                    let step = (RETURN_SPEED * dt).min(dist);
                    let dir = to_post / dist;
                    tf.translation.x += dir.x * step;
                    tf.translation.z += dir.y * step;
                    tf.rotation = Quat::from_rotation_y(yaw_to_face(dir));
                }
            }
            Brain::AtPost | Brain::Chase => {
                smooth_face_target(&mut tf, pos, look_target, dt);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_by_slot_maps_fielder_index() {
        let positions = positions_by_slot([(2, Vec2::new(10.0, 5.0)), (0, Vec2::new(1.0, 2.0))], 4);
        assert_eq!(positions[0], Vec2::new(1.0, 2.0));
        assert_eq!(positions[2], Vec2::new(10.0, 5.0));
        assert_eq!(positions[1], Vec2::ZERO);
    }

    #[test]
    fn positions_by_slot_ignores_out_of_range() {
        let positions = positions_by_slot([(9, Vec2::new(3.0, 4.0))], 3);
        assert_eq!(positions.len(), 3);
        assert_eq!(positions[2], Vec2::ZERO);
    }

    #[test]
    fn shortest_yaw_delta_takes_short_arc() {
        let d = shortest_yaw_delta(3.0, -3.0);
        assert!(d.abs() < 0.3, "expected short wrap (~0.28 rad), got {d}");
        assert!(
            (shortest_yaw_delta(0.0, std::f32::consts::FRAC_PI_2) - std::f32::consts::FRAC_PI_2)
                .abs()
                < 1e-5
        );
    }

    #[test]
    fn smooth_yaw_toward_moves_toward_target() {
        let next = smooth_yaw_toward(0.0, std::f32::consts::FRAC_PI_2, 0.1);
        assert!(next > 0.0 && next < std::f32::consts::FRAC_PI_2);
        let settled = smooth_yaw_toward(
            std::f32::consts::PI - 0.01,
            -std::f32::consts::PI + 0.01,
            1.0,
        );
        let delta = shortest_yaw_delta(settled, -std::f32::consts::PI + 0.01);
        assert!(
            delta.abs() < 0.05,
            "should wrap the short way, settled={settled} delta={delta}"
        );
    }

    #[test]
    fn chaser_within_collection_radius_leaves_chase() {
        assert_eq!(
            brain_on_chase_arrival(COLLECTION_RADIUS, Brain::Chase),
            Brain::Collect
        );
        assert_eq!(
            brain_on_chase_arrival(COLLECTION_RADIUS + 0.1, Brain::Chase),
            Brain::Chase
        );
        assert_eq!(
            brain_on_chase_arrival(0.5, Brain::AtPost),
            Brain::AtPost
        );
    }
}

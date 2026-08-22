//! Ball physics: gravity, drag, swing, pitch bounce with spin turn.

use bevy::prelude::*;

pub const BALL_RADIUS: f32 = 0.036;
const GRAVITY: f32 = 9.81;
/// Slows the whole simulation of ball flight so timings are humanly
/// playable (a common arcade-cricket trick).
pub const BALL_TIME_SCALE: f32 = 0.62;
const DRAG: f32 = 0.06; // per-second velocity damping in air

#[derive(Component, Default)]
pub struct CricketBall;

#[derive(Component, Debug, Clone)]
pub struct BallState {
    pub pos: Vec3,
    pub vel: Vec3,
    /// True once the delivery is resolved (scoring decided).
    pub dead: bool,
    /// True after the ball has bounced off the pitch.
    pub bounced: bool,
    /// True once the batter has struck it (disables swing).
    pub struck: bool,
}

impl Default for BallState {
    fn default() -> Self {
        BallState {
            pos: Vec3::ZERO,
            vel: Vec3::ZERO,
            dead: true,
            bounced: false,
            struck: false,
        }
    }
}

impl BallState {
    pub fn new_release(pos: Vec3, vel: Vec3) -> Self {
        BallState { pos, vel, dead: false, bounced: false, struck: false }
    }
}

/// Result flags written by the physics step for the flow system to read.
#[derive(Component, Default, Debug)]
pub struct BallFlags {
    /// Ball crossed the bat plane this frame.
    pub crossed_bat_plane: bool,
    /// Ball bounced this frame.
    pub just_bounced: bool,
    /// Position where the bounce happened.
    pub bounce_pos: Vec3,
}

/// Integrate the ball. Returns nothing; writes flags onto the entity.
pub fn physics_step(
    state: &mut BallState,
    flags: &mut BallFlags,
    plan_swing: f32,
    plan_turn: f32,
    dt_raw: f32,
) {
    let dt = dt_raw * BALL_TIME_SCALE;

    let pre_x = state.pos.x;
    let _pre_y = state.pos.y;

    // Gravity + drag + swing (swing only before bouncing or being struck).
    let mut accel = Vec3::new(0.0, -GRAVITY, 0.0);
    if !state.bounced && !state.struck {
        accel.z += plan_swing;
        state.vel *= 1.0 - DRAG * dt;
    } else {
        // Rolling friction after bounce scales with height ~ ground contact.
        if state.pos.y < BALL_RADIUS * 2.0 {
            let horiz = Vec2::new(state.vel.x, state.vel.z);
            let decel = 4.5 * dt;
            let mag = horiz.length();
            if mag > 0.01 {
                let new_mag = (mag - decel).max(0.0);
                let dir = horiz / mag;
                state.vel.x = dir.x * new_mag;
                state.vel.z = dir.y * new_mag;
            } else {
                state.vel.x = 0.0;
                state.vel.z = 0.0;
            }
        }
    }

    state.vel += accel * dt;
    state.pos += state.vel * dt;

    // Pitch bounce.
    flags.just_bounced = false;
    if state.pos.y < BALL_RADIUS && state.vel.y < 0.0 {
        state.pos.y = BALL_RADIUS;
        state.vel.y = -state.vel.y;
        flags.just_bounced = true;
        state.bounced = true;
        flags.bounce_pos = state.pos;
        // Spin turn applied as an instantaneous z-velocity change.
        state.vel.z += plan_turn;
    }

    flags.crossed_bat_plane = pre_x < BAT_PLANE_X && state.pos.x >= BAT_PLANE_X
        && state.vel.x > 1.0;
}

/// X position where we test bat contact (just in front of the striker).
pub const BAT_PLANE_X: f32 =
    crate::core::geometry::PITCH_HALF_LEN - 1.1;

/// Predict time until the ball reaches the bat plane (simple linear est.).
pub fn time_to_bat_plane(state: &BallState) -> Option<f32> {
    if state.vel.x <= 0.01 {
        return None;
    }
    Some((BAT_PLANE_X - state.pos.x) / state.vel.x)
}

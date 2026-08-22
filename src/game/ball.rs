//! Ball physics: gravity, drag, Magnus swing, pitch bounce with spin turn.
//! Realistic parameters: mass 0.16 kg, drag Cd≈0.47, Magnus for swing/spin.

use bevy::prelude::*;

pub const BALL_RADIUS: f32 = 0.036;
const GRAVITY: f32 = 9.81;
/// Slows the whole simulation of ball flight so timings are humanly
/// playable (a common arcade-cricket trick).
pub const BALL_TIME_SCALE: f32 = 0.62;
// Realistic aero constants (cricket ball: 155.9–163 g, 71 mm diam)
const MASS: f32 = 0.160; // kg
const RHO: f32 = 1.225; // kg/m3 air density
const CD: f32 = 0.47; // drag coeff
const AREA: f32 = std::f32::consts::PI * BALL_RADIUS * BALL_RADIUS;
const DRAG_K: f32 = 0.5 * RHO * CD * AREA / MASS; // ≈0.0073
const MAGNUS_K: f32 = 0.055; // scales plan_swing → lateral accel

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
/// `pitch` is used for bounce restitution/grip/turn. Pass `None` for
/// default (Hard) behaviour (useful for unit tests).
pub fn physics_step(
    state: &mut BallState,
    flags: &mut BallFlags,
    plan_swing: f32,
    plan_turn: f32,
    dt_raw: f32,
) {
    physics_step_with_pitch(state, flags, plan_swing, plan_turn, dt_raw, None);
}

pub fn physics_step_with_pitch(
    state: &mut BallState,
    flags: &mut BallFlags,
    plan_swing: f32,
    plan_turn: f32,
    dt_raw: f32,
    pitch: Option<crate::core::stadiums::PitchType>,
) {
    use crate::core::stadiums::PitchType;
    let dt = dt_raw * BALL_TIME_SCALE;

    let pre_x = state.pos.x;

    let pitch = pitch.unwrap_or(PitchType::Hard);
    let bounce_mul = pitch.bounce_mul();
    let turn_mul = pitch.turn_mul();
    let grip_mul = pitch.grip_mul();

    // --- Aerodynamics: gravity + drag + Magnus swing ---
    let mut accel = Vec3::new(0.0, -GRAVITY, 0.0);
    let speed = state.vel.length();
    if speed > 0.01 {
        // Quadratic drag: a_drag = -k * |v| * v
        let drag_a = DRAG_K * speed;
        accel -= state.vel * drag_a;

        // Magnus swing: lateral (Z) acceleration proportional to speed.
        // plan_swing is the seam-induced Magnus coefficient (−0.15..0.9).
        // We treat it as Cl-like value; scale by speed for realistic curve
        // that grows with pace (fast bowlers swing more at pace).
        if !state.bounced && !state.struck {
            let speed_factor = (speed / 28.0).clamp(0.45, 1.35);
            // Seam vertical → lateral Magnus; calibrated so fast bowler swing 0.9 → ~1.6 m/s²
            accel.z += plan_swing * MAGNUS_K * speed * speed_factor;
            // Subtle vertical lift from backspin (reduces effective gravity slightly)
            if plan_swing.abs() > 0.5 {
                accel.y += 0.45 * plan_swing.signum() * speed_factor;
            }
        } else if state.bounced && !state.struck {
            // Post-bounce Magnus from spin (turn)
            let spin_factor = (speed / 22.0).clamp(0.3, 1.2);
            accel.z += plan_turn * 0.022 * speed * spin_factor;
        }
        // Struck ball: no swing/spin Magnus (bat imparts its own, handled at contact)
    }

    // Rolling friction when ball is on/near ground after bounce
    if state.bounced && state.pos.y < BALL_RADIUS * 2.2 {
        let horiz = Vec2::new(state.vel.x, state.vel.z);
        let mu = 0.38 * (1.8 - grip_mul * 0.6); // Dusty grips more
        let decel = mu * GRAVITY * dt * 1.6;
        let mag = horiz.length();
        if mag > 0.02 {
            let new_mag = (mag - decel).max(0.0);
            // Apply friction as reduction of horizontal velocity
            let scale = new_mag / mag;
            state.vel.x *= scale;
            state.vel.z *= scale;
        } else {
            state.vel.x = 0.0;
            state.vel.z = 0.0;
        }
    }

    state.vel += accel * dt;
    state.pos += state.vel * dt;

    // --- Pitch bounce: restitution + grip + seam/spin turn ---
    flags.just_bounced = false;
    if state.pos.y < BALL_RADIUS && state.vel.y < 0.0 {
        state.pos.y = BALL_RADIUS;
        // Restitution depends on pitch hardness and incoming vertical speed
        let e_base = 0.58;
        let e = (e_base * bounce_mul).clamp(0.38, 0.82);
        state.vel.y = -state.vel.y * e;
        // Pace off the pitch: softer/dusty pitches kill horizontal pace
        let retain = (0.92 * grip_mul).clamp(0.78, 0.98);
        state.vel.x *= retain;
        state.vel.z *= retain;
        // Seam/spin deviation: instantaneous lateral impulse scaled by pitch turn
        // plus a tiny random seam wobble for realism (±3 cm/s)
        let wobble = (state.pos.x * 12.98).sin() * 0.03;
        state.vel.z += plan_turn * turn_mul * 0.95 + wobble;

        flags.just_bounced = true;
        state.bounced = true;
        flags.bounce_pos = state.pos;
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

// ---- Ball trail (visual juice for struck balls) ----
#[derive(Component)]
pub struct TrailDot {
    age: f32,
    lifespan: f32,
}

pub fn trail_spawn_system(
    mut commands: Commands,
    time: Res<Time>,
    ball_q: Query<(&BallState, &Transform), With<CricketBall>>,
    mut timer: Local<f32>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Ok((bs, tf)) = ball_q.single() else { return };
    if !bs.struck || bs.dead {
        *timer = 0.0;
        return;
    }
    let speed = bs.vel.length();
    if speed < 10.0 {
        return;
    }
    *timer += time.delta_secs();
    if *timer < 0.028 {
        return;
    }
    *timer = 0.0;
    // Spawn a fading dot at the ball's current position
    let size = 0.032 + (speed / 40.0).clamp(0.0, 0.025);
    let alpha = 0.55;
    commands.spawn((
        TrailDot { age: 0.0, lifespan: 0.45 },
        Mesh3d(meshes.add(Sphere::new(size))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 0.96, 0.88, alpha),
            emissive: LinearRgba::new(2.2, 1.1, 0.4, 1.0),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        })),
        Transform::from_translation(tf.translation),
    ));
}

pub fn trail_fade_system(
    mut commands: Commands,
    time: Res<Time>,
    mut dots: Query<(Entity, &mut TrailDot, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (e, mut dot, mut tf, handle) in &mut dots {
        dot.age += time.delta_secs();
        let t = (dot.age / dot.lifespan).clamp(0.0, 1.0);
        if t >= 1.0 {
            commands.entity(e).despawn();
            continue;
        }
        // shrink + fade
        let s = 1.0 - t * 0.7;
        tf.scale = Vec3::splat(s);
        if let Some(mat) = materials.get_mut(&handle.0) {
            let a = (1.0 - t) * 0.55;
            if let Color::Srgba(mut c) = mat.base_color {
                c.alpha = a;
                mat.base_color = Color::Srgba(c);
            } else {
                // fallback: lerp alpha via srgba conversion
                let mut srgba = mat.base_color.to_srgba();
                srgba.alpha = a;
                mat.base_color = Color::Srgba(srgba);
            }
            mat.emissive = LinearRgba::new(2.2 * (1.0 - t), 1.1 * (1.0 - t), 0.4 * (1.0 - t), 1.0);
        }
    }
}

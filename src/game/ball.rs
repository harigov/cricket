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

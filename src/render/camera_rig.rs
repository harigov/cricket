//! Camera rig: smoothly blends between gameplay camera modes.

use bevy::prelude::*;

#[derive(Resource, Default)]
pub struct CameraRig {
    pub mode: CamMode,
    /// Current smoothed position / look target.
    pos: Vec3,
    look: Vec3,
    init: bool,
    pub shake: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CamMode {
    #[default]
    BattingEnd,
    BowlingEnd,
    Broadcast,
    FollowBall,
}

impl CamMode {
    pub fn toggle_next(&self) -> Self {
        match self {
            CamMode::BattingEnd => CamMode::BowlingEnd,
            CamMode::BowlingEnd => CamMode::Broadcast,
            CamMode::Broadcast => CamMode::FollowBall,
            CamMode::FollowBall => CamMode::BattingEnd,
        }
    }
}

pub fn camera_toggle_system(
    input: Res<crate::input::PlayerInput>,
    mut rig: ResMut<CameraRig>,
) {
    if input.pressed(crate::input::Action::CycleCam) {
        rig.mode = rig.mode.toggle_next();
    }
}

pub fn trigger_shake(rig: &mut CameraRig, intensity: f32) {
    rig.shake = rig.shake.max(intensity);
}

pub fn update_camera(
    time: Res<Time>,
    ball_q: Query<&Transform, (With<crate::game::ball::CricketBall>, Without<Camera3d>)>,
    mut rig: ResMut<CameraRig>,
    mut cam: Query<&mut Transform, With<Camera3d>>,
) {
    let (target_pos, target_look) = match rig.mode {
        CamMode::BattingEnd => (
            Vec3::new(24.0, 8.0, 0.0),
            Vec3::new(-10.0, 1.2, 0.0),
        ),
        CamMode::BowlingEnd => (
            Vec3::new(-26.0, 9.5, 4.0),
            Vec3::new(12.0, 1.0, 0.0),
        ),
        CamMode::Broadcast => (
            Vec3::new(0.0, 34.0, 55.0),
            Vec3::new(0.0, 0.0, 0.0),
        ),
        CamMode::FollowBall => {
            if let Ok(bt) = ball_q.single() {
                (
                    bt.translation + Vec3::new(-14.0, 9.0, 10.0),
                    bt.translation,
                )
            } else {
                (Vec3::new(20.0, 10.0, 15.0), Vec3::ZERO)
            }
        }
    };

    if !rig.init {
        rig.pos = target_pos;
        rig.look = target_look;
        rig.init = true;
    }

    // Shake decays exponentially
    if rig.shake > 0.001 {
        rig.shake *= (-8.0 * time.delta_secs()).exp();
    } else {
        rig.shake = 0.0;
    }

    let k = (1.0 - (-6.0 * time.delta_secs()).exp()).clamp(0.0, 1.0);
    rig.pos = rig.pos.lerp(target_pos, k);
    rig.look = rig.look.lerp(target_look, k);

    // Apply shake as random offset
    let mut pos = rig.pos;
    let mut look = rig.look;
    if rig.shake > 0.01 {
        let n = time.elapsed_secs();
        pos += Vec3::new(
            (n * 47.0).sin() * rig.shake * 0.6,
            (n * 59.0).sin() * rig.shake * 0.35,
            (n * 37.0).sin() * rig.shake * 0.6,
        );
        look += Vec3::new(
            (n * 31.0).cos() * rig.shake * 0.25,
            0.0,
            (n * 43.0).cos() * rig.shake * 0.25,
        );
    }

    if let Ok(mut tf) = cam.single_mut() {
        tf.translation = pos;
        tf.look_at(look, Vec3::Y);
    }
}

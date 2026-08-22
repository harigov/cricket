//! Camera rig: broadcast-style direction with telephoto gameplay lenses,
//! impact cuts, boundary cameras and slow-motion replays.

use bevy::prelude::*;

use crate::game::ball::CricketBall;
use crate::core::geometry;

#[derive(Resource, Default)]
pub struct CameraRig {
    pub mode: CamMode,
    /// Current smoothed position / look target.
    pos: Vec3,
    look: Vec3,
    fov: f32,
    init: bool,
    pub shake: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CamMode {
    /// Low behind-keeper telephoto — classic broadcast batting lens.
    #[default]
    BattingEnd,
    /// Mirrored view from the bowler's end.
    BowlingEnd,
    /// Elevated establishing shot (over breaks, menus).
    Broadcast,
    /// Tight chase cam tracking the struck ball.
    FollowBall,
    /// Dramatic low close-up at the stumps for wickets.
    ImpactCut,
    /// Rope-level camera when the ball races to the boundary.
    BoundaryCam,
    /// Slow-motion side-on replay.
    ReplaySide,
}

impl CamMode {
    pub fn toggle_next(&self) -> Self {
        match self {
            CamMode::BattingEnd => CamMode::BowlingEnd,
            CamMode::BowlingEnd => CamMode::Broadcast,
            CamMode::Broadcast => CamMode::FollowBall,
            _ => CamMode::BattingEnd,
        }
    }
}

/// Recorded ball flight of the current delivery (seconds since release).
#[derive(Resource, Default)]
pub struct BallRecording {
    pub samples: Vec<(f32, Vec3)>,
    pub t: f32,
}

/// Replay playback state driven by the presentation director in match flow.
#[derive(Resource, Default)]
pub struct ReplayState {
    pub active: bool,
    pub t_play: f32,
    pub dur: f32,
}

/// Presentation director state shared with the HUD (e.g. REPLAY chip).
#[derive(Resource, Default)]
pub struct PresentationState {
    /// True while the slow-motion replay is on screen.
    pub replay_on: bool,
    /// True while the wicket impact cut is on screen.
    pub impact_on: bool,
}

pub fn camera_toggle_system(
    input: Res<crate::input::PlayerInput>,
    mut rig: ResMut<CameraRig>,
) {
    if input.pressed(crate::input::Action::CycleCam) {
        rig.mode = rig.mode.toggle_next();
    }
}

/// Compute the desired (position, look target, fov degrees) for a mode.
pub fn mode_view(
    mode: CamMode,
    ball: Option<Vec3>,
    boundary_r: f32,
    replay_pos: Option<Vec3>,
) -> (Vec3, Vec3, f32) {
    let stump_x = geometry::PITCH_HALF_LEN;
    match mode {
        // Medium telephoto behind the striker's end: down-axis with a slight
        // lateral offset so foreground bodies sit beside the wicket line.
        CamMode::BattingEnd => (
            Vec3::new(stump_x + 11.5, 7.2, 4.2),
            Vec3::new(-stump_x * 0.30, 0.90, 0.0),
            25.0,
        ),
        CamMode::BowlingEnd => (
            Vec3::new(-stump_x - 11.5, 7.2, -4.2),
            Vec3::new(stump_x * 0.30, 0.90, 0.0),
            25.0,
        ),
        CamMode::Broadcast => broadcast_establishing_view(boundary_r),
        CamMode::FollowBall => {
            let b = ball.unwrap_or(Vec3::new(20.0, 3.0, 10.0));
            (b + Vec3::new(-8.0, 3.2, 6.5), b + Vec3::new(1.5, 0.0, 0.0), 34.0)
        }
        CamMode::ImpactCut => (
            Vec3::new(stump_x + 5.5, 2.8, 7.0),
            Vec3::new(stump_x - 0.5, 0.65, 0.0),
            32.0,
        ),
        CamMode::BoundaryCam => {
            let b = ball.unwrap_or(Vec3::new(stump_x, 1.0, 0.0));
            let flat = Vec2::new(b.x, b.z);
            let dir = if flat.length_squared() > 0.01 { flat.normalize() } else { Vec2::X };
            // Sit just outside the rope along the ball's line of travel.
            let pos_flat = dir * (boundary_r + 6.0);
            (Vec3::new(pos_flat.x, 2.3, pos_flat.y), b, 28.0)
        }
        CamMode::ReplaySide => {
            // Side-on medium lens following the recorded flight.
            let focus = replay_pos
                .unwrap_or(Vec3::new(geometry::BATSMAN_POS.x, 1.0, 0.0));
            (
                Vec3::new(focus.x * 0.55 + 1.0, 2.4, if focus.z >= 0.0 { 14.0 } else { -14.0 }),
                focus,
                24.0,
            )
        }
    }
}

/// Wide elevated establishing shot: full oval bowl, sky, crowd tiers and towers.
pub fn broadcast_establishing_view(boundary_r: f32) -> (Vec3, Vec3, f32) {
    // Match BowlLayout::from_boundary proportions in stadium.rs.
    let bowl_outer = boundary_r + 3.2 + 1.9 * 5.0;
    let tower_r = bowl_outer + 7.5;
    let span = tower_r * 2.0;

    // High aerial crane behind the bowler's end: full oval, all four towers, sky headroom.
    let cam_back = span * 0.88;
    let cam_height = span * 0.56;
    let cam_side = boundary_r * 0.18;
    let pos = Vec3::new(cam_side, cam_height, cam_back);
    let look = Vec3::new(0.0, 4.0, 0.0);
    (pos, look, 65.0)
}

#[allow(clippy::too_many_arguments)]
pub fn update_camera(
    time: Res<Time>,
    ball_q: Query<&Transform, (With<CricketBall>, Without<Camera3d>)>,
    am: Option<Res<crate::game::ActiveMatch>>,
    wd: Option<Res<crate::game::WorldData>>,
    replay: Res<ReplayState>,
    recording: Res<BallRecording>,
    mut rig: ResMut<CameraRig>,
    mut cam: Query<(&mut Transform, &mut Projection), With<Camera3d>>,
) {
    let ball_pos = ball_q.iter().next().map(|t| t.translation);
    let replay_pos = if replay.active {
        sample_recording(&recording.samples, replay.t_play)
    } else {
        None
    };
    let boundary_r = wd
        .as_ref()
        .and_then(|w| am.as_ref().map(|a| w.stadiums[a.stadium].boundary_radius()))
        .unwrap_or(geometry::DEFAULT_BOUNDARY_RADIUS);

    let (mut target_pos, mut target_look, mut target_fov) =
        mode_view(rig.mode, ball_pos, boundary_r, replay_pos);

    if !rig.init {
        rig.pos = target_pos;
        rig.look = target_look;
        rig.fov = target_fov;
        rig.init = true;
    }

    // Shake decays exponentially
    if rig.shake > 0.001 {
        rig.shake *= (-8.0 * time.delta_secs()).exp();
    } else {
        rig.shake = 0.0;
    }

    // Snappy cuts between modes (broadcast switching), smooth within a mode:
    // use a faster blend so mode switches feel like deliberate cuts rather
    // than floaty drifts.
    let k = (1.0 - (-9.0 * time.delta_secs()).exp()).clamp(0.0, 1.0);
    rig.pos = rig.pos.lerp(target_pos, k);
    rig.look = rig.look.lerp(target_look, k);
    rig.fov += (target_fov - rig.fov) * k;

    // Apply shake as random offset
    let mut pos = rig.pos;
    let mut look = rig.look;
    if rig.shake > 0.01 {
        let n = time.elapsed_secs();
        pos += Vec3::new(
            (n * 47.0).sin() * rig.shake * 0.25,
            (n * 59.0).sin() * rig.shake * 0.15,
            (n * 37.0).sin() * rig.shake * 0.25,
        );
        look += Vec3::new(
            (n * 31.0).cos() * rig.shake * 0.18,
            0.0,
            (n * 43.0).cos() * rig.shake * 0.18,
        );
    }

    if let Ok((mut tf, mut proj)) = cam.single_mut() {
        tf.translation = pos;
        tf.look_at(look, Vec3::Y);
        if let Projection::Perspective(p) = &mut *proj {
            p.fov = rig.fov.to_radians();
        }
    }
}

/// Sample the recorded delivery path at `t` seconds since release.
pub fn sample_recording(recording: &[(f32, Vec3)], t: f32) -> Option<Vec3> {
    if recording.is_empty() {
        return None;
    }
    let first = recording[0].0;
    let last = recording[recording.len() - 1].0;
    let tc = t.clamp(first, last.max(first));
    for w in recording.windows(2) {
        let (t0, p0) = w[0];
        let (t1, p1) = w[1];
        if tc <= t1 {
            let u = ((tc - t0) / (t1 - t0).max(1e-5)).clamp(0.0, 1.0);
            return Some(p0.lerp(p1, u));
        }
    }
    recording.last().map(|(_, p)| *p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::geometry;

    #[test]
    fn broadcast_pulls_back_beyond_boundary() {
        let br = geometry::DEFAULT_BOUNDARY_RADIUS;
        let (pos, _, _) = broadcast_establishing_view(br);
        let flat = Vec2::new(pos.x, pos.z);
        assert!(flat.length() > br + 25.0, "camera too close: {flat:?}");
    }

    #[test]
    fn broadcast_elevated_with_sky_headroom() {
        let br = 65.0;
        let (pos, look, fov) = broadcast_establishing_view(br);
        assert!(pos.y > 80.0, "establishing cam should be high: y={}", pos.y);
        assert!(look.y < 8.0, "look target should sit near field center: y={}", look.y);
        assert!(look.y.abs() > 2.0, "look target should not be at ground level");
        assert!(look.z.abs() < 5.0, "look target should be near pitch center: z={}", look.z);
        assert!(fov >= 62.0, "wide lens for full-stadium framing: fov={fov}");
        let flat = Vec2::new(pos.x, pos.z);
        assert!(
            flat.length() > br + 60.0,
            "camera should sit well outside the boundary: {flat:?}"
        );
    }

    #[test]
    fn broadcast_wider_than_batting_lens() {
        let br = 65.0;
        let (_, _, bat_fov) = mode_view(CamMode::BattingEnd, None, br, None);
        let (_, _, wide_fov) = mode_view(CamMode::Broadcast, None, br, None);
        assert!(wide_fov > bat_fov + 15.0);
    }

    #[test]
    fn broadcast_scales_with_stadium_size() {
        let small = broadcast_establishing_view(55.0).0;
        let large = broadcast_establishing_view(75.0).0;
        assert!(large.z > small.z);
        assert!(large.y > small.y);
    }
}

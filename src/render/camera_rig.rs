//! Camera rig: broadcast-style direction with telephoto gameplay lenses,
//! impact cuts, boundary cameras and slow-motion replays.

use bevy::prelude::*;

use crate::core::geometry;
use crate::game::ball::CricketBall;
use crate::render::ring_geometry::floodlight_radius;
use crate::render::stadium::BowlLayout;

/// Batting-end camera height: 12 ft in metres (12 × 0.3048).
const BATTING_CAM_HEIGHT_M: f32 = 3.6576;
/// Distance behind the striker along the pitch axis: 10 ft in metres (10 × 0.3048).
const BATTING_CAM_BEHIND_BATSMAN_M: f32 = 3.048;
/// Aim point a few metres past the striker, down the pitch.
const BATTING_CAM_LOOK_X: f32 = 2.0;
/// Aim height, near the strip so the lens tilts down enough to frame the striker.
const BATTING_CAM_LOOK_Y: f32 = 0.35;

/// Opening walk-on lens: side-on to the square, close enough that the batters
/// read as people rather than specks (the establishing shot sits ~230 m out).
const INTRO_CAM_SIDE: f32 = 34.0;
const INTRO_CAM_HEIGHT: f32 = 11.0;
const INTRO_CAM_BACK: f32 = 12.0;
/// Vertical FOV. The 10 ft standoff is short, so framing the striker *and* the
/// bowler 30 m away needs a wide lens - wider than the establishing shot.
const BATTING_CAM_FOV_DEG: f32 = 60.0;

/// Bowling-end lens: a static camera 10 ft up and 25 ft back from the bowler's stumps.
const BOWLING_CAM_HEIGHT_M: f32 = 3.048; // 10 ft
const BOWLING_CAM_SETBACK_M: f32 = 7.62; // 25 ft
/// Vertical FOV. At ~27 m the vertical frame is ~12 m tall (2 × 26.8 m × tan(12.5°)),
/// framing the striker, stumps and pitch strip without wasting the 25 ft standoff.
const BOWLING_CAM_FOV_DEG: f32 = 25.0;

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
    /// Cinematic side-on lens for the opening walk-on.
    MatchIntro,
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

pub fn camera_toggle_system(input: Res<crate::input::PlayerInput>, mut rig: ResMut<CameraRig>) {
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
        // Over-the-shoulder from a 12 ft pole, 10 ft behind the striker —
        // short enough that a wider lens keeps the release point readable.
        CamMode::BattingEnd => {
            let batsman = geometry::BATSMAN_POS;
            let cam = Vec3::new(
                batsman.x + BATTING_CAM_BEHIND_BATSMAN_M,
                BATTING_CAM_HEIGHT_M,
                batsman.y,
            );
            // Aim short of the bowler and low: from only 10 ft back, sighting
            // the release point directly tilts the lens far enough up that the
            // striker drops out of frame, and a batting view has to show the
            // batter playing the ball as well as the ball itself.
            let look = Vec3::new(
                BATTING_CAM_LOOK_X,
                BATTING_CAM_LOOK_Y,
                geometry::RELEASE_POINT.z,
            );
            (cam, look, BATTING_CAM_FOV_DEG)
        }
        // Static over-the-shoulder from a 10 ft pole, 25 ft behind the bowler's
        // stumps — far enough back for a telephoto lens aimed straight at the striker.
        CamMode::BowlingEnd => {
            let cam = Vec3::new(-stump_x - BOWLING_CAM_SETBACK_M, BOWLING_CAM_HEIGHT_M, 0.0);
            // Unlike the batting lens (only 10 ft back), 25 ft gives enough standoff
            // to sight the striker's stumps directly — the optical axis lands on the
            // batsman with the pitch leading the eye into them.
            let look = Vec3::new(
                geometry::PITCH_HALF_LEN,
                geometry::STUMP_HEIGHT,
                geometry::BATSMAN_POS.y,
            );
            (cam, look, BOWLING_CAM_FOV_DEG)
        }
        CamMode::Broadcast => broadcast_establishing_view(boundary_r),
        CamMode::FollowBall => {
            let b = ball.unwrap_or(Vec3::new(20.0, 3.0, 10.0));
            (
                b + Vec3::new(-8.0, 3.2, 6.5),
                b + Vec3::new(1.5, 0.0, 0.0),
                34.0,
            )
        }
        CamMode::ImpactCut => (
            Vec3::new(stump_x + 5.5, 2.8, 7.0),
            Vec3::new(stump_x - 0.5, 0.65, 0.0),
            32.0,
        ),
        CamMode::BoundaryCam => {
            let b = ball.unwrap_or(Vec3::new(stump_x, 1.0, 0.0));
            let flat = Vec2::new(b.x, b.z);
            let dir = if flat.length_squared() > 0.01 {
                flat.normalize()
            } else {
                Vec2::X
            };
            // Sit just outside the rope along the ball's line of travel.
            let pos_flat = dir * (boundary_r + 6.0);
            (Vec3::new(pos_flat.x, 2.3, pos_flat.y), b, 28.0)
        }
        CamMode::MatchIntro => (
            Vec3::new(INTRO_CAM_BACK, INTRO_CAM_HEIGHT, INTRO_CAM_SIDE),
            Vec3::new(0.0, 1.2, 0.0),
            40.0,
        ),
        CamMode::ReplaySide => {
            // Side-on medium lens following the recorded flight.
            let focus = replay_pos.unwrap_or(Vec3::new(geometry::BATSMAN_POS.x, 1.0, 0.0));
            (
                Vec3::new(
                    focus.x * 0.55 + 1.0,
                    2.4,
                    if focus.z >= 0.0 { 14.0 } else { -14.0 },
                ),
                focus,
                24.0,
            )
        }
    }
}

/// Wide elevated establishing shot: full oval bowl, sky, crowd tiers and towers.
pub fn broadcast_establishing_view(boundary_r: f32) -> (Vec3, Vec3, f32) {
    // Bowl and tower radii shared with stadium spawn (BowlLayout, floodlight_radius).
    let bowl_outer = BowlLayout::from_boundary(boundary_r).outer_radius();
    let tower_r = floodlight_radius(bowl_outer);
    let span = tower_r * 2.0;

    // High aerial crane behind the bowler's end: full oval, all four towers, sky headroom.
    let cam_back = span * 0.88;
    let cam_height = span * 0.56;
    let cam_side = boundary_r * 0.18;
    let pos = Vec3::new(cam_side, cam_height, cam_back);
    let look = Vec3::new(0.0, 4.0, 0.0);
    (pos, look, 65.0)
}

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

    let (target_pos, target_look, target_fov) =
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
    // than floaty drifts. Bowling-end is a fixed world lens — no follow.
    let k = (1.0 - (-9.0 * time.delta_secs()).exp()).clamp(0.0, 1.0);
    if rig.mode == CamMode::BowlingEnd {
        rig.pos = target_pos;
        rig.look = target_look;
        rig.fov = target_fov;
    } else {
        rig.pos = rig.pos.lerp(target_pos, k);
        rig.look = rig.look.lerp(target_look, k);
        rig.fov += (target_fov - rig.fov) * k;
    }

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
        assert!(
            look.y < 8.0,
            "look target should sit near field center: y={}",
            look.y
        );
        assert!(
            look.y.abs() > 2.0,
            "look target should not be at ground level"
        );
        assert!(
            look.z.abs() < 5.0,
            "look target should be near pitch center: z={}",
            look.z
        );
        assert!(fov >= 62.0, "wide lens for full-stadium framing: fov={fov}");
        let flat = Vec2::new(pos.x, pos.z);
        assert!(
            flat.length() > br + 60.0,
            "camera should sit well outside the boundary: {flat:?}"
        );
    }

    /// The 10 ft standoff is short enough that a slightly high aim point pushes
    /// the striker below the frame; a batting view must still show the batter.
    #[test]
    fn batting_lens_keeps_striker_in_frame() {
        let br = geometry::DEFAULT_BOUNDARY_RADIUS;
        let (pos, look, fov) = mode_view(CamMode::BattingEnd, None, br, None);

        // Depression of the optical axis below horizontal.
        let flat = Vec2::new(look.x - pos.x, look.z - pos.z).length();
        let depression = ((pos.y - look.y) / flat).atan();

        // Height of the optical axis where the striker stands, and the bottom
        // of the frame there (half the vertical FOV below the axis).
        let to_striker = Vec2::new(
            geometry::BATSMAN_POS.x - pos.x,
            geometry::BATSMAN_POS.y - pos.z,
        )
        .length();
        let axis_y = pos.y - to_striker * depression.tan();
        let frame_bottom = axis_y - to_striker * (fov.to_radians() * 0.5).tan();

        const STRIKER_HEAD_Y: f32 = 1.8;
        assert!(
            frame_bottom < STRIKER_HEAD_Y,
            "striker's head ({STRIKER_HEAD_Y} m) falls below the frame bottom ({frame_bottom})              at {to_striker} m; lower the aim point or widen the lens"
        );
    }

    #[test]
    /// Compared against the bowling-end telephoto: the batting lens sits only
    /// 10 ft off the striker, so it is necessarily wide (see BATTING_CAM_FOV_DEG).
    fn broadcast_wider_than_gameplay_telephoto() {
        let br = 65.0;
        let (_, _, tele_fov) = mode_view(CamMode::BowlingEnd, None, br, None);
        let (_, _, wide_fov) = mode_view(CamMode::Broadcast, None, br, None);
        assert!(wide_fov > tele_fov + 15.0);
    }

    #[test]
    fn batting_lens_behind_striker_at_twelve_ft_pole() {
        let br = geometry::DEFAULT_BOUNDARY_RADIUS;
        let (pos, look, fov) = mode_view(CamMode::BattingEnd, None, br, None);
        let batsman = geometry::BATSMAN_POS;
        assert!(
            (pos.y - BATTING_CAM_HEIGHT_M).abs() < 0.02,
            "batting cam height should be 12 ft: y={}",
            pos.y
        );
        assert!(
            (pos.x - (batsman.x + BATTING_CAM_BEHIND_BATSMAN_M)).abs() < 0.02,
            "batting cam should sit 10 ft behind batsman: x={}",
            pos.x
        );
        assert!(
            (pos.z - batsman.y).abs() < 0.05,
            "batting cam should track batsman lateral line: z={}",
            pos.z
        );
        assert!(
            look.x < batsman.x,
            "batting cam should look down the pitch at the bowler: look={look:?}"
        );
        assert!(
            (50.0..=70.0).contains(&fov),
            "over-the-shoulder read from 10 ft needs a wide lens: fov={fov}"
        );
    }

    #[test]
    fn broadcast_scales_with_stadium_size() {
        let small = broadcast_establishing_view(55.0).0;
        let large = broadcast_establishing_view(75.0).0;
        assert!(large.z > small.z);
        assert!(large.y > small.y);
    }

    #[test]
    fn bowling_lens_behind_bowler_stumps() {
        let br = geometry::DEFAULT_BOUNDARY_RADIUS;
        let (pos, look, fov) = mode_view(CamMode::BowlingEnd, None, br, None);
        let bowler_stumps_x = -geometry::PITCH_HALF_LEN;

        assert!(
            (pos.y - BOWLING_CAM_HEIGHT_M).abs() < 0.02,
            "bowling cam height should be 10 ft: y={}",
            pos.y
        );
        assert!(
            (pos.x - (bowler_stumps_x - BOWLING_CAM_SETBACK_M)).abs() < 0.02,
            "bowling cam should sit 25 ft behind bowler's stumps: x={}",
            pos.x
        );
        assert!(
            pos.x < bowler_stumps_x,
            "bowling cam should be behind the bowler's stumps (further -X): x={}",
            pos.x
        );
        assert!(
            pos.x < geometry::BATSMAN_POS.x,
            "bowling cam should be on the far side of the stumps from the striker: x={}",
            pos.x
        );
        assert!(
            pos.z.abs() < 0.05,
            "bowling cam should be centred on the pitch: z={}",
            pos.z
        );
        assert!(
            look.x > pos.x,
            "bowling cam should look down +X toward the batting end: pos={pos:?} look={look:?}"
        );
        assert!(
            (look.x - geometry::PITCH_HALF_LEN).abs() < 0.05,
            "bowling cam should aim at the striker's stumps: look={look:?}"
        );
        assert!(
            (look.y - geometry::STUMP_HEIGHT).abs() < 0.05,
            "bowling cam should aim at upper-stump height: look={look:?}"
        );
        assert!(
            (20.0..=30.0).contains(&fov),
            "25 ft standoff is a telephoto gameplay lens: fov={fov}"
        );
    }

    /// From 25 ft back the optical axis must land on the batsman and the striker
    /// must read as a person, not a speck above empty foreground.
    #[test]
    fn bowling_lens_keeps_striker_in_frame() {
        let br = geometry::DEFAULT_BOUNDARY_RADIUS;
        let (pos, look, fov) = mode_view(CamMode::BowlingEnd, None, br, None);

        let flat = Vec2::new(look.x - pos.x, look.z - pos.z).length();
        let depression = ((pos.y - look.y) / flat).atan();

        let batsman = geometry::BATSMAN_POS;
        let to_striker = Vec2::new(batsman.x - pos.x, batsman.y - pos.z).length();
        let axis_y = pos.y - to_striker * depression.tan();
        let half_fov = fov.to_radians() * 0.5;
        let frame_bottom = axis_y - to_striker * half_fov.tan();
        let frame_top = axis_y + to_striker * half_fov.tan();
        let frame_height = frame_top - frame_bottom;

        const STRIKER_FEET_Y: f32 = 0.0;
        const STRIKER_HEAD_Y: f32 = 1.8;

        assert!(
            (0.0..=3.0).contains(&axis_y),
            "optical axis should land on the batsman at striker distance, not below ground: \
             axis_y={axis_y} at {to_striker} m"
        );
        assert!(
            frame_bottom < STRIKER_FEET_Y && frame_top > STRIKER_HEAD_Y,
            "striker ({STRIKER_FEET_Y}–{STRIKER_HEAD_Y} m) should sit inside the frame \
             [{frame_bottom}, {frame_top}] at {to_striker} m"
        );
        let batsman_fraction = STRIKER_HEAD_Y / frame_height;
        assert!(
            (0.10..=0.35).contains(&batsman_fraction),
            "striker should read as a person (~10–35% of frame height), not a speck: \
             {batsman_fraction:.2} of {frame_height:.1} m at {to_striker} m"
        );
    }
}

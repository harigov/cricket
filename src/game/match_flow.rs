//! The match orchestrator: drives the delivery cycle
//! ready -> (aim) -> run-up -> ball live -> resolution -> between balls.
//!
//! Design notes:
//! * Score is applied **exactly once**, when the ball is finalised into a
//!   [`PhaseEnum::ResultPause`]. Immediate outcomes (bowled, dot, wide)
//!   finalise directly; struck-ball outcomes go through a scripted
//!   [`Pending`] that a watcher resolves when the visuals catch up.
//! * Ball physics keeps running during the result pause so the world never
//!   freezes mid-flight.
//! * All randomness comes from a small LCG so behaviour is reproducible
//!   in tests and replays.

use crate::core::geometry::{self as geo};
use crate::core::rules::{BallOutcome, Dismissal, Innings};
use crate::core::teams::{
    BowlStyle, Player, Team, all_bowlers_ranked, batting_order, pick_bowlers,
};
use crate::core::{
    Footwork, ShotKind, footwork_from_move_y, select_shot, shot_length_penalty, shot_profile,
};
use crate::game::ball::*;
use crate::game::fielding::{self, Brain, Fielder};
use crate::game::*;
use crate::input::{Action, PlayerInput};
use crate::render::camera_rig::{
    BallRecording, CamMode, CameraRig, PresentationState, ReplayState,
};
use crate::render::player::{
    Anim, AnimState, Figure, FigureKind, batter_stance_quat, face_target, face_target_quat,
    spawn_figure,
};
use crate::state::RebuildScene;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

/// Seconds per completed run between the wickets.
const RUN_SECONDS: f32 = 2.9;
/// How long results stay on screen.
const RESULT_PAUSE_SECS: f32 = 2.4;
/// Run-up duration in seconds.
const RUNUP_SECS: f32 = 1.7;
/// Bowler follow-through past the popping crease after release (metres).
const BOWLER_FOLLOW_THROUGH_X: f32 = 0.45;
/// Timing offset beyond which the batter is beaten (play and miss).
const BEATEN_TIMING_THRESHOLD: f32 = 0.27;
/// Perfect-contact tier: offset below this is middle of the bat.
const TIER_PERFECT_MAX: f32 = 0.055;
/// Good-contact tier upper bound.
const TIER_GOOD_MAX: f32 = 0.115;
/// Okay-contact tier upper bound (edge band starts above this).
const TIER_OKAY_MAX: f32 = 0.19;
/// Chance an edge carries to the keeper for a dismissal.
const EDGE_CARRY_CHANCE: f32 = 0.62;
/// The aim ring lies flat on the pitch, so it has to clear the pitch slab
/// (y = 0.05) and the worn strip (y = 0.06) or it is hidden underneath them.
const AIM_MARKER_Y: f32 = 0.075;
/// Grace before Confirm can signal readiness (avoids result-screen carry-over).
const READY_GATE_GRACE: f32 = 0.5;
/// Auto-start the run-up if the batter never signals ready.
const READY_GATE_TIMEOUT: f32 = 12.0;
/// Higher top-edge carry chance on mistimed aerial strokes.
const AERIAL_MISTIME_EDGE_CHANCE: f32 = 0.78;
/// Hard cap on the retrieval/return phase so a fielder who can never quite
/// reach the ball (deep in the outfield, an odd bounce, whatever) cannot
/// soft-lock the match waiting for a throw that will never come.
const BALL_RETURN_TIMEOUT: f32 = 6.0;
/// Seconds for the final return throw to cross from the fielder to the bowler.
const RETURN_THROW_SECS: f32 = 0.6;

// ---------------------------------------------------------------------------
// Resources tied to the live scene
// ---------------------------------------------------------------------------

/// Entities composing the current match scene.
#[derive(Resource)]
pub struct MatchScene {
    pub stadium_root: Entity,
    pub ball: Entity,
    pub bowler: Entity,
    pub striker: Entity,
    pub non_striker: Entity,
    pub fielders: Vec<Entity>,
    pub marker: Option<Entity>,
}

/// Field layout currently deployed.
#[derive(Resource)]
pub struct CurrentLayout(pub geo::FieldLayout);

#[derive(Component)]
pub struct MatchSceneRoot;

// ---------------------------------------------------------------------------
// RNG helpers (small deterministic LCG)
// ---------------------------------------------------------------------------

thread_local! {
    static RNG: std::cell::Cell<u64> =
        const { std::cell::Cell::new(0x9E37_79B9_7F4A_7C15) };
}

fn next_u64() -> u64 {
    RNG.with(|s| {
        let x = s.get();
        s.set(
            x.wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407),
        );
        x
    })
}

fn unit() -> f32 {
    ((next_u64() >> 33) % 100_000) as f32 / 100_000.0
}

fn gauss() -> f32 {
    // Box-Muller-ish cheap approximation: sum of 3 uniforms centred.
    (unit() + unit() + unit() - 1.5) * 1.15
}

fn coin(p: f32) -> bool {
    unit() < p
}

// ---------------------------------------------------------------------------
// Ball finalization context (resolve_at_bat -> finalize_ball)
// ---------------------------------------------------------------------------

struct BallResolutionCtx<'a, 'w, 's> {
    commands: &'a mut Commands<'w, 's>,
    recent: &'a mut RecentBalls,
    phase_enum: &'a mut PhaseEnum,
    am: &'a mut ActiveMatch,
}

impl BallResolutionCtx<'_, '_, '_> {
    fn finalize(&mut self, outcome: BallOutcome, text: String) {
        finalize_ball(
            self.commands,
            self.recent,
            self.phase_enum,
            self.am,
            outcome,
            text,
        );
    }
}

/// Commentary for bowled / wide / dot when no shot connects.
struct UnplayedCommentary {
    bowled: &'static str,
    wide: &'static str,
    dot: &'static str,
}

fn resolve_unplayed_ball(
    ctx: &mut BallResolutionCtx<'_, '_, '_>,
    bs: &mut BallState,
    plan: &DeliveryPlan,
    commentary: UnplayedCommentary,
) {
    bs.dead = true;
    if hits_stumps(bs) {
        ctx.finalize(
            BallOutcome::Wicket(Dismissal::Bowled),
            commentary.bowled.into(),
        );
    } else if plan.wide {
        ctx.finalize(BallOutcome::Wide, commentary.wide.into());
    } else {
        ctx.finalize(BallOutcome::Runs(0), commentary.dot.into());
    }
}

// ---------------------------------------------------------------------------
// Scene construction
// ---------------------------------------------------------------------------

pub fn spawn_match_scene(
    commands: &mut Commands,
    asset_server: &AssetServer,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    wd: &WorldData,
    am: &ActiveMatch,
) -> MatchScene {
    let stadium = &wd.stadiums[am.stadium];
    let bat_team = am.batting_team(wd);
    let fld_team = am.fielding_team(wd);
    let stadium_root = crate::render::stadium::build_stadium(
        commands,
        asset_server,
        meshes,
        materials,
        images,
        stadium,
        bat_team,
        fld_team,
    );

    commands.insert_resource(BoundaryRadius(stadium.boundary_radius()));

    // Ball (parked at the keeper until released) — emissive red for visibility.
    let ball_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.92, 0.18, 0.12),
        emissive: LinearRgba::new(2.8, 0.35, 0.15, 1.0),
        perceptual_roughness: 0.35,
        reflectance: 0.25,
        ..Default::default()
    });
    let ball = commands
        .spawn((
            CricketBall,
            Mesh3d(meshes.add(Sphere::new(BALL_RADIUS * 1.08))),
            MeshMaterial3d(ball_mat),
            Transform::from_xyz(geo::PITCH_HALF_LEN + 1.2, BALL_RADIUS, 0.0),
            Visibility::default(),
            BallState::default(),
            BallFlags::default(),
        ))
        .id();

    // Batters face the bowler; bowler faces the striker.
    let bowler_end = Vec2::new(-geo::PITCH_HALF_LEN, 0.0);
    let striker = spawn_figure(
        commands,
        asset_server,
        meshes,
        materials,
        images,
        Vec3::new(geo::BATSMAN_POS.x, 0.0, geo::BATSMAN_POS.y),
        face_target(geo::BATSMAN_POS, bowler_end),
        bat_team,
        FigureKind::Batter,
    );
    let non_striker_pos = Vec2::new(-geo::PITCH_HALF_LEN + 1.6, 0.9);
    let non_striker = spawn_figure(
        commands,
        asset_server,
        meshes,
        materials,
        images,
        Vec3::new(non_striker_pos.x, 0.0, non_striker_pos.y),
        face_target(non_striker_pos, bowler_end),
        bat_team,
        FigureKind::NonStriker,
    );

    let bowler_pos = Vec2::new(-geo::PITCH_HALF_LEN - 8.0, 0.35);
    let bowler = spawn_figure(
        commands,
        asset_server,
        meshes,
        materials,
        images,
        Vec3::new(bowler_pos.x, 0.0, bowler_pos.y),
        face_target(bowler_pos, geo::BATSMAN_POS),
        fld_team,
        FigureKind::Bowler,
    );

    // Fielding side.
    let layout = geo::FieldLayout::standard();
    let fielders = fielding::spawn_field_side(
        commands,
        asset_server,
        meshes,
        materials,
        images,
        &layout.positions,
        fld_team,
    );

    commands.insert_resource(CurrentLayout(layout));

    MatchScene {
        stadium_root,
        ball,
        bowler,
        striker,
        non_striker,
        fielders,
        marker: None,
    }
}

pub fn despawn_match_scene(commands: &mut Commands, scene: &MatchScene) {
    commands.entity(scene.stadium_root).despawn();
    for e in [
        &scene.ball,
        &scene.bowler,
        &scene.striker,
        &scene.non_striker,
    ] {
        commands.entity(*e).despawn();
    }
    for e in &scene.fielders {
        commands.entity(*e).despawn();
    }
    if let Some(m) = scene.marker {
        commands.entity(m).despawn();
    }
    commands.remove_resource::<MatchScene>();
}

/// Clear per-delivery transient state.
fn reset_delivery_resources(commands: &mut Commands) {
    commands.insert_resource(ShotAttempt::default());
    commands.insert_resource(Pending::default());
    commands.insert_resource(ReleaseInfo::default());
}

// ---------------------------------------------------------------------------
// Phase: MatchIntro (opening walk-on)
// ---------------------------------------------------------------------------

/// Grace before Confirm can skip the intro (avoids menu carry-over).
const MATCH_INTRO_SKIP_GRACE: f32 = 1.1;

/// Off-screen start on the boundary, along the ray from the pitch centre.
pub fn intro_walk_start(goal: Vec2, boundary_r: f32) -> Vec2 {
    let dist = goal.length();
    if dist < 1.0 {
        Vec2::new(boundary_r * 0.85, 0.0)
    } else {
        goal * (boundary_r * 0.88 / dist)
    }
}

/// 0..1 eased progress for the walk-in; finishes before the full intro ends.
pub fn match_intro_walk_progress(elapsed: f32, duration: f32) -> f32 {
    let walk_t = (duration * 0.82).clamp(1.4, (duration - 0.35).max(1.4));
    let u = (elapsed / walk_t).clamp(0.0, 1.0);
    1.0 - (1.0 - u) * (1.0 - u)
}

/// Striker and non-striker positions during the opening walk-on.
pub fn match_intro_batter_positions(elapsed: f32, duration: f32, boundary_r: f32) -> (Vec2, Vec2) {
    let striker_goal = geo::BATSMAN_POS;
    let non_striker_goal = Vec2::new(-geo::PITCH_HALF_LEN + 1.6, 0.9);
    let p = match_intro_walk_progress(elapsed, duration);
    let striker = intro_walk_start(striker_goal, boundary_r).lerp(striker_goal, p);
    let non_striker = intro_walk_start(non_striker_goal, boundary_r).lerp(non_striker_goal, p);
    (striker, non_striker)
}

pub fn match_intro_should_finish(elapsed: f32, duration: f32, skip_requested: bool) -> bool {
    elapsed >= duration || (skip_requested && elapsed >= MATCH_INTRO_SKIP_GRACE)
}

/// The striker signals readiness with Confirm, after a short settle beat.
/// Times out so an idle match still bowls the next ball.
pub fn ready_gate_should_release(elapsed: f32, confirm_pressed: bool) -> bool {
    elapsed >= READY_GATE_TIMEOUT || (confirm_pressed && elapsed >= READY_GATE_GRACE)
}

#[derive(SystemParam)]
pub(crate) struct MatchIntroParams<'w, 's> {
    am: Option<Res<'w, ActiveMatch>>,
    br: Option<Res<'w, BoundaryRadius>>,
    audio: Res<'w, crate::game::audio::AudioSettings>,
    durations: Option<Res<'w, crate::game::audio::CommentaryDurations>>,
    batters: Query<'w, 's, (&'static Figure, &'static mut Transform, &'static mut Anim)>,
    cam: ResMut<'w, CameraRig>,
}

pub fn sys_match_intro(
    mut commands: Commands,
    mut phase: ResMut<Phase>,
    time: Res<Time>,
    input: Res<PlayerInput>,
    mut scene: MatchIntroParams,
) {
    let PhaseEnum::MatchIntro { t } = &mut phase.0 else {
        return;
    };
    let Some(am) = scene.am.as_ref() else {
        return;
    };
    let boundary_r = scene.br.as_ref().map(|b| b.0).unwrap_or(65.0);
    let duration = scene
        .durations
        .as_ref()
        .map(|d| crate::game::audio::welcome_intro_duration_secs(scene.audio.commentary, d))
        .unwrap_or(2.6);

    *t += time.delta_secs();
    let elapsed = *t;
    let (striker_pos, non_striker_pos) =
        match_intro_batter_positions(elapsed, duration, boundary_r);
    let walk_p = match_intro_walk_progress(elapsed, duration);
    let bowler_end = Vec2::new(-geo::PITCH_HALF_LEN, 0.0);

    for (fig, mut tf, mut anim) in &mut scene.batters {
        let (pos, prev_goal) = match fig.kind {
            FigureKind::Batter => (striker_pos, geo::BATSMAN_POS),
            FigureKind::NonStriker => (non_striker_pos, Vec2::new(-geo::PITCH_HALF_LEN + 1.6, 0.9)),
            _ => continue,
        };
        tf.translation = Vec3::new(pos.x, 0.0, pos.y);
        if walk_p < 1.0 {
            let start = intro_walk_start(prev_goal, boundary_r);
            let move_dir = pos - start;
            if move_dir.length_squared() > 1e-6 {
                tf.rotation = face_target_quat(pos, pos + move_dir);
            }
            anim.state = AnimState::Run { t: elapsed * 1.4 };
        } else {
            tf.rotation = face_target_quat(pos, bowler_end);
            anim.state = AnimState::Idle;
        }
    }

    scene.cam.mode = CamMode::MatchIntro;

    if match_intro_should_finish(elapsed, duration, input.pressed(Action::Confirm)) {
        enter_ready(&mut commands, &mut phase.0);
        scene.cam.mode = if am.user_batting() {
            CamMode::BattingEnd
        } else {
            CamMode::BowlingEnd
        };
    }
}

// ---------------------------------------------------------------------------
// Phase: ReadyToBall
// ---------------------------------------------------------------------------

#[derive(SystemParam)]
pub(crate) struct ReadySceneParams<'w, 's> {
    am: Option<Res<'w, ActiveMatch>>,
    layout: Option<Res<'w, CurrentLayout>>,
    players: Query<
        'w,
        's,
        (
            &'static Figure,
            Option<&'static Fielder>,
            &'static mut Transform,
            &'static mut Anim,
        ),
        Without<CricketBall>,
    >,
    // Parked here (bug: a stale ball from the previous delivery would
    // otherwise sit wherever it last came to rest, visible through the
    // whole Ready/Aim phase until `sys_runup`'s first frame finally moved
    // it — the "phantom ball" players reported).
    ball_q: Query<'w, 's, (&'static mut BallState, &'static mut Transform), With<CricketBall>>,
    cam: ResMut<'w, CameraRig>,
}

pub fn sys_ready(
    mut phase: ResMut<Phase>,
    time: Res<Time>,
    input: Res<PlayerInput>,
    mut scene: ReadySceneParams,
) {
    let PhaseEnum::ReadyToBall { t } = &mut phase.0 else {
        return;
    };
    let Some(am) = scene.am.as_ref() else {
        return;
    };
    let Some(layout) = scene.layout.as_ref() else {
        return;
    };
    *t += time.delta_secs();

    // Park everyone at their posts. One query: fielders also carry Figure,
    // so splitting Figure+Transform from Fielder+Transform triggers B0001.
    let bowler_end = Vec2::new(-geo::PITCH_HALF_LEN, 0.0);
    for (fig, fielder, mut tf, mut anim) in &mut scene.players {
        anim.state = AnimState::Idle;
        match fig.kind {
            FigureKind::Batter => {
                tf.translation = Vec3::new(geo::BATSMAN_POS.x, 0.0, geo::BATSMAN_POS.y);
                tf.rotation = batter_stance_quat(geo::BATSMAN_POS, bowler_end);
            }
            FigureKind::NonStriker => {
                let pos = Vec2::new(-geo::PITCH_HALF_LEN + 1.6, 0.9);
                tf.translation = Vec3::new(pos.x, 0.0, pos.y);
                tf.rotation = face_target_quat(pos, bowler_end);
            }
            FigureKind::Bowler => {
                let pos = Vec2::new(-geo::PITCH_HALF_LEN - 8.0, 0.35);
                tf.translation = Vec3::new(pos.x, 0.0, pos.y);
                tf.rotation = face_target_quat(pos, geo::BATSMAN_POS);
            }
            _ => {}
        }
        if let Some(f) = fielder
            && let Some(fp) = layout.0.positions.get(f.slot)
        {
            let p = fp.world_pos(geo::BATSMAN_POS);
            tf.translation.x = p.x;
            tf.translation.y = 0.0;
            tf.translation.z = p.y;
            tf.rotation = face_target_quat(p, geo::BATSMAN_POS);
        }
    }

    if std::env::var("CRICKET_CAMDEBUG").is_ok() {
        for (fig, fielder, tf, _a) in &scene.players {
            if fielder.is_none() {
                info!("FIGDEBUG {:?} at {:?}", fig.kind, tf.translation);
            }
        }
    }

    // Keep the ball pinned in the bowler's hand for the whole Ready/Aim
    // wait — otherwise it stays wherever the last delivery ended up until
    // the run-up's first frame snaps it into place.
    let bowler_pos = Vec2::new(-geo::PITCH_HALF_LEN - 8.0, 0.35);
    if let Ok((mut bs, mut tf)) = scene.ball_q.single_mut() {
        let parked = Vec3::new(bowler_pos.x - 0.3, 1.6, bowler_pos.y + 0.25);
        *bs = BallState {
            pos: parked,
            vel: Vec3::ZERO,
            dead: true,
            bounced: false,
            struck: false,
        };
        tf.translation = parked;
    }

    scene.cam.mode = if am.user_batting() {
        CamMode::BattingEnd
    } else {
        CamMode::BowlingEnd
    };

    if am.user_bowling() {
        if input.pressed(Action::Confirm) {
            phase.0 = PhaseEnum::AimLength {
                t: 0.0,
                lock: None,
                line: None,
            };
        }
    } else if am.user_batting() {
        if ready_gate_should_release(*t, input.pressed(Action::Confirm)) {
            phase.0 = PhaseEnum::RunUp { p: 0.0 };
        }
    } else if *t > 0.9 {
        // AI vs AI: brief beat, then automatic run-in.
        phase.0 = PhaseEnum::RunUp { p: 0.0 };
    }
}

// ---------------------------------------------------------------------------
// Delivery variation (human parity with the AI's ai_plan variety)
// ---------------------------------------------------------------------------

/// A named delivery variation the bowler can pick before running in. The
/// options offered depend on `BowlStyle` — see `variations_for`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DeliveryVariation {
    #[default]
    Stock,
    Slower,
    Bouncer,
    Yorker,
    Outswing,
    Inswing,
    ArmBall,
    Topspin,
    /// Off-spinner's wrong'un (turns the "wrong" way, like a leg-break).
    Doosra,
    /// Leg-spinner's skidding wrong'un (the leg-spin equivalent of a doosra).
    Slider,
}

/// The variation menu appropriate to a bowling style, in cycle order.
pub fn variations_for(style: BowlStyle) -> &'static [DeliveryVariation] {
    use DeliveryVariation::*;
    match style {
        BowlStyle::Fast | BowlStyle::FastMedium | BowlStyle::Medium => {
            &[Stock, Slower, Bouncer, Yorker, Outswing, Inswing]
        }
        BowlStyle::OffSpin => &[Stock, ArmBall, Topspin, Doosra],
        BowlStyle::LegSpin => &[Stock, ArmBall, Topspin, Slider],
    }
}

/// HUD/label text for a variation.
pub fn variation_label(v: DeliveryVariation) -> &'static str {
    match v {
        DeliveryVariation::Stock => "Stock",
        DeliveryVariation::Slower => "Slower ball",
        DeliveryVariation::Bouncer => "Bouncer",
        DeliveryVariation::Yorker => "Yorker",
        DeliveryVariation::Outswing => "Outswinger",
        DeliveryVariation::Inswing => "Inswinger",
        DeliveryVariation::ArmBall => "Arm ball",
        DeliveryVariation::Topspin => "Topspinner",
        DeliveryVariation::Doosra => "Doosra",
        DeliveryVariation::Slider => "Slider",
    }
}

/// Cycle to the next variation available for `style`, wrapping around.
/// If `current` isn't in that style's menu (e.g. the bowler changed),
/// falls back to the first entry.
pub fn next_variation(style: BowlStyle, current: DeliveryVariation) -> DeliveryVariation {
    let opts = variations_for(style);
    let idx = opts.iter().position(|&v| v == current).unwrap_or(0);
    opts[(idx + 1) % opts.len()]
}

/// Where in the length-oscillation window (centre, amplitude) a variation
/// aims, in the same `length_from_stumps` units as `build_plan` — small is
/// full (yorker territory), large is short (bouncer territory).
fn variation_length_window(variation: DeliveryVariation) -> (f32, f32) {
    match variation {
        DeliveryVariation::Bouncer => (12.0, 1.6),
        DeliveryVariation::Yorker => (3.0, 1.2),
        DeliveryVariation::Topspin
        | DeliveryVariation::Doosra
        | DeliveryVariation::Slider
        | DeliveryVariation::ArmBall => (7.5, 3.0),
        _ => (8.0, 5.5), // Stock / Slower / Outswing / Inswing: the classic full sweep
    }
}

/// How much harder a variation is to land accurately, scaling the existing
/// skill-based scatter in `sys_aim`. A yorker or bouncer from a poor bowler
/// should sometimes end up a full toss or a wide; a stock ball shouldn't.
fn variation_difficulty(variation: DeliveryVariation) -> f32 {
    match variation {
        DeliveryVariation::Stock => 1.0,
        DeliveryVariation::Slower => 1.15,
        DeliveryVariation::Outswing | DeliveryVariation::Inswing => 1.2,
        DeliveryVariation::ArmBall => 1.15,
        DeliveryVariation::Bouncer => 1.35,
        DeliveryVariation::Topspin => 1.25,
        DeliveryVariation::Doosra | DeliveryVariation::Slider => 1.4,
        DeliveryVariation::Yorker => 1.5, // hardest to land: full and fast
    }
}

/// The current variation selection while aiming, reset to `Stock` at the
/// start of every human bowling turn (see `sys_ready`).
#[derive(Resource, Default, Clone, Copy)]
pub struct AimVariation(pub DeliveryVariation);

/// 0..1 oscillating pace-meter value used for both the live marker and the
/// HUD readout, so they never fall out of sync.
pub fn aim_pace_value(t: f32) -> f32 {
    (t * 1.6).sin() * 0.5 + 0.5
}

// ---------------------------------------------------------------------------
// Phase: AimLength (user bowling)
// ---------------------------------------------------------------------------

pub fn sys_aim(
    mut phase: ResMut<Phase>,
    input: Res<PlayerInput>,
    time: Res<Time>,
    am: Option<Res<ActiveMatch>>,
    wd: Option<Res<WorldData>>,
    scene: Option<ResMut<MatchScene>>,
    mut variation: ResMut<AimVariation>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut marker_q: Query<&mut Transform, With<AimMarker>>,
) {
    let PhaseEnum::AimLength { t, lock, line } = &mut phase.0 else {
        return;
    };
    let (Some(am), Some(wd), Some(scene)) = (am, wd, scene) else {
        return;
    };
    let scene = scene.into_inner();
    *t += time.delta_secs();

    let bowler = am.bowler(&wd);
    let style = bowler.style.unwrap_or(BowlStyle::Medium);
    let skill = bowler.bowling as f32 / 100.0;

    if scene.marker.is_none() {
        let ring = meshes.add(Torus::new(0.38, 0.55));
        let e = commands
            .spawn((
                AimMarker,
                Mesh3d(ring),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::srgba(0.98, 0.82, 0.18, 0.72),
                    emissive: LinearRgba::new(1.6, 1.1, 0.2, 1.0),
                    alpha_mode: AlphaMode::Blend,
                    unlit: true,
                    ..Default::default()
                })),
                Transform::from_xyz(-5.0, AIM_MARKER_Y, 0.0),
                Visibility::default(),
            ))
            .id();
        scene.marker = Some(e);
        // Fresh aim: start from the bowler's stock ball every time.
        variation.0 = DeliveryVariation::Stock;
    }

    match (*lock, *line) {
        (None, _) => {
            // Stage 1: pick a variation (free, no lock needed) and lock the length.
            if input.pressed(Action::CycleType) {
                variation.0 = next_variation(style, variation.0);
            }
            let (center, amp) = variation_length_window(variation.0);
            let length = (center + t.sin() * amp).max(1.5);
            if let Some(e) = scene.marker
                && let Ok(mut tf) = marker_q.get_mut(e)
            {
                tf.translation.x = geo::PITCH_HALF_LEN - length;
                tf.translation.z = 0.0;
            }
            if input.pressed(Action::Confirm) {
                *lock = Some(length);
            }
        }
        (Some(locked_len), None) => {
            // Stage 2: sweep for line.
            let sweep = (*t * 1.4).sin() * 1.25;
            if let Some(e) = scene.marker
                && let Ok(mut tf) = marker_q.get_mut(e)
            {
                tf.translation.x = geo::PITCH_HALF_LEN - locked_len;
                tf.translation.z = sweep;
            }
            if input.pressed(Action::Confirm) {
                *line = Some(sweep);
            }
        }
        (Some(locked_len), Some(locked_line)) => {
            // Stage 3: oscillating pace meter, then release.
            if let Some(e) = scene.marker
                && let Ok(mut tf) = marker_q.get_mut(e)
            {
                tf.translation.x = geo::PITCH_HALF_LEN - locked_len;
                tf.translation.z = locked_line;
            }
            if input.pressed(Action::Confirm) {
                let pace = aim_pace_value(*t);
                let scatter = ((1.0 - skill) * 0.5 + 0.08) * variation_difficulty(variation.0);
                let plan = build_plan(
                    style,
                    locked_line + gauss() * scatter,
                    (locked_len + gauss() * scatter * 2.5).max(1.5),
                    variation.0,
                    pace,
                    skill,
                );
                commands.insert_resource(CurrentDelivery(Some(plan)));
                if let Some(e) = scene.marker.take() {
                    commands.entity(e).despawn();
                }
                phase.0 = PhaseEnum::RunUp { p: 0.0 };
            }
        }
    }
}

/// Build a DeliveryPlan from style, aimed line/length, chosen variation and
/// pace-meter fraction (0 = holding back, 1 = full effort).
pub fn build_plan(
    style: BowlStyle,
    line_z: f32,
    length_from_stumps: f32,
    variation: DeliveryVariation,
    pace_frac: f32,
    skill: f32,
) -> DeliveryPlan {
    // Pace envelope as a fraction of the style's nominal top speed. `0.72`
    // is the long-standing playability baseline (see history); a better
    // bowler can push meaningfully harder off the same run-up, but nobody
    // can exceed their style's `base_speed()` — that's the hard ceiling
    // applied at the end.
    let skill = skill.clamp(0.0, 1.0);
    let min_mul = 0.72;
    let max_mul = 0.72 + 0.22 * skill;
    let speed_mul = min_mul + (max_mul - min_mul) * pace_frac.clamp(0.0, 1.0);
    let mut speed = style.base_speed() * speed_mul;

    let mut swing: f32 = match style {
        BowlStyle::Fast | BowlStyle::FastMedium => 0.9,
        BowlStyle::Medium => 0.4,
        BowlStyle::OffSpin => 0.15,
        BowlStyle::LegSpin => -0.15,
    };
    let mut turn: f32 = match style {
        BowlStyle::OffSpin => 2.4,
        BowlStyle::LegSpin => -2.6,
        _ => 0.0,
    };

    match variation {
        DeliveryVariation::Stock => {}
        DeliveryVariation::Slower => speed *= 0.78,
        DeliveryVariation::Bouncer => swing *= 0.5,
        DeliveryVariation::Yorker => {
            swing *= 0.6;
            speed *= 1.02;
        }
        DeliveryVariation::Outswing => swing = swing.abs() * 1.6,
        DeliveryVariation::Inswing => swing = -swing.abs() * 1.6,
        DeliveryVariation::ArmBall => turn *= 0.15, // skids on, barely turns
        DeliveryVariation::Topspin => {
            turn *= 0.35; // extra dip/bounce, less lateral turn
            speed *= 0.95;
        }
        DeliveryVariation::Doosra => turn = -turn.abs() * 1.3, // turns the "wrong" way
        DeliveryVariation::Slider => {
            turn = turn.abs() * 0.2; // skids straight on
            speed *= 1.05;
        }
    }
    // Never exceed what the style/bowler realistically bowl at, whatever
    // the pace meter + variation multipliers work out to.
    speed = speed.min(style.base_speed());

    let label = if variation == DeliveryVariation::Stock {
        style.label().to_string()
    } else {
        variation_label(variation).to_string()
    };

    DeliveryPlan {
        speed,
        line_z,
        length_from_stumps,
        swing,
        turn,
        label,
        wide: line_z.abs() > 1.35,
    }
}

/// AI bowler plan with skill-based scatter and situational variety, using
/// the same variation menu the human player gets.
fn ai_plan(bowler: &Player, pitch: crate::core::stadiums::PitchType) -> DeliveryPlan {
    let style = bowler.style.unwrap_or(BowlStyle::FastMedium);
    let skill = bowler.bowling as f32 / 100.0;
    let scatter = (1.25 - skill) * 0.55;
    // Line: usually tight, occasionally wider to set up
    let line = gauss() * 0.32 * scatter.max(0.25);

    let variation = ai_pick_variation(style);
    let (center, amp) = variation_length_window(variation);
    let length = (center
        + gauss() * amp * 0.5
        + match pitch {
            crate::core::stadiums::PitchType::Green => -0.35,
            crate::core::stadiums::PitchType::Dusty => 0.45,
            _ => 0.0,
        })
    .clamp(2.0, 14.0);

    // AI mostly bowls at a brisk, slightly varied pace off a skill-scaled ceiling.
    let pace_frac = (0.55 + unit() * 0.4).clamp(0.0, 1.0);
    let mut plan = build_plan(style, line, length, variation, pace_frac, skill);

    // Extra swing on green tops occasionally, on top of the outswing/inswing bias.
    if matches!(style, BowlStyle::Fast | BowlStyle::FastMedium)
        && pitch == crate::core::stadiums::PitchType::Green
        && unit() < 0.32
    {
        plan.swing *= 1.7;
    }
    plan.wide = plan.line_z.abs() > 1.35;
    plan
}

/// Pick a delivery variation for the AI bowler, weighted so a stock ball is
/// still the most common choice (mirrors the old length-distribution rolls:
/// ~13% slower balls for seamers, ~9% yorkers, ~11% bouncers, occasional
/// spin variations).
fn ai_pick_variation(style: BowlStyle) -> DeliveryVariation {
    use DeliveryVariation::*;
    let roll = unit();
    if style.is_spin() {
        if roll < 0.55 {
            Stock
        } else if roll < 0.75 {
            ArmBall
        } else if roll < 0.90 {
            Topspin
        } else if style == BowlStyle::OffSpin {
            Doosra
        } else {
            Slider
        }
    } else if roll < 0.55 {
        Stock
    } else if roll < 0.68 {
        Slower
    } else if roll < 0.80 {
        Bouncer
    } else if roll < 0.90 {
        Yorker
    } else if roll < 0.95 {
        Outswing
    } else {
        Inswing
    }
}

// ---------------------------------------------------------------------------
// Phase: RunUp
// ---------------------------------------------------------------------------

/// Ease-in approach then a planted delivery stride so the bowler accelerates
/// into the run-up and decelerates through the release rather than gliding.
fn bowler_runup_x(p: f32) -> f32 {
    let start_x = -geo::PITCH_HALF_LEN - 8.0;
    let plant_x = geo::RELEASE_POINT.x - 0.55;
    let release_x = geo::RELEASE_POINT.x - 0.15;
    if p < 0.7 {
        let t = (p / 0.7).powi(2);
        start_x + (plant_x - start_x) * t
    } else {
        let t = ((p - 0.7) / 0.3).clamp(0.0, 1.0);
        let ease = 1.0 - (1.0 - t).powi(2);
        plant_x + (release_x - plant_x) * ease
    }
}

/// Predict arrival at the bat plane in **game** seconds (matches scaled physics).
fn estimate_arrival_at_bat(plan: &DeliveryPlan, start: Vec3) -> f32 {
    let bounce_x = geo::PITCH_HALF_LEN - plan.length_from_stumps;
    let dx_pre = (bounce_x - start.x).max(1.0);
    let t_pre = dx_pre / plan.speed.max(0.1);
    let dx_post = (BAT_PLANE_X - bounce_x).max(0.5);
    let t_post = dx_post / (plan.speed.max(0.1) * 0.82);
    t_pre + 0.14 + t_post
}

#[derive(SystemParam)]
pub(crate) struct RunupParams<'w, 's> {
    del: Res<'w, CurrentDelivery>,
    am: Option<Res<'w, ActiveMatch>>,
    wd: Option<Res<'w, WorldData>>,
    figs: Query<
        'w,
        's,
        (&'static Figure, &'static mut Transform, &'static mut Anim),
        Without<CricketBall>,
    >,
    ball_q: Query<'w, 's, (&'static mut BallState, &'static mut Transform), With<CricketBall>>,
    commands: Commands<'w, 's>,
    _scene: Option<ResMut<'w, MatchScene>>,
}

pub fn sys_runup(mut phase: ResMut<Phase>, time: Res<Time>, mut runup: RunupParams) {
    let PhaseEnum::RunUp { p } = &mut phase.0 else {
        return;
    };
    *p += time.delta_secs() / RUNUP_SECS;
    let p = *p;

    // Bowler jogs in; delivery stride over the last 30%.
    // Keep the mocap run clip through the whole approach: switching to procedural
    // `BowlAction` here stops the clip and the deep knee bend buries the feet
    // ~1 m below the pitch (see `bowler_runup_keeps_mocap_run` test note).
    for (fig, mut tf, mut anim) in &mut runup.figs {
        if fig.kind == FigureKind::Bowler {
            let pc = p.clamp(0.0, 1.0);
            tf.translation.x = bowler_runup_x(pc);
            tf.translation.y = 0.0;
            tf.rotation = face_target_quat(
                Vec2::new(tf.translation.x, tf.translation.z),
                geo::BATSMAN_POS,
            );
            anim.state = AnimState::Run { t: pc * 4.0 };
        }
    }

    // Ball rides in the bowler's hand until release.
    if p < 1.0 {
        if let Ok((mut bs, mut tf)) = runup.ball_q.single_mut() {
            for (fig, tf_bowler, _) in &runup.figs {
                if fig.kind == FigureKind::Bowler {
                    bs.pos = Vec3::new(
                        tf_bowler.translation.x - 0.3,
                        1.6 + (p * 20.0).sin().abs() * 0.2,
                        tf_bowler.translation.z + 0.25,
                    );
                    tf.translation = bs.pos;
                }
            }
        }
        return;
    }

    // ---- RELEASE ----
    let (Some(am), Some(wd)) = (runup.am.as_ref(), runup.wd.as_ref()) else {
        return;
    };
    let plan = runup
        .del
        .0
        .clone()
        .unwrap_or_else(|| ai_plan(am.bowler(wd), am.pitch(wd)));
    runup
        .commands
        .insert_resource(CurrentDelivery(Some(plan.clone())));

    let start = Vec3::new(
        geo::RELEASE_POINT.x,
        geo::RELEASE_POINT.y,
        geo::RELEASE_POINT.z + plan.line_z * 0.35,
    );
    // Solve initial velocity so the ball pitches at the intended spot
    // (works in scaled time, matching the physics integrator).
    let bounce_x = geo::PITCH_HALF_LEN - plan.length_from_stumps;
    let dx = (bounce_x - start.x).max(1.0);
    let ts = dx / plan.speed; // scaled seconds to the bounce
    let g_scaled = 9.81;
    let vy = (BALL_RADIUS - start.y + 0.5 * g_scaled * ts * ts) / ts;
    let vz = (plan.line_z - start.z) / ts;
    let vel = Vec3::new(plan.speed, vy, vz);

    if let Ok((mut bs, mut tf)) = runup.ball_q.single_mut() {
        *bs = BallState::new_release(start, vel);
        tf.translation = bs.pos;
    }

    // Carry through past the crease; procedural settle eases back to idle.
    for (fig, mut tf, mut anim) in &mut runup.figs {
        if fig.kind == FigureKind::Bowler {
            tf.translation.x = geo::RELEASE_POINT.x + BOWLER_FOLLOW_THROUGH_X;
            tf.translation.y = 0.0;
            anim.state = AnimState::BowlSettle { t: 0.0 };
        }
    }

    let est_t_arrive = estimate_arrival_at_bat(&plan, start);
    runup.commands.insert_resource(ReleaseInfo {
        active: true,
        resolved: false,
        t: 0.0,
        t_arrive: est_t_arrive,
    });
    info!(
        "BALL RELEASED: arrive ~{:.0}s speed {:.0}",
        est_t_arrive, plan.speed
    );

    phase.0 = PhaseEnum::BallLive;
}

// ---------------------------------------------------------------------------
// Ball physics (steps during live play and the result pause)
// ---------------------------------------------------------------------------

pub fn sys_ball_physics(
    phase: Res<Phase>,
    time: Res<Time>,
    del: Res<CurrentDelivery>,
    am: Option<Res<ActiveMatch>>,
    wd: Option<Res<WorldData>>,
    mut q: Query<(&mut BallState, &mut BallFlags, &mut Transform), With<CricketBall>>,
) {
    if !matches!(phase.0, PhaseEnum::BallLive | PhaseEnum::ResultPause { .. }) {
        return;
    }
    let Ok((mut bs, mut flags, mut tf)) = q.single_mut() else {
        return;
    };

    let settled = bs.dead && bs.pos.y <= BALL_RADIUS * 1.5 && bs.vel.length_squared() < 0.02;
    if settled {
        return;
    }

    let pitch = am
        .as_ref()
        .and_then(|a| wd.as_ref().map(|w| a.pitch(w)))
        .unwrap_or(crate::core::stadiums::PitchType::Hard);

    if !bs.dead {
        if let Some(plan) = del.0.as_ref() {
            physics_step_with_pitch(
                &mut bs,
                &mut flags,
                plan.swing,
                plan.turn,
                time.delta_secs(),
                Some(pitch),
            );
        }
    } else {
        // Dead ball still rolls/settles without delivery forces.
        let mut f = BallFlags::default();
        physics_step_with_pitch(&mut bs, &mut f, 0.0, 0.0, time.delta_secs(), Some(pitch));
    }
    tf.translation = bs.pos;
}

// ---------------------------------------------------------------------------
// Shot input capture (human batter + AI batter scheduling)
// ---------------------------------------------------------------------------

#[derive(SystemParam)]
pub(crate) struct ShotInputParams<'w, 's> {
    am: Option<Res<'w, ActiveMatch>>,
    wd: Option<Res<'w, WorldData>>,
    del: Res<'w, CurrentDelivery>,
    rel: ResMut<'w, ReleaseInfo>,
    attempt: ResMut<'w, ShotAttempt>,
    batters: Query<'w, 's, (&'static Figure, &'static mut Anim)>,
}

/// AI batter inputs for footwork, aim and loft from line, length and skill.
pub fn ai_batting_inputs(
    plan: &DeliveryPlan,
    skill: f32,
    agg: f32,
    quality: f32,
    defend_bias: f32,
    swing_prob: f32,
    rng_unit: impl Fn() -> f32,
    rng_coin: impl Fn(f32) -> bool,
) -> Option<(Footwork, f32, bool)> {
    if !rng_coin(swing_prob) {
        return None;
    }
    if plan.wide {
        return None;
    }
    if quality > 0.75 && agg < 0.6 && rng_coin(defend_bias) {
        return Some((Footwork::Planted, 0.0, false));
    }

    let len = plan.length_from_stumps;
    let line = plan.line_z;
    let mut footwork = if len > 11.0 {
        Footwork::Back
    } else if len < 4.5 {
        Footwork::Front
    } else if line > 0.42 {
        Footwork::Back
    } else {
        // Everything that isn't short, wide-of-off or angling across is met
        // forward — the two branches this replaces both said so.
        Footwork::Front
    };
    let mut aim = if line > 0.45 {
        0.58
    } else if line < -0.30 {
        -0.52
    } else if len < 4.5 {
        0.08
    } else if len > 11.0 {
        -0.62
    } else {
        0.0
    };
    let loft = if len > 11.0 {
        rng_coin((0.35 + agg * 0.45).clamp(0.1, 0.85))
    } else if (4.5..=11.0).contains(&len) {
        rng_coin((agg * 0.45 * (1.25 - quality)).clamp(0.05, 0.88))
    } else {
        rng_coin((agg * 0.35).clamp(0.05, 0.55))
    };

    // Better batters pick length-appropriate footwork more often.
    let noise = (1.0 - skill) * 0.42;
    aim = (aim + (rng_unit() * 2.0 - 1.0) * (0.28 + noise)).clamp(-1.0, 1.0);
    if rng_unit() < noise * 0.35 {
        footwork = match footwork {
            Footwork::Front => Footwork::Back,
            Footwork::Back => Footwork::Front,
            Footwork::Planted => {
                if len > 9.0 {
                    Footwork::Back
                } else {
                    Footwork::Front
                }
            }
        };
    }
    if skill > 0.72 && rng_unit() < skill * 0.55 {
        footwork = if len > 10.5 {
            Footwork::Back
        } else if len < 5.0 {
            Footwork::Front
        } else {
            footwork
        };
        if len > 10.5 {
            aim = aim.min(-0.55);
        } else if len < 5.0 {
            aim = aim.clamp(-0.2, 0.45);
        }
    }

    Some((footwork, aim, loft))
}

pub fn sys_shot_input(
    phase: ResMut<Phase>,
    time: Res<Time>,
    input: Res<PlayerInput>,
    mut shot: ShotInputParams,
) {
    let PhaseEnum::BallLive = phase.0 else { return };
    if shot.am.is_none() {
        return;
    }
    if !shot.rel.active || shot.rel.resolved {
        return;
    }
    shot.rel.t += time.delta_secs() * BALL_TIME_SCALE;
    let Some(plan) = shot.del.0.as_ref() else {
        return;
    };

    let user_batting = shot
        .am
        .as_ref()
        .map(|am| am.user_batting())
        .unwrap_or(false);

    if user_batting && input.pressed(Action::Confirm) {
        let footwork = footwork_from_move_y(input.move_vec.y);
        let loft = input.held(Action::Loft);
        let aim = input.move_vec.x.clamp(-1.0, 1.0);
        let kind = select_shot(footwork, aim, loft);
        for (fig, mut anim) in &mut shot.batters {
            if fig.kind == FigureKind::Batter {
                anim.state = AnimState::BatShot { p: 0.0, shot: kind };
            }
        }
        if !shot.attempt.pressed {
            shot.attempt.pressed = true;
            let offset = shot.rel.t - shot.rel.t_arrive;
            shot.attempt.offset = Some(offset);
            shot.attempt.loft = loft;
            shot.attempt.dir_x = aim;
            shot.attempt.footwork = footwork;
            shot.attempt.kind = kind;
            info!(
                "SHOT registered: {:?} offset {:.3}s loft={} aim={:.2}",
                kind, offset, loft, aim
            );
        }
    } else if let (Some(am), Some(wd)) = (shot.am.as_ref(), shot.wd.as_ref())
        && !user_batting
        && !shot.attempt.ai_scheduled
        && shot.rel.t > shot.rel.t_arrive - 0.45
    {
        // `!user_batting` matters: without it, a human batter who simply
        // leaves the ball (never presses Confirm) had this AI-swing
        // scheduler fire on their behalf once the arrival window opened,
        // silently playing a shot they never asked for — indistinguishable
        // from "the ball hit me even though I didn't swing".
        shot.attempt.ai_scheduled = true;
        let batsman = am.striker(wd);
        let q = plan.quality_vs_batsman();
        let skill = batsman.batting as f32 / 100.0;
        let length_factor = if plan.length_from_stumps < 4.5 {
            0.04
        } else if plan.length_from_stumps > 11.0 {
            0.025
        } else {
            0.0
        };
        let sigma =
            (0.045 + (1.0 - q) * 0.10 - (skill - 0.7) * 0.04 + length_factor).clamp(0.028, 0.30);
        let agg = chase_pressure(
            am.state.innings.target,
            am.state.innings.runs,
            am.state.innings.legal_balls,
            am.state.overs,
        );
        let defend_bias = if q > 0.75 && agg < 0.6 { 0.18 } else { 0.0 };
        let swing_prob = (0.58 + agg * 0.38 - q * 0.32 - defend_bias).clamp(0.18, 0.96);
        if let Some((footwork, aim, loft)) =
            ai_batting_inputs(plan, skill, agg, q, defend_bias, swing_prob, unit, coin)
        {
            shot.attempt.pressed = true;
            shot.attempt.offset = Some((gauss() * sigma).clamp(-0.5, 0.5));
            shot.attempt.loft = loft;
            shot.attempt.dir_x = aim;
            shot.attempt.footwork = footwork;
            shot.attempt.kind = select_shot(footwork, aim, loft);
        }
    }
}

/// Required run rate pressure 0..1 for the chasing side.
pub(crate) fn chase_pressure(target: Option<u32>, runs: u32, balls: u32, overs: u32) -> f32 {
    match target {
        None => 0.55,
        Some(t) => {
            let need = t.saturating_sub(runs) as f32;
            let total_balls = overs * 6;
            let overs_left = (total_balls.saturating_sub(balls) as f32 / 6.0).max(0.5);
            let rrr = need / overs_left;
            (rrr / 12.0).clamp(0.3, 1.0)
        }
    }
}

// ---------------------------------------------------------------------------
// Bat-plane resolution
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
enum Tier {
    Perfect,
    Good,
    Okay,
    Edge,
}

/// Watch for the ball crossing the bat plane and resolve the outcome.
#[derive(SystemParam)]
pub(crate) struct ContactWatchParams<'w, 's> {
    commands: Commands<'w, 's>,
    phase: ResMut<'w, Phase>,
    am: Option<ResMut<'w, ActiveMatch>>,
    wd: Option<Res<'w, WorldData>>,
    rel: ResMut<'w, ReleaseInfo>,
    attempt: Res<'w, ShotAttempt>,
    del: Res<'w, CurrentDelivery>,
    br: Res<'w, BoundaryRadius>,
    recent: ResMut<'w, RecentBalls>,
    ball_q: Query<'w, 's, (&'static mut BallState, &'static mut BallFlags), With<CricketBall>>,
    gts: Query<'w, 's, (&'static Fielder, &'static GlobalTransform)>,
    layout: Option<Res<'w, CurrentLayout>>,
    chasers: Query<'w, 's, (Entity, &'static Fielder, &'static mut Brain)>,
}

pub fn sys_contact_watch(mut watch: ContactWatchParams) {
    let PhaseEnum::BallLive = watch.phase.0 else {
        return;
    };
    let (Some(am), Some(wd), Some(layout)) =
        (watch.am.as_mut(), watch.wd.as_ref(), watch.layout.as_ref())
    else {
        return;
    };
    if !watch.rel.active || watch.rel.resolved {
        return;
    }
    let Ok((mut bs, flags)) = watch.ball_q.single_mut() else {
        return;
    };
    if !flags.crossed_bat_plane {
        return;
    }
    watch.rel.resolved = true;
    let Some(plan) = watch.del.0.as_ref().cloned() else {
        return;
    };

    // Snapshot fielder post positions indexed by slot.
    let fielder_pos = fielding::positions_by_slot(
        watch
            .gts
            .iter()
            .map(|(f, g)| (f.slot, Vec2::new(g.translation().x, g.translation().z))),
        layout.0.positions.len(),
    );

    let batting_skill = am.striker(wd).batting as f32 / 100.0;
    let chaser_slot = resolve_at_bat(
        &mut watch.commands,
        &mut watch.recent,
        &mut watch.phase.0,
        am,
        &plan,
        &watch.attempt,
        &mut bs,
        &fielder_pos,
        layout,
        watch.br.0,
        batting_skill,
    );

    if let Some(slot) = chaser_slot {
        for (_e, f, mut brain) in &mut watch.chasers {
            if f.slot == slot {
                *brain = Brain::Chase;
            }
        }
    }
}

/// Decide what happens when the ball reaches the bat. Returns the fielder
/// slot that should chase (if any). Immediate outcomes finalise the ball
/// right here; struck balls insert a scripted `Pending`.
fn resolve_at_bat(
    commands: &mut Commands,
    recent: &mut RecentBalls,
    phase_enum: &mut PhaseEnum,
    am: &mut ActiveMatch,
    plan: &DeliveryPlan,
    attempt: &ShotAttempt,
    bs: &mut BallState,
    fielder_pos: &[Vec2],
    layout: &CurrentLayout,
    boundary_r: f32,
    batting_skill: f32,
) -> Option<usize> {
    let mut ctx = BallResolutionCtx {
        commands,
        recent,
        phase_enum,
        am,
    };
    let skill = batting_skill;

    // ---- No shot offered ----
    let Some(offset) = attempt.offset else {
        resolve_unplayed_ball(
            &mut ctx,
            bs,
            plan,
            UnplayedCommentary {
                bowled: "BOWLED! No shot offered.",
                wide: "Wide!",
                dot: "Shouldered arms. Dot ball.",
            },
        );
        return None;
    };

    // ---- Wide: never a batted trajectory ----
    // `plan.wide` is decided at release time and is the single source of
    // truth for "was this ball even fair to play at". Bug: previously a
    // registered `attempt.offset` (e.g. the AI's speculative swing timer
    // firing, or a human pressing Confirm) fell straight through to the
    // clean-contact branch below with no regard for line, so a wide could
    // come flying back off the bat exactly like a fair-ball boundary. Gate
    // it here, before timing/tiering ever runs, so "struck" can only ever
    // mean "struck at a fair ball".
    if plan.wide {
        resolve_unplayed_ball(
            &mut ctx,
            bs,
            plan,
            UnplayedCommentary {
                bowled: "BOWLED!",
                wide: "Wide! Well outside off, no shot to be offered there.",
                dot: "Wide!",
            },
        );
        return None;
    }

    let ao = offset.abs();
    let profile = shot_profile(attempt.kind);
    let len_mul = shot_length_penalty(attempt.kind, plan.length_from_stumps);
    let effective_ao = ao * len_mul / profile.forgiveness;

    // ---- Play and miss / thick edge band ----
    if effective_ao >= BEATEN_TIMING_THRESHOLD {
        resolve_unplayed_ball(
            &mut ctx,
            bs,
            plan,
            UnplayedCommentary {
                bowled: "BOWLED!",
                wide: "Wide!",
                dot: "Beaten! Past the edge.",
            },
        );
        return None;
    }

    let tier = if effective_ao < TIER_PERFECT_MAX {
        Tier::Perfect
    } else if effective_ao < TIER_GOOD_MAX {
        Tier::Good
    } else if effective_ao < TIER_OKAY_MAX {
        Tier::Okay
    } else {
        Tier::Edge
    };

    let edge_chance = if attempt.kind.aerial() && tier == Tier::Edge {
        AERIAL_MISTIME_EDGE_CHANCE
    } else {
        EDGE_CARRY_CHANCE
    };

    // ---- Edged ----
    if tier == Tier::Edge && coin(edge_chance) {
        bs.dead = true;
        let edge_text = if attempt.kind.aerial() {
            "Top edge! TAKEN behind!"
        } else {
            "Edged & TAKEN behind!"
        };
        ctx.finalize(
            BallOutcome::Wicket(Dismissal::CaughtBehind { keeper: true }),
            edge_text.into(),
        );
        return None;
    }

    // ---- Clean contact: build the exit velocity from stroke profile ----
    let tier_speed = match tier {
        Tier::Perfect => 1.08,
        Tier::Good => 1.0,
        Tier::Okay => 0.86,
        Tier::Edge => 0.42,
    };
    let tier_elev = match tier {
        Tier::Perfect => 0.0,
        Tier::Good => 2.0,
        Tier::Okay => 5.0,
        Tier::Edge => -6.0,
    };
    let mut speed = profile.speed * tier_speed * skill.clamp(0.82, 1.15);
    let mut elev = (profile.elev + tier_elev).max(2.0);

    if matches!(attempt.kind, ShotKind::Defend | ShotKind::Backfoot) {
        speed = speed.min(13.5);
        elev = elev.min(7.5);
    } else if attempt.kind.aerial() && tier != Tier::Edge {
        speed *= 1.06;
    }

    // Direction: stroke angle dominates; aim and timing perturb placement.
    let mut angle = profile.angle + attempt.dir_x * 14.0 + offset.signum() * effective_ao * 95.0;
    if tier == Tier::Edge {
        // Squirts behind square either side.
        angle = 105.0 + unit() * 55.0;
        if unit() < 0.45 {
            angle = -angle;
        }
        speed = 8.0 + unit() * 7.0;
        elev = 5.0 + unit() * 5.0;
    }

    let dir_xz = crate::core::angle_dir(angle);
    let vel = Vec3::new(
        dir_xz.x * speed,
        elev.to_radians().sin() * speed,
        dir_xz.y * speed,
    );
    bs.pos = Vec3::new(BAT_PLANE_X, bs.pos.y.max(0.35), bs.pos.z);
    bs.vel = vel;
    bs.bounced = false;
    bs.struck = true;

    // ---- Predict how the field plays out ----
    let pred = predict_outcome(bs.pos, bs.vel, fielder_pos, boundary_r);

    let (outcome, text, runs_anim, apply_in, chaser) = match &pred {
        Prediction::Six => (
            BallOutcome::Six,
            "MAXIMUM! That's out of the ground!".into(),
            0,
            3.4,
            None,
        ),
        Prediction::Four => (
            BallOutcome::Four,
            "FOUR! Crashed to the rope.".into(),
            0,
            3.4,
            None,
        ),
        Prediction::Caught { slot } => {
            let name = layout
                .0
                .positions
                .get(*slot)
                .map(|fp| fp.name.to_string())
                .unwrap_or_else(|| "fielder".into());
            (
                BallOutcome::Wicket(Dismissal::Caught { fielder: *slot }),
                format!("CAUGHT at {}!", name),
                0,
                3.2,
                Some(*slot),
            )
        }
        Prediction::Runs {
            n,
            gamble,
            risky,
            chaser,
        } => {
            let total = n + usize::from(*gamble);
            if *gamble && *risky && coin(0.16) {
                (
                    BallOutcome::WicketAndRuns(Dismissal::RunOut, *n as u8),
                    "RUN OUT going for the extra!".into(),
                    (*n as u32 + 1).min(3),
                    RUN_SECONDS * (*n as f32 + 1.1) + 0.6,
                    Some(*chaser),
                )
            } else {
                let txt = match total {
                    0 => "Fielded. Dot ball.".to_string(),
                    1 => "Quick single taken.".to_string(),
                    _ => format!("They come back for {total}."),
                };
                (
                    BallOutcome::Runs(total.min(3) as u8),
                    txt,
                    (total.min(3)) as u32,
                    RUN_SECONDS * (total.min(3) as f32 + 0.65) + 0.8,
                    Some(*chaser),
                )
            }
        }
    };

    ctx.commands.insert_resource(Pending(Some(PendingOutcome {
        outcome,
        text,
        apply_in,
        elapsed: 0.0,
        runs_anim,
        chaser_slot: chaser,
        boundary: matches!(pred, Prediction::Six | Prediction::Four),
        aerial_catch: matches!(pred, Prediction::Caught { .. }),
    })));

    pred_chaser(&pred)
}

fn pred_chaser(p: &Prediction) -> Option<usize> {
    match p {
        Prediction::Caught { slot } => Some(*slot),
        Prediction::Runs { chaser, .. } => Some(*chaser),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Flight outcome prediction (fast coarse ballistic sim)
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub(crate) enum Prediction {
    Six,
    Four,
    Caught {
        slot: usize,
    },
    Runs {
        n: usize,
        gamble: bool,
        risky: bool,
        chaser: usize,
    },
}

/// Fixed real-time step for coarse ballistic prediction (seconds).
const PRED_STEP_SECS: f32 = 0.04;

pub(crate) fn predict_outcome(
    pos: Vec3,
    vel: Vec3,
    fielders: &[Vec2],
    boundary_r: f32,
) -> Prediction {
    let mut p = pos;
    let mut v = vel;
    let dt_real = PRED_STEP_SECS;
    let dt = PRED_STEP_SECS * BALL_TIME_SCALE;
    let mut bounced = false;
    let mut crossed_rope = false;
    let mut stop_pos = p;
    let mut t_total = 0.0_f32;
    let mut landing: Option<(Vec2, f32)> = None;

    for _ in 0..400 {
        v.y -= 9.81 * dt;
        p += v * dt;
        t_total += dt_real;
        let flat = Vec2::new(p.x, p.z);

        if flat.length() > boundary_r {
            crossed_rope = true;
            break;
        }
        if p.y <= BALL_RADIUS && v.y < 0.0 {
            p.y = BALL_RADIUS;
            if landing.is_none() {
                landing = Some((flat, t_total));
            }
            v.y = -v.y * 0.52;
            v.x *= 0.78;
            v.z *= 0.78;
            bounced = true;
        }
        if bounced && p.y < BALL_RADIUS * 2.5 {
            v.x *= 1.0 - 2.4 * dt_real;
            v.z *= 1.0 - 2.4 * dt_real;
        }
        stop_pos = p;
        if p.y < BALL_RADIUS * 2.0 && v.x.abs() + v.z.abs() < 0.4 {
            break;
        }
    }

    if crossed_rope {
        return if bounced {
            Prediction::Four
        } else {
            Prediction::Six
        };
    }

    // Aerial catch near the landing spot.
    if let Some((land, t_air)) = landing
        && t_air > 0.75
        && t_air < 4.5
    {
        let mut best = (usize::MAX, f32::MAX);
        for (i, fp) in fielders.iter().enumerate() {
            let d = (*fp - land).length();
            if d < best.1 {
                best = (i, d);
            }
        }
        if best.1 < 2.4 {
            return Prediction::Caught { slot: best.0 };
        }
    }

    // Ground shot: time until a fielder cuts it off.
    let stop_flat = Vec2::new(stop_pos.x, stop_pos.z);
    let mut nearest = (usize::MAX, f32::MAX);
    for (i, fp) in fielders.iter().enumerate() {
        let d = (*fp - stop_flat).length();
        if d < nearest.1 {
            nearest = (i, d);
        }
    }
    let chase_time = nearest.1 / fielding::FIELDER_SPEED;
    let dead_time = chase_time.max(t_total.min(2.0)) + 0.9;
    let n = ((dead_time / RUN_SECONDS) as usize).min(3);
    let gamble = n < 3 && chase_time > RUN_SECONDS * 1.15;
    let risky = chase_time < RUN_SECONDS * 1.45;
    Prediction::Runs {
        n,
        gamble,
        risky,
        chaser: nearest.0,
    }
}

/// Linear projection: does this ball hit the stumps?
fn hits_stumps(bs: &BallState) -> bool {
    if bs.vel.x <= 0.01 {
        return false;
    }
    let t = (geo::PITCH_HALF_LEN - bs.pos.x) / bs.vel.x;
    if t < 0.0 {
        return false;
    }
    let z = bs.pos.z + bs.vel.z * t;
    let y = bs.pos.y + bs.vel.y * t - 0.5 * 9.81 * t * t;
    z.abs() < 0.17 && y > 0.0 && y < geo::STUMP_HEIGHT + 0.06
}

// ---------------------------------------------------------------------------
// Pending outcome watcher + score application
// ---------------------------------------------------------------------------

/// Applies a ball outcome exactly once, transitioning to the result pause.
fn finalize_ball(
    commands: &mut Commands,
    recent: &mut RecentBalls,
    phase_enum: &mut PhaseEnum,
    am: &mut ActiveMatch,
    outcome: BallOutcome,
    text: String,
) {
    am.state.innings.apply_ball(&outcome);
    recent.push_outcome(&outcome);
    *phase_enum = PhaseEnum::ResultPause { t: 0.0, text };
    commands.insert_resource(ReleaseInfo {
        active: false,
        resolved: true,
        t: 0.0,
        t_arrive: 0.0,
    });
}

pub fn wicket_shake_trigger(
    phase: Res<Phase>,
    mut rig: ResMut<CameraRig>,
    mut last_text: Local<String>,
) {
    if let PhaseEnum::ResultPause { text, .. } = &phase.0 {
        if text != &*last_text {
            let upper = text.to_uppercase();
            if upper.contains("BOWLED")
                || upper.contains("CAUGHT")
                || upper.contains("TAKEN")
                || upper.contains("RUN OUT")
                || upper.contains("WICKET")
            {
                rig.shake = 1.4;
            } else if upper.contains("FOUR") || upper.contains("SIX") || upper.contains("MAXIMUM") {
                rig.shake = 0.5;
            }
            *last_text = text.clone();
        }
    } else {
        // clear when leaving result pause so next wicket retriggers
        if !last_text.is_empty() && !matches!(phase.0, PhaseEnum::ResultPause { .. }) {
            // keep for re-entry detection; do nothing
        }
    }
}

fn enter_ready(commands: &mut Commands, phase_enum: &mut PhaseEnum) {
    *phase_enum = PhaseEnum::ReadyToBall { t: 0.0 };
    reset_delivery_resources(commands);
    commands.insert_resource(CurrentDelivery(None));
}

pub fn clear_recent_on_innings_change(
    phase: Res<Phase>,
    mut recent: ResMut<RecentBalls>,
    mut last: Local<Option<usize>>,
    am: Option<Res<ActiveMatch>>,
) {
    let team = am.as_ref().map(|a| a.state.innings.batting_team);
    if team != *last {
        recent.entries.clear();
        *last = team;
    }
    if matches!(phase.0, PhaseEnum::InningsBreak) {
        recent.entries.clear();
    }
}

#[derive(SystemParam)]
pub(crate) struct PendingWatchParams<'w, 's> {
    commands: Commands<'w, 's>,
    phase: ResMut<'w, Phase>,
    am: Option<ResMut<'w, ActiveMatch>>,
    br: Res<'w, BoundaryRadius>,
    pending: ResMut<'w, Pending>,
    recent: ResMut<'w, RecentBalls>,
    ball_q: Query<'w, 's, &'static mut BallState, With<CricketBall>>,
    fielders: Query<'w, 's, (&'static Fielder, &'static GlobalTransform)>,
}

pub fn sys_pending_watch(time: Res<Time>, _rel: Res<ReleaseInfo>, mut watch: PendingWatchParams) {
    let PhaseEnum::BallLive = watch.phase.0 else {
        return;
    };
    let Some(am) = watch.am.as_mut() else {
        return;
    };
    let Some(p) = watch.pending.0.as_mut() else {
        return;
    };

    // Physical early triggers.
    let Ok(bs) = watch.ball_q.single() else {
        return;
    };
    let flat = Vec2::new(bs.pos.x, bs.pos.z);

    if p.boundary && flat.length() > watch.br.0 {
        let (o, _) = (p.outcome.clone(), p.text.clone());
        finish_pending(
            &mut watch.commands,
            &mut watch.recent,
            &mut watch.phase,
            am,
            &mut watch.pending,
            &mut watch.ball_q,
            o,
        );
        return;
    }

    if p.aerial_catch
        && !bs.bounced
        && bs.vel.y < 0.0
        && bs.pos.y < 2.6
        && bs.pos.y > 0.2
        && let Some(slot) = p.chaser_slot
    {
        for (f, gt) in &watch.fielders {
            if f.slot == slot {
                let fp = Vec2::new(gt.translation().x, gt.translation().z);
                if (fp - flat).length() < 1.5 {
                    let o = p.outcome.clone();
                    finish_pending(
                        &mut watch.commands,
                        &mut watch.recent,
                        &mut watch.phase,
                        am,
                        &mut watch.pending,
                        &mut watch.ball_q,
                        o,
                    );
                    return;
                }
            }
        }
    }

    // Timer fallback.
    p.apply_in -= time.delta_secs();
    if p.apply_in <= 0.0 {
        let o = p.outcome.clone();
        finish_pending(
            &mut watch.commands,
            &mut watch.recent,
            &mut watch.phase,
            am,
            &mut watch.pending,
            &mut watch.ball_q,
            o,
        );
    }
}

fn finish_pending(
    commands: &mut Commands,
    recent: &mut RecentBalls,
    phase: &mut Phase,
    am: &mut ActiveMatch,
    pending: &mut Pending,
    ball_q: &mut Query<&mut BallState, With<CricketBall>>,
    outcome: BallOutcome,
) {
    let text = pending
        .0
        .as_ref()
        .map(|p| p.text.clone())
        .unwrap_or_default();
    *pending = Pending(None);
    if let Ok(mut bs) = ball_q.single_mut() {
        bs.dead = true;
    }
    finalize_ball(commands, recent, &mut phase.0, am, outcome, text);
}

// ---------------------------------------------------------------------------
// Runner animation (batters shuttling between wickets)
// ---------------------------------------------------------------------------

fn flerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn update_striker_runner(
    tf: &mut Transform,
    anim: &mut Anim,
    elapsed: f32,
    runs_anim: u32,
    s_crease: f32,
    n_crease: f32,
    bowler_end: Vec2,
) {
    if runs_anim == 0 {
        return;
    }
    let u = elapsed / RUN_SECONDS;
    let legs = u.floor();
    if legs as i32 >= runs_anim as i32 {
        let done = legs as i32 % 2 == 1;
        tf.translation = Vec3::new(
            if done { n_crease } else { s_crease },
            0.0,
            geo::BATSMAN_POS.y,
        );
        tf.rotation = face_target_quat(Vec2::new(tf.translation.x, tf.translation.z), bowler_end);
        anim.state = AnimState::Idle;
        return;
    }
    let frac = u - legs;
    let tri = if legs as i32 % 2 == 0 {
        frac
    } else {
        1.0 - frac
    };
    let prev = Vec2::new(tf.translation.x, tf.translation.z);
    let x = flerp(s_crease, n_crease, tri);
    let z = geo::BATSMAN_POS.y + if legs as i32 % 2 == 0 { 0.45 } else { -0.45 };
    tf.translation = Vec3::new(x, 0.0, z);
    let move_dir = Vec2::new(x, z) - prev;
    if move_dir.length_squared() > 1e-6 {
        tf.rotation = Quat::from_rotation_y(crate::render::player::yaw_to_face(move_dir));
    }
    anim.state = AnimState::Run { t: elapsed };
}

fn update_non_striker_runner(
    tf: &mut Transform,
    anim: &mut Anim,
    elapsed: f32,
    runs_anim: u32,
    s_crease: f32,
    n_crease: f32,
    bowler_end: Vec2,
) {
    if runs_anim == 0 {
        return;
    }
    let u = elapsed / RUN_SECONDS;
    let legs = u.floor();
    if legs as i32 >= runs_anim as i32 {
        let done = legs as i32 % 2 == 1;
        tf.translation = Vec3::new(if done { s_crease } else { n_crease }, 0.0, 0.9);
        tf.rotation = face_target_quat(Vec2::new(tf.translation.x, tf.translation.z), bowler_end);
        anim.state = AnimState::Idle;
        return;
    }
    let frac = u - legs;
    let tri = if legs as i32 % 2 == 0 {
        frac
    } else {
        1.0 - frac
    };
    let prev = Vec2::new(tf.translation.x, tf.translation.z);
    let x = flerp(n_crease, s_crease, tri);
    let z = if legs as i32 % 2 == 0 { 0.45 } else { -0.45 };
    tf.translation = Vec3::new(x, 0.0, z);
    let move_dir = Vec2::new(x, z) - prev;
    if move_dir.length_squared() > 1e-6 {
        tf.rotation = Quat::from_rotation_y(crate::render::player::yaw_to_face(move_dir));
    }
    anim.state = AnimState::Run { t: elapsed };
}

pub fn sys_runners(
    time: Res<Time>,
    phase: Res<Phase>,
    mut pending: ResMut<Pending>,
    mut figs: Query<(&Figure, &mut Transform, &mut Anim)>,
) {
    let PhaseEnum::BallLive = phase.0 else { return };
    let Some(p) = pending.0.as_mut() else { return };
    p.elapsed += time.delta_secs();

    let s_crease = geo::PITCH_HALF_LEN - geo::CREASE_DEPTH;
    let n_crease = -s_crease;
    let bowler_end = Vec2::new(-geo::PITCH_HALF_LEN, 0.0);

    for (fig, mut tf, mut anim) in &mut figs {
        match fig.kind {
            FigureKind::Batter => {
                update_striker_runner(
                    &mut tf,
                    &mut anim,
                    p.elapsed,
                    p.runs_anim,
                    s_crease,
                    n_crease,
                    bowler_end,
                );
            }
            FigureKind::NonStriker => {
                update_non_striker_runner(
                    &mut tf,
                    &mut anim,
                    p.elapsed,
                    p.runs_anim,
                    s_crease,
                    n_crease,
                    bowler_end,
                );
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Result pause -> over break / innings break / match over
// ---------------------------------------------------------------------------

pub fn sys_result_pause(
    time: Res<Time>,
    mut phase: ResMut<Phase>,
    am: Option<ResMut<ActiveMatch>>,
    _wd: Option<Res<WorldData>>,
) {
    let PhaseEnum::ResultPause { t, .. } = &mut phase.0 else {
        return;
    };
    let Some(am) = am else {
        return;
    };
    let am = am.into_inner();
    *t += time.delta_secs();
    if *t < RESULT_PAUSE_SECS {
        return;
    }

    match am.state.check_progression() {
        Some(crate::core::rules::Progression::InningsBreak) => {
            phase.0 = PhaseEnum::InningsBreak;
            return;
        }
        Some(crate::core::rules::Progression::MatchOver) => {
            phase.0 = PhaseEnum::MatchOver;
            return;
        }
        None => {}
    }

    if am.state.result.is_some() {
        phase.0 = PhaseEnum::MatchOver;
    } else {
        // Route through the retrieval/return phase first — a fielder has
        // to actually fetch and throw the ball back before the next
        // delivery can start (bug: the ball used to just vanish here and
        // reappear in the bowler's hand next round).
        phase.0 = PhaseEnum::BallReturn {
            t: 0.0,
            then_over_break: am.state.innings.over_complete(),
        };
    }
}

fn finish_ball_return(commands: &mut Commands, phase_enum: &mut PhaseEnum, then_over_break: bool) {
    if then_over_break {
        *phase_enum = PhaseEnum::OverBreak { t: 0.0 };
    } else {
        enter_ready(commands, phase_enum);
    }
}

#[derive(SystemParam)]
pub(crate) struct BallReturnParams<'w, 's> {
    commands: Commands<'w, 's>,
    ball_q: Query<'w, 's, (&'static mut BallState, &'static mut Transform), With<CricketBall>>,
    fielders: Query<
        'w,
        's,
        (
            &'static Fielder,
            &'static mut Brain,
            &'static GlobalTransform,
        ),
    >,
}

/// Fetch-and-return: whoever is already chasing (or, failing that, whoever
/// is nearest) walks to the dead ball, "carries" it home, then it flies the
/// last stretch back to the bowler in a short animated throw. Bounded by
/// [`BALL_RETURN_TIMEOUT`] so this can never hang the match.
pub fn sys_ball_return(
    time: Res<Time>,
    mut phase: ResMut<Phase>,
    mut p: BallReturnParams,
    mut was_active: Local<bool>,
    mut retriever: Local<Option<usize>>,
    mut throw: Local<Option<(Vec3, f32)>>,
) {
    let PhaseEnum::BallReturn { t, then_over_break } = &mut phase.0 else {
        *was_active = false;
        return;
    };
    *t += time.delta_secs();
    let t_now = *t;
    let then_over_break = *then_over_break;

    let bowler_pos = Vec2::new(-geo::PITCH_HALF_LEN - 8.0, 0.35);
    let bowler_hand = Vec3::new(bowler_pos.x - 0.3, 1.6, bowler_pos.y + 0.25);

    let Ok((mut bs, mut tf)) = p.ball_q.single_mut() else {
        finish_ball_return(&mut p.commands, &mut phase.0, then_over_break);
        *was_active = false;
        return;
    };

    if !*was_active {
        *was_active = true;
        *throw = None;
        // A struck ball already has a chaser dispatched from contact
        // resolution — let them keep going. Otherwise (a wide, a leave, a
        // dot ball nobody was sent after) send whoever is nearest to where
        // it actually stopped.
        let already = p
            .fielders
            .iter()
            .find(|(_, b, _)| matches!(**b, Brain::Chase | Brain::Collect | Brain::Return))
            .map(|(f, _, _)| f.slot);
        *retriever = already;
        if retriever.is_none() {
            let rest = Vec2::new(bs.pos.x, bs.pos.z);
            let nearest = p
                .fielders
                .iter()
                .map(|(f, _, gt)| {
                    let d = (Vec2::new(gt.translation().x, gt.translation().z) - rest).length();
                    (f.slot, d)
                })
                .min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((slot, _)) = nearest {
                *retriever = Some(slot);
                for (f, mut brain, _) in &mut p.fielders {
                    if f.slot == slot {
                        *brain = Brain::Chase;
                    }
                }
            }
        }
    }

    if t_now >= BALL_RETURN_TIMEOUT {
        bs.pos = bowler_hand;
        bs.vel = Vec3::ZERO;
        tf.translation = bowler_hand;
        finish_ball_return(&mut p.commands, &mut phase.0, then_over_break);
        *was_active = false;
        return;
    }

    let retriever_state = retriever.and_then(|slot| {
        p.fielders
            .iter()
            .find(|(f, _, _)| f.slot == slot)
            .map(|(_, b, gt)| (*b, gt.translation()))
    });

    match (retriever_state, *throw) {
        (Some((Brain::Collect, gt)), _) | (Some((Brain::Return, gt)), _) => {
            // Carried in the fielder's hands on the walk back.
            let carry = gt + Vec3::new(0.0, 1.1, 0.0);
            bs.pos = carry;
            tf.translation = carry;
        }
        (Some((Brain::AtPost, gt)), None) => {
            // Just arrived home: kick off the final throw from here.
            *throw = Some((gt + Vec3::new(0.0, 1.1, 0.0), t_now));
        }
        (_, Some((start, start_t))) => {
            let u = ((t_now - start_t) / RETURN_THROW_SECS).clamp(0.0, 1.0);
            let arc = (u * std::f32::consts::PI).sin() * 1.4; // short lob arc home
            let pos = start.lerp(bowler_hand, u) + Vec3::new(0.0, arc, 0.0);
            bs.pos = pos;
            tf.translation = pos;
            if u >= 1.0 {
                finish_ball_return(&mut p.commands, &mut phase.0, then_over_break);
                *was_active = false;
            }
        }
        (None, _) => {
            // No fielders in the world at all (e.g. a headless test scene)
            // — nothing to animate, just hand the ball back.
            finish_ball_return(&mut p.commands, &mut phase.0, then_over_break);
            *was_active = false;
        }
        _ => {}
    }
}

/// Automatic bowler rotation (v1 behaviour): cycles through the top-5
/// bowlers by over number, nudging one slot along if that would repeat the
/// bowler who just finished. Used for AI-fielding sides, and as the
/// fallback if a human captain doesn't choose in time.
fn automatic_bowler_pick(team: &Team, innings: &Innings) -> Option<usize> {
    let opts = pick_bowlers(team, 5);
    if opts.is_empty() {
        return None;
    }
    let over = (innings.legal_balls / 6) as usize;
    let mut idx = over % opts.len();
    if Some(opts[idx]) == innings.previous_bowler && opts.len() > 1 {
        idx = (idx + 1) % opts.len();
    }
    Some(opts[idx])
}

/// Cursor position to open the chooser on: the first eligible bowler in
/// ranked order, or 0 if (pathologically) nobody is eligible.
fn default_bowler_cursor(team: &Team, innings: &Innings, overs: u32) -> usize {
    all_bowlers_ranked(team)
        .iter()
        .position(|&p| innings.bowler_eligible(p, overs))
        .unwrap_or(0)
}

pub fn sys_over_break(
    mut commands: Commands,
    time: Res<Time>,
    mut phase: ResMut<Phase>,
    am: Option<ResMut<ActiveMatch>>,
    wd: Option<Res<WorldData>>,
) {
    let PhaseEnum::OverBreak { t } = &mut phase.0 else {
        return;
    };
    let (Some(am), Some(wd)) = (am, wd) else {
        return;
    };
    let am = am.into_inner();
    *t += time.delta_secs();
    if *t < 1.3 {
        return;
    }

    if am.user_bowling() {
        // Human captain: hand off to the chooser rather than picking for them.
        let cursor =
            default_bowler_cursor(am.fielding_team(&wd), &am.state.innings, am.state.overs);
        phase.0 = PhaseEnum::BowlerSelect { t: 0.0, cursor };
        return;
    }

    // AI-fielding side: automatic rotation (v1 behaviour, unchanged).
    if let Some(next) = automatic_bowler_pick(am.fielding_team(&wd), &am.state.innings) {
        am.bowler_player = next;
        am.state.innings.current_bowler = Some(next);
    }
    enter_ready(&mut commands, &mut phase.0);
}

/// How long the human bowler-select screen waits before falling back to the
/// automatic pick, so an idle player can never soft-lock the match.
const BOWLER_SELECT_TIMEOUT: f32 = 12.0;

/// Human-controlled bowler chooser between overs. Only entered when the
/// player's side is fielding (see `sys_over_break`); AI sides never see it.
pub fn sys_bowler_select(
    mut commands: Commands,
    time: Res<Time>,
    input: Res<PlayerInput>,
    mut phase: ResMut<Phase>,
    am: Option<ResMut<ActiveMatch>>,
    wd: Option<Res<WorldData>>,
) {
    let PhaseEnum::BowlerSelect { t, cursor } = &mut phase.0 else {
        return;
    };
    let (Some(am), Some(wd)) = (am, wd) else {
        return;
    };
    let am = am.into_inner();
    *t += time.delta_secs();

    let team = am.fielding_team(&wd);
    let options = all_bowlers_ranked(team);
    if options.is_empty() {
        // No legal bowlers at all (shouldn't happen with 11-a-side rosters).
        enter_ready(&mut commands, &mut phase.0);
        return;
    }
    *cursor = (*cursor).min(options.len() - 1);

    if input.pressed(Action::Next) {
        *cursor = (*cursor + 1) % options.len();
    }
    if input.pressed(Action::Prev) {
        *cursor = (*cursor + options.len() - 1) % options.len();
    }

    if *t >= BOWLER_SELECT_TIMEOUT {
        if let Some(next) = automatic_bowler_pick(team, &am.state.innings) {
            am.bowler_player = next;
            am.state.innings.current_bowler = Some(next);
        }
        enter_ready(&mut commands, &mut phase.0);
        return;
    }

    if input.pressed(Action::Confirm) {
        let choice = options[*cursor];
        // Reject illegal picks outright — no two overs in a row, no bowler
        // over the one-fifth-of-overs cap. The player just stays on the
        // screen and can pick again.
        if am.state.innings.bowler_eligible(choice, am.state.overs) {
            am.bowler_player = choice;
            am.state.innings.current_bowler = Some(choice);
            enter_ready(&mut commands, &mut phase.0);
        }
    }
}

pub fn sys_innings_break(
    mut commands: Commands,
    input: Res<PlayerInput>,
    mut phase: ResMut<Phase>,
    am: Option<ResMut<ActiveMatch>>,
    wd: Option<Res<WorldData>>,
    mut rebuild: MessageWriter<RebuildScene>,
) {
    let PhaseEnum::InningsBreak = phase.0 else {
        return;
    };
    let (Some(am), Some(wd)) = (am, wd) else {
        return;
    };
    let am = am.into_inner();
    if !input.pressed(Action::Confirm) {
        return;
    }

    // The chasing side is teams[1] before `start_chase` swaps them.
    let chasing_idx = am.state.teams[1];
    let bowling_idx = am.state.teams[0];
    let order = batting_order(&wd.teams[chasing_idx]);
    // Full roster, not just the top-5 automatic rotation — the human
    // bowler-select screen needs every bowling-capable player carded up.
    let bowlers = all_bowlers_ranked(&wd.teams[bowling_idx]);
    am.state.start_chase(order, &bowlers);
    am.bowler_player = bowlers[0];
    am.state.innings.current_bowler = Some(bowlers[0]);

    rebuild.write(RebuildScene);
    enter_ready(&mut commands, &mut phase.0);
}

// ---------------------------------------------------------------------------
// Camera direction
// ---------------------------------------------------------------------------

/// Record the ball flight during live play for slow-motion replays.
pub fn record_ball_flight(
    phase: Res<Phase>,
    time: Res<Time>,
    ball_q: Query<&Transform, With<CricketBall>>,
    mut rec: ResMut<BallRecording>,
) {
    match &phase.0 {
        PhaseEnum::MatchIntro { .. }
        | PhaseEnum::RunUp { .. }
        | PhaseEnum::AimLength { .. }
        | PhaseEnum::ReadyToBall { .. } => {
            rec.samples.clear();
            rec.t = 0.0;
        }
        PhaseEnum::BallLive => {
            rec.t += time.delta_secs();
            if rec.t > 6.0 {
                return; // safety cap
            }
            if let Ok(tf) = ball_q.single() {
                // Stop once the ball has settled so replays stay tight.
                if let Some((_, last)) = rec.samples.last()
                    && (*last - tf.translation).length_squared() < 1e-6
                {
                    return;
                }
                let t = rec.t;
                rec.samples.push((t, tf.translation));
            }
        }
        _ => {}
    }
}

/// True when CRICKET_AUTOTEST is capturing stadium overview screenshots.
pub fn stadium_qa_autotest_active() -> bool {
    matches!(
        std::env::var("CRICKET_AUTOTEST").as_deref(),
        Ok("stadium") | Ok("stadium-night")
    )
}

/// Stadium QA captures: force the broadcast establishing lens every frame so
/// menu-driven camera modes cannot win over the overview shot.
pub fn sys_stadium_qa_camera(mut rig: ResMut<CameraRig>, mut replay: ResMut<ReplayState>) {
    if !stadium_qa_autotest_active() {
        return;
    }
    rig.mode = CamMode::Broadcast;
    replay.active = false;
}

fn camera_mode_ball_live(
    pending: &Pending,
    recording: &BallRecording,
    am: &ActiveMatch,
) -> CamMode {
    if pending.0.is_some() {
        // Struck ball in flight: follow it tightly, switching to the
        // rope-level camera as it nears the boundary fence.
        let near_rope = pending.0.as_ref().map(|p| p.boundary).unwrap_or(false);
        if near_rope && recording.samples.len() > 12 {
            CamMode::BoundaryCam
        } else {
            CamMode::FollowBall
        }
    } else if am.user_bowling() {
        CamMode::BowlingEnd
    } else {
        CamMode::BattingEnd
    }
}

fn result_pause_is_wicket(text: &str) -> bool {
    let upper = text.to_uppercase();
    ["BOWLED", "CAUGHT", "TAKEN", "RUN OUT"]
        .iter()
        .any(|w| upper.contains(w))
}

fn result_pause_is_boundary(text: &str) -> bool {
    let upper = text.to_uppercase();
    ["FOUR", "SIX", "MAXIMUM"].iter().any(|w| upper.contains(w))
}

fn init_result_pause_presentation(
    text: &str,
    recording: &BallRecording,
    replay: &mut ReplayState,
) -> bool {
    let wicket = result_pause_is_wicket(text);
    let boundary = result_pause_is_boundary(text);
    let enough_footage = recording.samples.len() > 10;
    replay.active = false;
    replay.t_play = 0.0;
    replay.dur = ((recording.samples.last().map_or(0.0, |(t, _)| *t)) * 0.55).clamp(0.7, 1.7);
    enough_footage && (wicket || boundary)
}

fn apply_result_pause_camera(
    t: f32,
    text: &str,
    eligible: bool,
    am: &ActiveMatch,
    replay: &mut ReplayState,
    pres: &mut PresentationState,
) -> CamMode {
    let wicket = result_pause_is_wicket(text);

    if wicket && t < 0.9 {
        pres.impact_on = true;
        replay.active = false;
        CamMode::ImpactCut
    } else if eligible && t >= 0.5 && t <= 0.5 + replay.dur {
        replay.active = true;
        replay.t_play = (t - 0.5) * 0.5;
        pres.replay_on = true;
        CamMode::ReplaySide
    } else {
        replay.active = false;
        if eligible && t < 0.5 {
            if wicket {
                CamMode::ImpactCut
            } else {
                CamMode::FollowBall
            }
        } else if am.user_bowling() {
            CamMode::BowlingEnd
        } else {
            CamMode::FollowBall
        }
    }
}

/// Broadcast presentation director: impact cuts on wickets, a boundary
/// camera flash, then a slow-motion side-on replay before play resumes.
pub fn sys_camera_modes(
    phase: Res<Phase>,
    am: Option<Res<ActiveMatch>>,
    pending: Res<Pending>,
    recording: Res<BallRecording>,
    mut replay: ResMut<ReplayState>,
    mut pres: ResMut<PresentationState>,
    mut rig: ResMut<CameraRig>,
    mut was_pause: Local<bool>,
    mut eligible: Local<bool>,
) {
    let Some(am) = am.as_ref() else {
        return;
    };
    pres.replay_on = false;
    pres.impact_on = false;

    match phase.0.clone() {
        PhaseEnum::BallLive | PhaseEnum::ResultPause { .. } => {}
        _ => {
            *was_pause = false;
            *eligible = false;
            replay.active = false;
        }
    }

    match phase.0.clone() {
        PhaseEnum::BallLive => {
            *was_pause = false;
            rig.mode = camera_mode_ball_live(&pending, &recording, am);
        }
        PhaseEnum::ResultPause { t, text } => {
            // Fresh result pause? Decide whether this moment deserves the full
            // treatment (impact cut + slow-mo replay).
            if !*was_pause {
                *eligible = init_result_pause_presentation(&text, &recording, &mut replay);
            }
            *was_pause = true;
            rig.mode = apply_result_pause_camera(t, &text, *eligible, am, &mut replay, &mut pres);
        }
        PhaseEnum::OverBreak { .. }
        | PhaseEnum::BowlerSelect { .. }
        | PhaseEnum::InningsBreak
        | PhaseEnum::MatchOver => {
            rig.mode = CamMode::Broadcast;
            replay.active = false;
        }
        PhaseEnum::BallReturn { .. } => {
            // Wide broadcast shot while the fielder walks/throws it home,
            // rather than leaving whatever tight camera the result pause
            // last chose pointed at nothing in particular.
            rig.mode = CamMode::Broadcast;
        }
        PhaseEnum::MatchIntro { .. } => {
            rig.mode = CamMode::MatchIntro;
            replay.active = false;
        }
        _ => {} // ready/aim/runup set their own mode in sys_ready
    }
}

/// Reset fielder brains when a new delivery cycle starts.
pub fn fielding_brain_reset(phase: Res<Phase>, mut last: Local<u8>, mut brains: Query<&mut Brain>) {
    let disc = match phase.0 {
        PhaseEnum::ReadyToBall { .. } => 1u8,
        _ => 0,
    };
    if disc == 1 && *last != 1 {
        for mut b in &mut brains {
            if matches!(*b, Brain::Chase | Brain::Collect | Brain::Return) {
                *b = Brain::AtPost;
            }
        }
    }
    *last = disc;
}

// ---------------------------------------------------------------------------
// Ball visibility: motion trail dots during live flight
// ---------------------------------------------------------------------------

#[derive(Component)]
pub(crate) struct BallTrailDot {
    life: f32,
}

pub(crate) struct BallTrailAssets {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

/// Leave short-lived emissive crumbs along the ball path for readability.
pub fn sys_ball_trail(
    phase: Res<Phase>,
    time: Res<Time>,
    ball_q: Query<&Transform, (With<CricketBall>, Without<BallTrailDot>)>,
    mut dots: Query<(Entity, &mut BallTrailDot, &mut Transform), Without<CricketBall>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut trail_assets: Local<Option<BallTrailAssets>>,
    mut last_spawn: Local<f32>,
) {
    let live = matches!(phase.0, PhaseEnum::BallLive);
    for (e, mut dot, mut tf) in &mut dots {
        dot.life -= time.delta_secs();
        if dot.life <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        tf.scale = Vec3::splat(dot.life * 1.4);
    }
    if !live {
        *last_spawn = 0.0;
        return;
    }
    *last_spawn += time.delta_secs();
    if *last_spawn < 0.045 {
        return;
    }
    *last_spawn = 0.0;
    let Ok(ball_tf) = ball_q.single() else { return };

    if trail_assets.is_none() {
        *trail_assets = Some(BallTrailAssets {
            mesh: meshes.add(Sphere::new(0.028)),
            material: materials.add(StandardMaterial {
                base_color: Color::srgba(1.0, 0.35, 0.2, 0.55),
                emissive: LinearRgba::new(1.8, 0.4, 0.15, 1.0),
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                ..Default::default()
            }),
        });
    }
    let assets = trail_assets.as_ref().unwrap();
    commands.spawn((
        BallTrailDot { life: 0.35 },
        Mesh3d(assets.mesh.clone()),
        MeshMaterial3d(assets.material.clone()),
        Transform::from_translation(ball_tf.translation),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::rules::MatchState;
    use crate::core::teams::{batting_order, builtin_teams};
    use crate::input::PlayerInput;
    use crate::render::camera_rig::CameraRig;
    use bevy::time::TimeUpdateStrategy;

    /// Advance the app clock by `secs` on the next `App::update()` call.
    ///
    /// MinimalPlugins runs Bevy's `time_system` in `First`, which overwrites
    /// manually bumped `Time<Real>` / `Time<Virtual>` when the strategy is still
    /// `Automatic`. Manual duration keeps `Res<Time>::delta_secs()` deterministic.
    ///
    /// Steps larger than the virtual clock's max delta (250ms by default) are
    /// clamped; use repeated small steps when simulating longer intervals.
    fn advance_test_time(world: &mut World, secs: f32) {
        world.insert_resource(TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_secs_f32(secs),
        ));
    }

    /// Prime Bevy's time pipeline so the next manual step reports a real delta.
    fn prime_test_time(app: &mut App) {
        advance_test_time(app.world_mut(), 0.0);
        app.update();
    }

    /// User's team is `teams[1]` (fielding) here — the bowling-flow tests
    /// (`sys_aim`, `sys_runup`, etc.) all rely on `user_bowling() == true`.
    fn minimal_active_match() -> ActiveMatch {
        let team = &builtin_teams()[0];
        let order = batting_order(team);
        let bowlers = pick_bowlers(team, 5);
        ActiveMatch {
            state: MatchState::new(20, [0, 1], order, &bowlers),
            stadium: 0,
            user_team: Some(1),
            bowler_player: bowlers[0],
        }
    }

    /// Same as `minimal_active_match`, but the user's team is `teams[0]`
    /// (batting), so the *other* side is fielding — used to exercise the
    /// AI-automatic bowler rotation path.
    fn minimal_active_match_ai_fielding() -> ActiveMatch {
        let mut am = minimal_active_match();
        am.user_team = Some(0);
        am
    }

    /// Fielders carry both Figure and Fielder; overlapping Transform queries panic (B0001).
    #[test]
    fn sys_ready_resets_fielders_with_figure_component() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_systems(Update, sys_ready);

        let layout = geo::FieldLayout::standard();
        let slot = 3;
        let expected = layout.positions[slot].world_pos(geo::BATSMAN_POS);

        app.world_mut().spawn((
            Figure {
                kind: FigureKind::Fielder(slot),
            },
            Fielder {
                slot,
                is_keeper: false,
                label: "cover",
                post: expected,
            },
            Transform::from_xyz(99.0, 99.0, 99.0),
            Anim::default(),
        ));

        app.world_mut()
            .insert_resource(Phase(PhaseEnum::ReadyToBall { t: 0.0 }));
        app.world_mut().insert_resource(PlayerInput::default());
        app.world_mut().insert_resource(minimal_active_match());
        app.world_mut().insert_resource(CurrentLayout(layout));
        app.world_mut().insert_resource(CameraRig::default());

        app.update();

        let mut q = app.world_mut().query::<(&Figure, &Transform, &Anim)>();
        let world = app.world_mut();
        let mut found = false;
        for (fig, tf, anim) in q.iter(world) {
            if fig.kind == FigureKind::Fielder(slot) {
                assert_eq!(tf.translation.x, expected.x);
                assert_eq!(tf.translation.y, 0.0);
                assert_eq!(tf.translation.z, expected.y);
                assert!(matches!(anim.state, AnimState::Idle));
                found = true;
            }
        }
        assert!(found, "fielder entity should exist and be repositioned");
    }

    #[test]
    fn chase_pressure_scales_with_match_length() {
        let early_5 = chase_pressure(Some(31), 10, 0, 5);
        let late_5 = chase_pressure(Some(31), 10, 24, 5);
        assert!(late_5 > early_5);

        let early_20 = chase_pressure(Some(121), 60, 0, 20);
        let late_20 = chase_pressure(Some(121), 60, 108, 20);
        assert!(late_20 > early_20);
        assert!(late_5 > early_20);
        assert!(late_20 > chase_pressure(Some(121), 60, 60, 20));
    }

    #[test]
    fn predict_outcome_uses_scaled_physics_time() {
        let pos = Vec3::new(10.0, 1.0, 0.0);
        let vel = Vec3::new(18.0, 8.0, 0.0);
        let fielders = vec![Vec2::new(50.0, 0.0)];
        let pred = predict_outcome(pos, vel, &fielders, 70.0);
        match pred {
            Prediction::Runs { chaser, .. } => assert_eq!(chaser, 0),
            other => panic!("expected ground runs prediction, got {:?}", other),
        }
    }

    /// Bug 3: a wide must never produce a batted trajectory, even if a
    /// shot attempt was somehow registered against it (an AI swing timer
    /// firing, or a mistaken press). `plan.wide` has to be the authoritative
    /// gate ahead of any timing/tier logic.
    #[test]
    fn resolve_at_bat_never_bats_a_wide_even_with_an_attempt() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(minimal_active_match());
        app.insert_resource(RecentBalls::default());
        app.insert_resource(Phase(PhaseEnum::BallLive));

        fn probe(
            mut commands: Commands,
            mut recent: ResMut<RecentBalls>,
            mut phase: ResMut<Phase>,
            mut am: ResMut<ActiveMatch>,
        ) {
            // Well outside the wide threshold (line_z.abs() > 1.35).
            let plan = build_plan(
                BowlStyle::Medium,
                1.9,
                7.0,
                DeliveryVariation::Stock,
                0.7,
                0.7,
            );
            assert!(plan.wide, "test plan should be a wide");

            let attempt = ShotAttempt {
                pressed: true,
                offset: Some(0.0), // perfect timing, i.e. "would have middled it"
                loft: false,
                dir_x: 0.0,
                footwork: Footwork::Front,
                kind: ShotKind::StraightDrive,
                ai_scheduled: false,
            };
            let mut bs = BallState::new_release(
                Vec3::new(BAT_PLANE_X, 0.9, plan.line_z),
                Vec3::new(10.0, 0.0, 0.0),
            );
            let fielder_pos = vec![Vec2::ZERO; geo::FieldLayout::standard().positions.len()];
            let layout = CurrentLayout(geo::FieldLayout::standard());

            let chaser = resolve_at_bat(
                &mut commands,
                &mut recent,
                &mut phase.0,
                &mut am,
                &plan,
                &attempt,
                &mut bs,
                &fielder_pos,
                &layout,
                65.0,
                0.7,
            );
            assert!(chaser.is_none(), "a wide should never dispatch a chaser");
            assert!(!bs.struck, "a wide ball must never be marked struck");
        }

        app.add_systems(Update, probe);
        app.update();

        match &app.world().resource::<Phase>().0 {
            PhaseEnum::ResultPause { text, .. } => {
                assert!(
                    text.to_uppercase().contains("WIDE"),
                    "expected a wide result, got {text:?}"
                );
            }
            other => panic!("expected ResultPause after a wide, got {other:?}"),
        }
        assert_eq!(
            app.world()
                .resource::<RecentBalls>()
                .entries
                .back()
                .unwrap(),
            "Wd"
        );
    }

    #[test]
    fn sys_over_break_waits_about_one_point_three_seconds_then_auto_rotates_for_ai() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_systems(Update, sys_over_break);

        app.world_mut()
            .insert_resource(Phase(PhaseEnum::OverBreak { t: 0.0 }));
        app.world_mut()
            .insert_resource(minimal_active_match_ai_fielding());
        app.world_mut().insert_resource(WorldData::new());
        app.world_mut().insert_resource(Assets::<Mesh>::default());
        app.world_mut()
            .insert_resource(Assets::<StandardMaterial>::default());
        prime_test_time(&mut app);

        let mut elapsed = 0.0_f32;
        let mut ready = false;
        while elapsed < 2.0 && !ready {
            advance_test_time(app.world_mut(), 0.05);
            elapsed += 0.05;
            app.update();
            ready = matches!(
                app.world().resource::<Phase>().0,
                PhaseEnum::ReadyToBall { .. }
            );
        }
        assert!(
            ready,
            "AI-fielding over break should auto-rotate straight to ready"
        );
        assert!(
            elapsed >= 1.25,
            "over break ended too quickly: {:.2}s",
            elapsed
        );
        assert!(elapsed < 1.45, "over break took too long: {:.2}s", elapsed);
    }

    #[test]
    fn sys_over_break_hands_off_to_chooser_when_user_is_fielding() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_systems(Update, sys_over_break);

        app.world_mut()
            .insert_resource(Phase(PhaseEnum::OverBreak { t: 0.0 }));
        app.world_mut().insert_resource(minimal_active_match());
        app.world_mut().insert_resource(WorldData::new());
        app.world_mut().insert_resource(Assets::<Mesh>::default());
        app.world_mut()
            .insert_resource(Assets::<StandardMaterial>::default());
        prime_test_time(&mut app);

        for _ in 0..30 {
            advance_test_time(app.world_mut(), 0.05);
            app.update();
        }

        match &app.world().resource::<Phase>().0 {
            PhaseEnum::BowlerSelect { .. } => {}
            other => panic!("expected BowlerSelect for a human-fielding over break, got {other:?}"),
        }
    }

    #[test]
    fn sys_bowler_select_rejects_ineligible_and_accepts_eligible_choice() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_systems(Update, sys_bowler_select);

        let wd = WorldData::new();
        let mut am = minimal_active_match();
        let ranked = all_bowlers_ranked(am.fielding_team(&wd));
        // Force the top-ranked bowler to be ineligible (just bowled the last over).
        am.state.innings.previous_bowler = Some(ranked[0]);
        app.world_mut()
            .insert_resource(Phase(PhaseEnum::BowlerSelect { t: 0.0, cursor: 0 }));
        app.world_mut().insert_resource(am);
        app.world_mut().insert_resource(WorldData::new());
        app.world_mut().insert_resource(PlayerInput {
            just_pressed: vec![Action::Confirm],
            ..Default::default()
        });
        prime_test_time(&mut app);
        advance_test_time(app.world_mut(), 0.05);
        app.update();

        // Cursor still on the ineligible top bowler: Confirm must be rejected.
        match &app.world().resource::<Phase>().0 {
            PhaseEnum::BowlerSelect { cursor, .. } => assert_eq!(*cursor, 0),
            other => panic!("ineligible confirm should not advance the phase, got {other:?}"),
        }
        assert_eq!(
            app.world()
                .resource::<ActiveMatch>()
                .state
                .innings
                .current_bowler,
            None,
            "an ineligible pick must never be applied"
        );

        // Move to the next (eligible) candidate and confirm again.
        app.world_mut().resource_mut::<PlayerInput>().just_pressed = vec![Action::Next];
        advance_test_time(app.world_mut(), 0.05);
        app.update();
        app.world_mut().resource_mut::<PlayerInput>().just_pressed = vec![Action::Confirm];
        advance_test_time(app.world_mut(), 0.05);
        app.update();

        match &app.world().resource::<Phase>().0 {
            PhaseEnum::ReadyToBall { .. } => {}
            other => panic!("eligible confirm should hand off to ReadyToBall, got {other:?}"),
        }
        assert_eq!(
            app.world()
                .resource::<ActiveMatch>()
                .state
                .innings
                .current_bowler,
            Some(ranked[1])
        );
    }

    #[test]
    fn sys_bowler_select_times_out_to_automatic_pick() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_systems(Update, sys_bowler_select);

        app.world_mut()
            .insert_resource(Phase(PhaseEnum::BowlerSelect { t: 0.0, cursor: 0 }));
        app.world_mut().insert_resource(minimal_active_match());
        app.world_mut().insert_resource(WorldData::new());
        app.world_mut().insert_resource(PlayerInput::default());
        prime_test_time(&mut app);

        let mut elapsed = 0.0_f32;
        while elapsed < BOWLER_SELECT_TIMEOUT + 0.5 {
            advance_test_time(app.world_mut(), 0.25);
            elapsed += 0.25;
            app.update();
        }

        match &app.world().resource::<Phase>().0 {
            PhaseEnum::ReadyToBall { .. } => {}
            other => panic!("timeout should fall back to the automatic pick, got {other:?}"),
        }
        assert!(
            app.world()
                .resource::<ActiveMatch>()
                .state
                .innings
                .current_bowler
                .is_some(),
            "timeout fallback should still assign a bowler"
        );
    }

    #[test]
    fn sys_runup_moves_ball_transform_with_bowler() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_systems(Update, sys_runup);

        app.world_mut().spawn((
            Figure {
                kind: FigureKind::Bowler,
            },
            Transform::from_xyz(-20.0, 0.0, 0.35),
            Anim {
                state: AnimState::Run { t: 0.0 },
            },
        ));
        app.world_mut().spawn((
            CricketBall,
            BallState::default(),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));

        app.world_mut()
            .insert_resource(Phase(PhaseEnum::RunUp { p: 0.0 }));
        app.world_mut().insert_resource(CurrentDelivery(None));
        app.world_mut().insert_resource(minimal_active_match());
        app.world_mut().insert_resource(WorldData::new());
        app.world_mut().insert_resource(MatchScene {
            stadium_root: Entity::PLACEHOLDER,
            ball: Entity::PLACEHOLDER,
            bowler: Entity::PLACEHOLDER,
            striker: Entity::PLACEHOLDER,
            non_striker: Entity::PLACEHOLDER,
            fielders: vec![],
            marker: None,
        });
        prime_test_time(&mut app);

        for _ in 0..13 {
            advance_test_time(app.world_mut(), 0.05);
            app.update();
        }

        let delta = app.world().resource::<Time>().delta_secs();
        let run_p = match &app.world().resource::<Phase>().0 {
            PhaseEnum::RunUp { p } => *p,
            other => panic!("unexpected phase after run-up step: {other:?}"),
        };
        let world = app.world_mut();
        let mut q = world.query::<(&BallState, &Transform)>();
        let (bs, tf) = q.single(world).unwrap();
        assert!(
            (run_p - 0.65 / RUNUP_SECS).abs() < 0.02,
            "run-up progress should reflect 0.65s of motion (p={run_p:.3}, last_delta={delta:.3})"
        );
        assert!(
            bs.pos.x > -16.5,
            "ball did not follow bowler far enough (p={run_p:.3}, pos={:?})",
            bs.pos
        );
        assert_eq!(tf.translation, bs.pos);
    }

    /// Run-up must stay on the mocap jog through the delivery stride; switching
    /// to procedural `BowlAction` at `p > 0.7` was measured to bury foot bones
    /// to about **−1.03 m** world Y (vs **+0.004 m** with run clip retained).
    #[test]
    fn bowler_runup_keeps_mocap_run_through_delivery_stride() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_systems(Update, sys_runup);

        app.world_mut().spawn((
            Figure {
                kind: FigureKind::Bowler,
            },
            Transform::from_xyz(-20.0, 0.0, 0.35),
            Anim::default(),
        ));
        app.world_mut().spawn((
            CricketBall,
            BallState::default(),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));

        app.world_mut()
            .insert_resource(Phase(PhaseEnum::RunUp { p: 0.84 }));
        app.world_mut().insert_resource(CurrentDelivery(None));
        app.world_mut().insert_resource(minimal_active_match());
        app.world_mut().insert_resource(WorldData::new());
        app.world_mut().insert_resource(MatchScene {
            stadium_root: Entity::PLACEHOLDER,
            ball: Entity::PLACEHOLDER,
            bowler: Entity::PLACEHOLDER,
            striker: Entity::PLACEHOLDER,
            non_striker: Entity::PLACEHOLDER,
            fielders: vec![],
            marker: None,
        });
        prime_test_time(&mut app);
        advance_test_time(app.world_mut(), 0.02);
        app.update();

        let mut q = app.world_mut().query::<&Anim>();
        let anim = q.single(app.world()).unwrap();
        assert!(
            matches!(anim.state, AnimState::Run { .. }),
            "delivery stride should keep mocap run, got {:?}",
            anim.state
        );
        assert!(
            !matches!(anim.state, AnimState::BowlAction { .. }),
            "BowlAction during run-up sinks the bowler's feet below the pitch"
        );
    }

    /// The bowler used to be lerped linearly all the way to the crease, so he
    /// kept gliding at approach speed through the delivery stride and looked
    /// like he was sliding on the pitch. The stride must decelerate.
    #[test]
    fn bowler_plants_and_decelerates_through_the_delivery_stride() {
        let sample = |a: f32, b: f32| (bowler_runup_x(b) - bowler_runup_x(a)).abs() / (b - a);

        // Mid-approach speed vs speed at the very end of the delivery stride.
        let approach = sample(0.45, 0.55);
        let stride_end = sample(0.9, 1.0);
        assert!(
            stride_end < approach * 0.5,
            "delivery stride should slow to well under approach speed              (approach={approach:.2} m/p, stride_end={stride_end:.2} m/p)"
        );

        // And the approach itself should build speed rather than be uniform.
        let early = sample(0.05, 0.15);
        assert!(
            early < approach,
            "run-up should accelerate into the crease (early={early:.2}, approach={approach:.2})"
        );

        // Monotonic forward travel: never drift backwards down the pitch.
        let mut prev = bowler_runup_x(0.0);
        for i in 1..=100 {
            let x = bowler_runup_x(i as f32 / 100.0);
            assert!(x >= prev - 1e-4, "bowler moved backwards at p={i}");
            prev = x;
        }
    }

    /// Regression: `cleanup_after_match` can remove `ActiveMatch` mid-`Update`
    /// when deferred commands flush at a camera sync point. Gameplay systems
    /// must tolerate the missing resource and match-exit must run last.
    #[test]
    fn match_over_confirm_survives_active_match_teardown() {
        use crate::input::Action;
        use crate::state::AppState;
        use bevy::state::app::StatesPlugin;

        fn flush_active_match_removal(mut commands: Commands) {
            commands.remove_resource::<ActiveMatch>();
        }

        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin));
        app.init_state::<AppState>();
        app.insert_resource(Phase(PhaseEnum::MatchOver));
        app.insert_resource(PlayerInput {
            just_pressed: vec![Action::Confirm],
            ..Default::default()
        });
        app.insert_resource(minimal_active_match());
        app.insert_resource(CurrentDelivery(None));
        app.insert_resource(ReleaseInfo::default());
        app.insert_resource(ShotAttempt::default());
        app.insert_resource(WorldData::new());
        app.insert_resource(BoundaryRadius(65.0));
        app.insert_resource(CurrentLayout(geo::FieldLayout::standard()));
        app.insert_resource(CameraRig::default());
        app.insert_resource(Pending::default());
        app.insert_resource(RecentBalls::default());
        app.world_mut()
            .spawn((CricketBall, BallState::default(), BallFlags::default()));

        app.add_systems(
            Update,
            (
                flush_active_match_removal,
                sys_shot_input,
                sys_contact_watch,
                sys_pending_watch,
                sys_ball_physics,
            )
                .chain()
                .run_if(in_state(AppState::InMatch)),
        );

        app.world_mut()
            .resource_mut::<NextState<AppState>>()
            .set(AppState::InMatch);

        app.update();
    }

    #[test]
    fn match_intro_batter_positions_walk_from_boundary_to_creases() {
        let boundary = 65.0;
        let duration = 2.65;
        let (s0, n0) = match_intro_batter_positions(0.0, duration, boundary);
        let striker_goal = geo::BATSMAN_POS;
        let non_striker_goal = Vec2::new(-geo::PITCH_HALF_LEN + 1.6, 0.9);
        assert!(
            (s0 - intro_walk_start(striker_goal, boundary)).length() < 1e-4,
            "striker should start on the boundary"
        );
        assert!(
            (n0 - intro_walk_start(non_striker_goal, boundary)).length() < 1e-4,
            "non-striker should start on the boundary"
        );
        assert!(s0.length() > striker_goal.length() + 5.0);
        assert!(n0.length() > non_striker_goal.length() + 5.0);

        let (s1, n1) = match_intro_batter_positions(duration, duration, boundary);
        assert!((s1 - striker_goal).length() < 0.05);
        assert!((n1 - non_striker_goal).length() < 0.05);
    }

    #[test]
    fn match_intro_walk_progress_eases_and_finishes_before_hold_tail() {
        let duration = 2.65;
        let mid = match_intro_walk_progress(duration * 0.4, duration);
        let end = match_intro_walk_progress(duration, duration);
        assert!(mid > 0.2 && mid < 0.95);
        assert!((end - 1.0).abs() < 1e-4);
    }

    #[test]
    fn sys_match_intro_hands_over_to_ready_to_ball() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_systems(Update, sys_match_intro);

        app.world_mut().spawn((
            Figure {
                kind: FigureKind::Batter,
            },
            Transform::default(),
            Anim::default(),
        ));
        app.world_mut().spawn((
            Figure {
                kind: FigureKind::NonStriker,
            },
            Transform::default(),
            Anim::default(),
        ));

        app.world_mut()
            .insert_resource(Phase(PhaseEnum::MatchIntro { t: 0.0 }));
        app.world_mut().insert_resource(PlayerInput::default());
        app.world_mut().insert_resource(minimal_active_match());
        app.world_mut().insert_resource(BoundaryRadius(65.0));
        app.world_mut()
            .insert_resource(crate::game::audio::AudioSettings::default());
        app.world_mut()
            .insert_resource(crate::game::audio::CommentaryDurations::default());
        app.world_mut().insert_resource(CameraRig::default());
        prime_test_time(&mut app);

        let mut elapsed = 0.0_f32;
        let mut ready = false;
        while elapsed < 4.0 && !ready {
            advance_test_time(app.world_mut(), 0.05);
            elapsed += 0.05;
            app.update();
            ready = matches!(
                app.world().resource::<Phase>().0,
                PhaseEnum::ReadyToBall { .. }
            );
        }
        assert!(ready, "intro should transition to ReadyToBall");
        assert!(
            elapsed >= 2.0,
            "intro should not end instantly: {:.2}s",
            elapsed
        );
        assert!(elapsed < 3.5, "intro ran too long: {:.2}s", elapsed);
    }

    #[test]
    fn match_intro_should_finish_respects_grace_and_duration() {
        assert!(!match_intro_should_finish(0.4, 3.0, true));
        assert!(match_intro_should_finish(1.2, 3.0, true));
        assert!(match_intro_should_finish(3.0, 3.0, false));
    }

    #[test]
    fn ready_gate_should_release_respects_grace_and_timeout() {
        assert!(!ready_gate_should_release(0.2, true));
        assert!(ready_gate_should_release(0.55, true));
        assert!(ready_gate_should_release(12.0, false));
        assert!(!ready_gate_should_release(11.0, false));
    }

    #[test]
    fn ai_batting_inputs_picks_short_ball_pull() {
        let plan = build_plan(
            BowlStyle::Fast,
            0.0,
            12.5,
            DeliveryVariation::Stock,
            0.7,
            0.7,
        );
        let inputs = ai_batting_inputs(&plan, 0.85, 0.7, 0.5, 0.0, 1.0, || 0.5, |_| true)
            .expect("should swing");
        let kind = select_shot(inputs.0, inputs.1, inputs.2);
        assert!(matches!(
            kind,
            ShotKind::Pull | ShotKind::Hook | ShotKind::LateCut
        ));
    }

    #[test]
    fn ai_batting_inputs_defends_good_ball_when_not_chasing() {
        let plan = build_plan(
            BowlStyle::Medium,
            0.05,
            7.2,
            DeliveryVariation::Stock,
            0.7,
            0.7,
        );
        let inputs = ai_batting_inputs(&plan, 0.8, 0.4, 0.9, 1.0, 1.0, || 0.1, |_| true)
            .expect("should swing");
        assert_eq!(inputs.0, Footwork::Planted);
        assert_eq!(select_shot(inputs.0, inputs.1, inputs.2), ShotKind::Defend);
    }

    #[test]
    fn sys_match_intro_skips_to_ready_on_confirm_after_grace() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_systems(Update, sys_match_intro);

        app.world_mut().spawn((
            Figure {
                kind: FigureKind::Batter,
            },
            Transform::default(),
            Anim::default(),
        ));
        app.world_mut().spawn((
            Figure {
                kind: FigureKind::NonStriker,
            },
            Transform::default(),
            Anim::default(),
        ));

        app.world_mut()
            .insert_resource(Phase(PhaseEnum::MatchIntro { t: 1.15 }));
        app.world_mut().insert_resource(PlayerInput {
            just_pressed: vec![Action::Confirm],
            ..Default::default()
        });
        app.world_mut().insert_resource(minimal_active_match());
        app.world_mut().insert_resource(BoundaryRadius(65.0));
        app.world_mut()
            .insert_resource(crate::game::audio::AudioSettings::default());
        app.world_mut()
            .insert_resource(crate::game::audio::CommentaryDurations::default());
        app.world_mut().insert_resource(CameraRig::default());
        prime_test_time(&mut app);
        advance_test_time(app.world_mut(), 0.0);
        app.update();
        assert!(
            matches!(
                app.world().resource::<Phase>().0,
                PhaseEnum::ReadyToBall { .. }
            ),
            "confirm after grace should skip to ReadyToBall"
        );
    }

    /// A pull/hook-style shot travels leg-side-and-down-the-ground; the
    /// fielder sent to fetch it must be on that side (deep midwicket),
    /// never someone standing square on the off side. Regression check for
    /// the coordinate hand-off between shot direction and fielder chase
    /// (bug 2's "runs the wrong way").
    #[test]
    fn predict_outcome_sends_deep_midwicket_not_the_off_side() {
        let layout = geo::FieldLayout::standard();
        let fielder_pos: Vec<Vec2> = layout
            .positions
            .iter()
            .map(|fp| fp.world_pos(geo::BATSMAN_POS))
            .collect();
        let deep_midwicket_slot = layout
            .positions
            .iter()
            .position(|fp| fp.name == "Deep Midwicket")
            .unwrap();

        // Pull shot profile: angle -62 (leg side), elev 14, speed ~29.
        let dir = crate::core::angle_dir(-62.0);
        let speed = 29.0_f32;
        let elev: f32 = 14.0;
        let pos = Vec3::new(BAT_PLANE_X, 0.9, geo::BATSMAN_POS.y);
        let vel = Vec3::new(
            dir.x * speed,
            elev.to_radians().sin() * speed,
            dir.y * speed,
        );

        let pred = predict_outcome(pos, vel, &fielder_pos, 65.0);
        let chaser = match pred {
            Prediction::Runs { chaser, .. } => chaser,
            Prediction::Caught { slot } => slot,
            other => panic!("expected a ground/aerial fielding outcome, got {:?}", other),
        };
        assert_eq!(
            chaser, deep_midwicket_slot,
            "leg-side pull should send deep midwicket, not an off-side fielder"
        );
        // Whoever was picked must actually be on the leg side (negative
        // relative z), matching BATSMAN_POS's leg-side convention.
        assert!(fielder_pos[chaser].y < geo::BATSMAN_POS.y);
    }
}

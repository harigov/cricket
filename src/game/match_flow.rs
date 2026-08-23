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
use crate::core::rules::{BallOutcome, Dismissal};
use crate::core::teams::{BowlStyle, Player, batting_order, pick_bowlers};
use crate::game::ball::*;
use crate::game::fielding::{self, Brain, Fielder};
use crate::game::*;
use crate::input::{Action, PlayerInput};
use crate::render::camera_rig::{
    BallRecording, CamMode, CameraRig, PresentationState, ReplayState,
};
use crate::render::player::{
    Anim, AnimState, Figure, FigureKind, face_target, face_target_quat, spawn_figure,
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
    >,
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
                tf.rotation = face_target_quat(geo::BATSMAN_POS, bowler_end);
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

    scene.cam.mode = if am.user_batting() {
        CamMode::BattingEnd
    } else {
        CamMode::BowlingEnd
    };

    let user_bowling = am.user_bowling();
    if user_bowling {
        if input.pressed(Action::Confirm) {
            phase.0 = PhaseEnum::AimLength { t: 0.0, lock: None };
        }
    } else {
        // AI bowling: brief beat, then automatic run-in. Human batting can't
        // hurry it (that's the point of the wait).
        if *t > 0.9 {
            phase.0 = PhaseEnum::RunUp { p: 0.0 };
        }
    }
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
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut marker_q: Query<&mut Transform, With<AimMarker>>,
) {
    let PhaseEnum::AimLength { t, lock } = &mut phase.0 else {
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
                Transform::from_xyz(-5.0, 0.04, 0.0)
                    .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
                Visibility::default(),
            ))
            .id();
        scene.marker = Some(e);
    }

    let length = 8.0 + t.sin() * 5.5; // oscillates 2.5..13.5 m
    match *lock {
        None => {
            // Stage 1: lock the length.
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
        Some(locked_len) => {
            // Stage 2: sweep for line.
            let sweep = (*t * 1.4).sin() * 1.25;
            if let Some(e) = scene.marker
                && let Ok(mut tf) = marker_q.get_mut(e)
            {
                tf.translation.x = geo::PITCH_HALF_LEN - locked_len;
                tf.translation.z = sweep;
            }
            if input.pressed(Action::Confirm) {
                let scatter = (1.0 - skill) * 0.5 + 0.08;
                let plan = build_plan(
                    style,
                    sweep + gauss() * scatter,
                    (locked_len + gauss() * scatter * 2.5).max(1.5),
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

/// Build a DeliveryPlan from style + aimed line/length.
pub fn build_plan(style: BowlStyle, line_z: f32, length_from_stumps: f32) -> DeliveryPlan {
    let speed = style.base_speed() * 0.72; // playability scaling baked in
    let swing = match style {
        BowlStyle::Fast | BowlStyle::FastMedium => 0.9,
        BowlStyle::Medium => 0.4,
        BowlStyle::OffSpin => 0.15,
        BowlStyle::LegSpin => -0.15,
    };
    let turn = match style {
        BowlStyle::OffSpin => 2.4,
        BowlStyle::LegSpin => -2.6,
        _ => 0.0,
    };
    DeliveryPlan {
        speed,
        line_z,
        length_from_stumps,
        swing,
        turn,
        label: style.label().to_string(),
        wide: line_z.abs() > 1.35,
    }
}

/// AI bowler plan with skill-based scatter and situational variety.
fn ai_plan(bowler: &Player, pitch: crate::core::stadiums::PitchType) -> DeliveryPlan {
    let style = bowler.style.unwrap_or(BowlStyle::FastMedium);
    let skill = bowler.bowling as f32 / 100.0;
    let scatter = (1.25 - skill) * 0.55;
    // Line: usually tight, occasionally wider to set up
    let line = gauss() * 0.32 * scatter.max(0.25);

    // Length variety: yorkers, bouncers, and pitch-aware fuller/shorter bias
    let length = match style {
        s if s.is_spin() => {
            let roll = unit();
            if roll < 0.14 {
                3.2 + unit() * 1.6 // arm ball / yorker-ish
            } else if roll < 0.82 {
                6.2 + gauss() * 1.7 + pitch.turn_mul() * 0.22
            } else {
                9.2 + unit() * 2.4
            }
        }
        _ => {
            let roll = unit();
            if roll < 0.09 {
                2.0 + unit() * 1.3 // yorker
            } else if roll < 0.20 {
                12.2 + unit() * 1.6 // bouncer
            } else {
                7.0 + gauss() * 2.6
            }
        }
    }
    .clamp(2.0, 14.0)
        + match pitch {
            crate::core::stadiums::PitchType::Green => -0.35,
            crate::core::stadiums::PitchType::Dusty => 0.45,
            _ => 0.0,
        };

    let mut plan = build_plan(style, line, length.clamp(2.0, 14.0));
    // Speed variation including slower balls for seamers
    let mut speed_mul = 0.94 + unit() * 0.14;
    if !style.is_spin() && unit() < 0.13 {
        speed_mul *= 0.75;
        plan.label = format!("{} (slower)", plan.label);
    }
    // Extra swing on green tops occasionally
    if matches!(style, BowlStyle::Fast | BowlStyle::FastMedium)
        && pitch == crate::core::stadiums::PitchType::Green
        && unit() < 0.32
    {
        plan.swing *= 1.7;
    }
    plan.speed *= speed_mul;
    plan.wide = plan.line_z.abs() > 1.35;
    plan
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
    for (fig, mut tf, mut anim) in &mut runup.figs {
        if fig.kind == FigureKind::Bowler {
            tf.translation.x = bowler_runup_x(p.clamp(0.0, 1.0));
            tf.translation.y = 0.0;
            tf.rotation = face_target_quat(
                Vec2::new(tf.translation.x, tf.translation.z),
                geo::BATSMAN_POS,
            );
            anim.state = if p > 0.7 {
                AnimState::BowlAction { p: (p - 0.7) / 0.3 }
            } else {
                AnimState::Run { t: p * 4.0 }
            };
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
        for (fig, mut anim) in &mut shot.batters {
            if fig.kind == FigureKind::Batter {
                anim.state = AnimState::BatSwing { p: 0.0 };
            }
        }
        if !shot.attempt.pressed {
            shot.attempt.pressed = true;
            let offset = shot.rel.t - shot.rel.t_arrive;
            shot.attempt.offset = Some(offset);
            shot.attempt.loft = input.held(Action::Loft);
            shot.attempt.dir_x = input.move_vec.x;
            info!(
                "SHOT registered: offset {:.3}s (arrive {:.3}s, t {:.3}s) loft={} dir={:.2}",
                offset,
                shot.rel.t_arrive,
                shot.rel.t,
                shot.attempt.loft,
                shot.attempt.dir_x
            );
        }
    } else if let (Some(am), Some(wd)) = (shot.am.as_ref(), shot.wd.as_ref()) {
        if !shot.attempt.ai_scheduled && shot.rel.t > shot.rel.t_arrive - 0.45 {
        // AI batter: line/length aware decision
        shot.attempt.ai_scheduled = true;
        if plan.wide {
            return;
        }
        let batsman = am.striker(wd);
        let q = plan.quality_vs_batsman();
        let skill = batsman.batting as f32 / 100.0;
        // Fuller / good-length balls are easier to time; short balls harder
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
        // Defend good balls more often unless chasing hard
        let defend_bias = if q > 0.75 && agg < 0.6 { 0.18 } else { 0.0 };
        let swing_prob = (0.58 + agg * 0.38 - q * 0.32 - defend_bias).clamp(0.18, 0.96);
        if coin(swing_prob) {
            shot.attempt.pressed = true;
            shot.attempt.offset = Some((gauss() * sigma).clamp(-0.5, 0.5));
            // Direction mapped to line & length
            let mut preferred = 0.0f32;
            if plan.line_z > 0.45 {
                preferred = 0.55; // wide off -> square through off
            } else if plan.line_z < -0.30 {
                preferred = -0.55; // leg stump -> flick / pull leg side
            } else if plan.length_from_stumps < 4.5 {
                preferred = 0.10; // yorker -> straight
            } else if plan.length_from_stumps > 11.0 {
                preferred = -0.40; // bouncer -> pull leg side
                // short balls lofted more often
                shot.attempt.loft = coin((0.35 + agg * 0.45).clamp(0.1, 0.85));
            }
            if plan.length_from_stumps <= 11.0 && plan.length_from_stumps >= 4.5 {
                // good length: loft only when chasing or bad ball
                shot.attempt.loft = coin((agg * 0.45 * (1.25 - q)).clamp(0.05, 0.88));
            } else if shot.attempt.loft {
                // already set for short balls; keep
            }
            // Add variation around preferred
            let spread = 0.32 + (1.0 - skill) * 0.18;
            shot.attempt.dir_x = (preferred + (unit() * 2.0 - 1.0) * spread).clamp(-1.0, 1.0);
        }
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
    let (Some(mut am), Some(wd), Some(layout)) = (
        watch.am.as_mut(),
        watch.wd.as_ref(),
        watch.layout.as_ref(),
    ) else {
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
        &mut am,
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

    let ao = offset.abs();

    // ---- Play and miss / thick edge band ----
    if ao >= BEATEN_TIMING_THRESHOLD {
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

    let tier = if ao < TIER_PERFECT_MAX {
        Tier::Perfect
    } else if ao < TIER_GOOD_MAX {
        Tier::Good
    } else if ao < TIER_OKAY_MAX {
        Tier::Okay
    } else {
        Tier::Edge
    };

    // ---- Edged ----
    if tier == Tier::Edge && coin(EDGE_CARRY_CHANCE) {
        bs.dead = true;
        ctx.finalize(
            BallOutcome::Wicket(Dismissal::CaughtBehind { keeper: true }),
            "Edged & TAKEN behind!".into(),
        );
        return None;
    }

    // ---- Clean contact: build the exit velocity ----
    let (mut speed, mut elev): (f32, f32) = match tier {
        Tier::Perfect => (34.0, 9.0),
        Tier::Good => (29.0, 15.0),
        Tier::Okay => (23.0, 24.0),
        Tier::Edge => (11.0, 8.0),
    };
    speed *= skill.clamp(0.82, 1.15);
    if attempt.loft && tier != Tier::Edge {
        elev += 17.0;
        speed *= 1.12;
    }

    // Direction: input sweeps leg (-) to off (+); timing biases it too.
    let mut angle = attempt.dir_x * 80.0 + offset.signum() * ao * 260.0;
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
    let Some(mut am) = watch.am.as_mut() else {
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
            &mut am,
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
                        &mut am,
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
            &mut am,
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
    mut commands: Commands,
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
    } else if am.state.innings.over_complete() {
        phase.0 = PhaseEnum::OverBreak { t: 0.0 };
    } else {
        enter_ready(&mut commands, &mut phase.0);
    }
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

    // Rotate to the next bowler (v1: automatic selection).
    let team = am.fielding_team(&wd);
    let opts = crate::core::teams::pick_bowlers(team, 5);
    if !opts.is_empty() {
        let over = (am.state.innings.legal_balls / 6) as usize;
        let mut idx = over % opts.len();
        if Some(opts[idx]) == am.state.innings.previous_bowler && opts.len() > 1 {
            idx = (idx + 1) % opts.len();
        }
        am.bowler_player = opts[idx];
        am.state.innings.current_bowler = Some(opts[idx]);
    }
    enter_ready(&mut commands, &mut phase.0);
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
    let bowlers = pick_bowlers(&wd.teams[bowling_idx], 5);
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
        PhaseEnum::RunUp { .. } | PhaseEnum::AimLength { .. } | PhaseEnum::ReadyToBall { .. } => {
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
        PhaseEnum::OverBreak { .. } | PhaseEnum::InningsBreak | PhaseEnum::MatchOver => {
            rig.mode = CamMode::Broadcast;
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
            if matches!(*b, Brain::Chase) {
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

    #[test]
    fn sys_over_break_waits_about_one_point_three_seconds() {
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
        assert!(ready, "over break should transition to ready");
        assert!(
            elapsed >= 1.25,
            "over break ended too quickly: {:.2}s",
            elapsed
        );
        assert!(elapsed < 1.45, "over break took too long: {:.2}s", elapsed);
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
            Anim::default(),
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
        app.world_mut().spawn((
            CricketBall,
            BallState::default(),
            BallFlags::default(),
        ));

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
}

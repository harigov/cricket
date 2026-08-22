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
use crate::core::teams::{batting_order, pick_bowlers, BowlStyle, Player};
use crate::game::ball::*;
use crate::game::fielding::{self, Brain, Fielder};
use crate::game::*;
use crate::input::{Action, PlayerInput};
use crate::render::camera_rig::{CamMode, CameraRig};
use crate::render::player::{spawn_figure, Anim, AnimState, Figure, FigureKind};
use crate::state::RebuildScene;
use bevy::prelude::*;

/// Seconds per completed run between the wickets.
const RUN_SECONDS: f32 = 2.9;
/// How long results stay on screen.
const RESULT_PAUSE_SECS: f32 = 2.4;
/// Run-up duration in seconds.
const RUNUP_SECS: f32 = 1.7;

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
        s.set(x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407));
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
// Scene construction
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub fn spawn_match_scene(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    wd: &WorldData,
    am: &ActiveMatch,
) -> MatchScene {
    let stadium = &wd.stadiums[am.stadium];
    let stadium_root = crate::render::stadium::build_stadium(
        commands, meshes, materials, stadium);

    commands.insert_resource(BoundaryRadius(stadium.boundary_radius()));

    // Ball (parked at the keeper until released).
    let ball = commands
        .spawn((
            CricketBall,
            Mesh3d(meshes.add(Sphere::new(BALL_RADIUS))),
            MeshMaterial3d(materials.add(Color::srgb_u8(0xB5, 0x3A, 0x2E))),
            Transform::from_xyz(geo::PITCH_HALF_LEN + 1.2, BALL_RADIUS, 0.0),
            Visibility::default(),
            BallState::default(),
            BallFlags::default(),
        ))
        .id();

    let bat_team = am.batting_team(wd);
    let fld_team = am.fielding_team(wd);

    // Batters face the bowler (-X => yaw 0); non-striker turns around.
    let striker = spawn_figure(
        commands, meshes, materials,
        Vec3::new(geo::BATSMAN_POS.x, 0.0, geo::BATSMAN_POS.y),
        0.0,
        bat_team.primary_color, bat_team.secondary_color,
        FigureKind::Batter,
    );
    let non_striker = spawn_figure(
        commands, meshes, materials,
        Vec3::new(-geo::PITCH_HALF_LEN + 1.6, 0.0, 0.9),
        180.0,
        bat_team.primary_color, bat_team.secondary_color,
        FigureKind::NonStriker,
    );

    // Bowler at the top of his mark, facing down the pitch (+X).
    let bowler = spawn_figure(
        commands, meshes, materials,
        Vec3::new(-geo::PITCH_HALF_LEN - 8.0, 0.0, 0.35),
        180.0,
        fld_team.primary_color, fld_team.secondary_color,
        FigureKind::Bowler,
    );

    // Fielding side.
    let layout = geo::FieldLayout::standard();
    let fielders = fielding::spawn_field_side(
        commands, meshes, materials,
        &layout.positions,
        fld_team.primary_color, fld_team.secondary_color,
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
    for e in [&scene.ball, &scene.bowler, &scene.striker, &scene.non_striker] {
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

#[allow(clippy::too_many_arguments)]
pub fn sys_ready(
    mut phase: ResMut<Phase>,
    time: Res<Time>,
    input: Res<PlayerInput>,
    am: Res<ActiveMatch>,
    layout: Res<CurrentLayout>,
    mut figs: Query<(&Figure, &mut Transform, &mut Anim)>,
    mut fielders: Query<(&Fielder, &mut Brain, &mut Transform), Without<Figure>>,
    mut cam: ResMut<CameraRig>,
    scene: Res<MatchScene>,
) {
    let PhaseEnum::ReadyToBall { t } = &mut phase.0 else { return };
    *t += time.delta_secs();

    // Park everyone at their posts.
    for (fig, mut tf, mut anim) in &mut figs {
        anim.state = AnimState::Idle;
        match fig.kind {
            FigureKind::Batter => {
                tf.translation = Vec3::new(
                    geo::BATSMAN_POS.x, 0.0, geo::BATSMAN_POS.y);
                tf.rotation = Quat::IDENTITY;
            }
            FigureKind::NonStriker => {
                tf.translation =
                    Vec3::new(-geo::PITCH_HALF_LEN + 1.6, 0.0, 0.9);
                tf.rotation = Quat::from_rotation_y(std::f32::consts::PI);
            }
            FigureKind::Bowler => {
                tf.translation =
                    Vec3::new(-geo::PITCH_HALF_LEN - 8.0, 0.0, 0.35);
                tf.rotation = Quat::from_rotation_y(std::f32::consts::PI);
            }
            _ => {}
        }
    }
    for (f, _brain, mut tf) in &mut fielders {
        if let Some(fp) = layout.0.positions.get(f.slot) {
            let p = fp.world_pos(geo::BATSMAN_POS);
            tf.translation.x = p.x;
            tf.translation.z = p.y;
            let dir = geo::BATSMAN_POS - p;
            tf.rotation = Quat::from_rotation_y(
                crate::render::player::yaw_to_face(dir));
        }
    }

    cam.mode = if am.user_batting() { CamMode::BattingEnd }
               else { CamMode::BowlingEnd };

    let user_bowling = am.user_bowling();
    if user_bowling {
        if input.pressed(Action::Confirm) {
            phase.0 = PhaseEnum::AimLength { t: 0.0, lock: None };
        }
    } else {
        // AI bowling: brief beat, then automatic run-in. Human batting can't
        // hurry it (that's the point of the wait).
        if *t > 0.9 {
            info!("RUNUP START");
            phase.0 = PhaseEnum::RunUp { p: 0.0 };
        }
    }
    let _ = (&scene, &figs);
}

// ---------------------------------------------------------------------------
// Phase: AimLength (user bowling)
// ---------------------------------------------------------------------------

pub fn sys_aim(
    mut phase: ResMut<Phase>,
    input: Res<PlayerInput>,
    time: Res<Time>,
    am: Res<ActiveMatch>,
    wd: Res<WorldData>,
    mut scene: ResMut<MatchScene>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut marker_q: Query<&mut Transform, With<AimMarker>>,
) {
    let PhaseEnum::AimLength { t, lock } = &mut phase.0 else { return };
    *t += time.delta_secs();

    let bowler = am.bowler(&wd);
    let style = bowler.style.unwrap_or(BowlStyle::Medium);
    let skill = bowler.bowling as f32 / 100.0;

    if scene.marker.is_none() {
        let e = commands
            .spawn((
                AimMarker,
                Mesh3d(meshes.add(Sphere::new(0.09))),
                MeshMaterial3d(materials.add(Color::srgb(2.0, 1.2, 0.2))),
                Transform::from_xyz(-5.0, 0.06, 0.0),
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
                && let Ok(mut tf) = marker_q.get_mut(e) {
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
                && let Ok(mut tf) = marker_q.get_mut(e) {
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
pub fn build_plan(
    style: BowlStyle,
    line_z: f32,
    length_from_stumps: f32,
) -> DeliveryPlan {
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

#[allow(clippy::too_many_arguments)]
pub fn sys_runup(
    mut phase: ResMut<Phase>,
    time: Res<Time>,
    del: Res<CurrentDelivery>,
    am: Res<ActiveMatch>,
    wd: Res<WorldData>,
    mut figs: Query<(&Figure, &mut Transform, &mut Anim)>,
    mut ball_q: Query<&mut BallState, With<CricketBall>>,
    mut commands: Commands,
    _meshes: ResMut<Assets<Mesh>>,
    _materials: ResMut<Assets<StandardMaterial>>,
    _scene: ResMut<MatchScene>,
) {
    let PhaseEnum::RunUp { p } = &mut phase.0 else { return };
    *p += time.delta_secs() / RUNUP_SECS;
    let p = *p;

    // Bowler jogs in; delivery stride over the last 30%.
    for (fig, mut tf, mut anim) in &mut figs {
        if fig.kind == FigureKind::Bowler {
            let start_x = -geo::PITCH_HALF_LEN - 8.0;
            let end_x = geo::RELEASE_POINT.x - 0.4;
            tf.translation.x = start_x + (end_x - start_x) * p.clamp(0.0, 1.0);
            anim.state = if p > 0.7 {
                AnimState::BowlAction { p: (p - 0.7) / 0.3 }
            } else {
                AnimState::Run { t: p * 4.0 }
            };
        }
    }

    // Ball rides in the bowler's hand until release.
    if p < 1.0 {
        if let Ok(mut bs) = ball_q.single_mut() {
            for (fig, tf, _) in &figs {
                if fig.kind == FigureKind::Bowler {
                    bs.pos = Vec3::new(
                        tf.translation.x - 0.3,
                        1.6 + (p * 20.0).sin().abs() * 0.2,
                        tf.translation.z + 0.25,
                    );
                }
            }
        }
        return;
    }

    // ---- RELEASE ----
    let plan = del.0.clone().unwrap_or_else(|| {
        ai_plan(am.bowler(&wd), am.pitch(&wd))
    });
    commands.insert_resource(CurrentDelivery(Some(plan.clone())));

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

    if let Ok(mut bs) = ball_q.single_mut() {
        *bs = BallState::new_release(start, vel);
    }

    let est_t_arrive =
        (BAT_PLANE_X - start.x) / plan.speed.max(0.1) / BALL_TIME_SCALE;
    commands.insert_resource(ReleaseInfo {
        active: true,
        resolved: false,
        t: 0.0,
        t_arrive: est_t_arrive,
    });
    info!("BALL RELEASED: arrive ~{}s speed {}", est_t_arrive.round(), plan.speed.round());

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
    mut q: Query<
        (&mut BallState, &mut BallFlags, &mut Transform),
        With<CricketBall>,
    >,
) {
    if !matches!(phase.0, PhaseEnum::BallLive | PhaseEnum::ResultPause { .. }) {
        return;
    }
    let Ok((mut bs, mut flags, mut tf)) = q.single_mut() else { return };

    let settled =
        bs.dead && bs.pos.y <= BALL_RADIUS * 1.5 && bs.vel.length_squared() < 0.02;
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

#[allow(clippy::too_many_arguments)]
pub fn sys_shot_input(
    phase: ResMut<Phase>,
    time: Res<Time>,
    input: Res<PlayerInput>,
    am: Res<ActiveMatch>,
    wd: Res<WorldData>,
    del: Res<CurrentDelivery>,
    mut rel: ResMut<ReleaseInfo>,
    mut attempt: ResMut<ShotAttempt>,
) {
    let PhaseEnum::BallLive = phase.0 else { return };
    if !rel.active || rel.resolved {
        return;
    }
    rel.t += time.delta_secs();
    let Some(plan) = del.0.as_ref() else { return };

    if am.user_batting() {
        // Human batter: register the press the instant it happens.
        if !attempt.pressed && input.pressed(Action::Confirm) {
            attempt.pressed = true;
            attempt.offset = Some(rel.t - rel.t_arrive);
            attempt.loft = input.held(Action::Loft);
            attempt.dir_x = input.move_vec.x;
        }
    } else if !attempt.ai_scheduled && rel.t > rel.t_arrive - 0.45 {
        // AI batter: line/length aware decision
        attempt.ai_scheduled = true;
        if plan.wide {
            return;
        }
        let batsman = am.striker(&wd);
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
        let sigma = (0.045 + (1.0 - q) * 0.10 - (skill - 0.7) * 0.04 + length_factor)
            .clamp(0.028, 0.30);
        let agg = chase_pressure(
            am.state.innings.target,
            am.state.innings.runs,
            am.state.innings.legal_balls,
        );
        // Defend good balls more often unless chasing hard
        let defend_bias = if q > 0.75 && agg < 0.6 { 0.18 } else { 0.0 };
        let swing_prob = (0.58 + agg * 0.38 - q * 0.32 - defend_bias).clamp(0.18, 0.96);
        if coin(swing_prob) {
            attempt.pressed = true;
            attempt.offset = Some((gauss() * sigma).clamp(-0.5, 0.5));
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
                attempt.loft = coin((0.35 + agg * 0.45).clamp(0.1, 0.85));
            }
            if plan.length_from_stumps <= 11.0 && plan.length_from_stumps >= 4.5 {
                // good length: loft only when chasing or bad ball
                attempt.loft = coin((agg * 0.45 * (1.25 - q)).clamp(0.05, 0.88));
            } else if attempt.loft {
                // already set for short balls; keep
            }
            // Add variation around preferred
            let spread = 0.32 + (1.0 - skill) * 0.18;
            attempt.dir_x = (preferred + (unit() * 2.0 - 1.0) * spread).clamp(-1.0, 1.0);
        }
    }
}

/// Required run rate pressure 0..1 for the chasing side.
fn chase_pressure(target: Option<u32>, runs: u32, balls: u32) -> f32 {
    match target {
        None => 0.55,
        Some(t) => {
            let need = t.saturating_sub(runs) as f32;
            let overs_left = ((120 - balls.min(120)) as f32 / 6.0).max(0.5);
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
#[allow(clippy::too_many_arguments)]
pub fn sys_contact_watch(
    mut commands: Commands,
    mut phase: ResMut<Phase>,
    mut am: ResMut<ActiveMatch>,
    wd: Res<WorldData>,
    mut rel: ResMut<ReleaseInfo>,
    attempt: Res<ShotAttempt>,
    del: Res<CurrentDelivery>,
    br: Res<BoundaryRadius>,
    mut ball_q: Query<(&mut BallState, &mut BallFlags), With<CricketBall>>,
    gts: Query<&GlobalTransform, With<Fielder>>,
    layout: Res<CurrentLayout>,
    mut chasers: Query<(Entity, &Fielder, &mut Brain)>,
) {
    let PhaseEnum::BallLive = phase.0 else { return };
    if !rel.active || rel.resolved {
        return;
    }
    let Ok((mut bs, flags)) = ball_q.single_mut() else { return };
    if !flags.crossed_bat_plane {
        return;
    }
    rel.resolved = true;
    let Some(plan) = del.0.as_ref().cloned() else { return };

    // Snapshot fielder post positions.
    let fielder_pos: Vec<Vec2> = gts
        .iter()
        .map(|g| Vec2::new(g.translation().x, g.translation().z))
        .collect();

    let batting_skill = am.striker(&wd).batting as f32 / 100.0;
    let chaser_slot = resolve_at_bat(
        &mut commands,
        &mut phase.0,
        &mut am,
        &plan,
        &attempt,
        &mut bs,
        &fielder_pos,
        &layout,
        br.0,
        batting_skill,
    );

    if let Some(slot) = chaser_slot {
        for (_e, f, mut brain) in &mut chasers {
            if f.slot == slot {
                *brain = Brain::Chase;
            }
        }
    }
}

/// Decide what happens when the ball reaches the bat. Returns the fielder
/// slot that should chase (if any). Immediate outcomes finalise the ball
/// right here; struck balls insert a scripted `Pending`.
#[allow(clippy::too_many_arguments)]
fn resolve_at_bat(
    commands: &mut Commands,
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
    let skill = batting_skill;

    // ---- No shot offered ----
    let Some(offset) = attempt.offset else {
        bs.dead = true;
        if hits_stumps(bs) {
            finalize_ball(commands, phase_enum, am,
                BallOutcome::Wicket(Dismissal::Bowled),
                "BOWLED! No shot offered.".into());
        } else if plan.wide {
            finalize_ball(commands, phase_enum, am, BallOutcome::Wide, "Wide!".into());
        } else {
            finalize_ball(commands, phase_enum, am, BallOutcome::Runs(0),
                "Shouldered arms. Dot ball.".into());
        }
        return None;
    };

    let ao = offset.abs();

    // ---- Play and miss / thick edge band ----
    if ao >= 0.27 {
        bs.dead = true;
        if hits_stumps(bs) {
            finalize_ball(commands, phase_enum, am,
                BallOutcome::Wicket(Dismissal::Bowled), "BOWLED!".into());
        } else if plan.wide {
            finalize_ball(commands, phase_enum, am, BallOutcome::Wide, "Wide!".into());
        } else {
            finalize_ball(commands, phase_enum, am, BallOutcome::Runs(0),
                "Beaten! Past the edge.".into());
        }
        return None;
    }

    let tier = if ao < 0.055 { Tier::Perfect }
        else if ao < 0.115 { Tier::Good }
        else if ao < 0.19 { Tier::Okay }
        else { Tier::Edge };

    // ---- Edged ----
    if tier == Tier::Edge && coin(0.62) {
        bs.dead = true;
        finalize_ball(commands, phase_enum, am,
            BallOutcome::Wicket(Dismissal::CaughtBehind { keeper: true }),
            "Edged & TAKEN behind!".into());
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
        if unit() < 0.45 { angle = -angle; }
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
            BallOutcome::Six, "MAXIMUM! That's out of the ground!".into(),
            0, 3.4, None),
        Prediction::Four => (
            BallOutcome::Four, "FOUR! Crashed to the rope.".into(),
            0, 3.4, None),
        Prediction::Caught { slot } => {
            let name = layout.0.positions.get(*slot)
                .map(|fp| fp.name.to_string())
                .unwrap_or_else(|| "fielder".into());
            (
                BallOutcome::Wicket(Dismissal::Caught { fielder: 0 }),
                format!("CAUGHT at {}!", name),
                0, 3.2, Some(*slot))
        }
        Prediction::Runs { n, gamble, risky, chaser } => {
            let total = n + usize::from(*gamble);
            if *gamble && *risky && coin(0.16) {
                (
                    BallOutcome::WicketAndRuns(Dismissal::RunOut, *n as u8),
                    "RUN OUT going for the extra!".into(),
                    (*n as u32 + 1).min(3),
                    RUN_SECONDS * (*n as f32 + 1.1) + 0.6,
                    Some(*chaser))
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
                    Some(*chaser))
            }
        }
    };

    commands.insert_resource(Pending(Some(PendingOutcome {
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

enum Prediction {
    Six,
    Four,
    Caught { slot: usize },
    Runs { n: usize, gamble: bool, risky: bool, chaser: usize },
}

fn predict_outcome(
    pos: Vec3,
    vel: Vec3,
    fielders: &[Vec2],
    boundary_r: f32,
) -> Prediction {
    let mut p = pos;
    let mut v = vel;
    let dt = 0.04_f32;
    let mut bounced = false;
    let mut crossed_rope = false;
    let mut stop_pos = p;
    let mut t_total = 0.0_f32;
    let mut landing: Option<(Vec2, f32)> = None;

    for _ in 0..400 {
        v.y -= 9.81 * dt;
        p += v * dt;
        t_total += dt;
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
            v.x *= 1.0 - 2.4 * dt;
            v.z *= 1.0 - 2.4 * dt;
        }
        stop_pos = p;
        if p.y < BALL_RADIUS * 2.0 && v.x.abs() + v.z.abs() < 0.4 {
            break;
        }
    }

    if crossed_rope {
        return if bounced { Prediction::Four } else { Prediction::Six };
    }

    // Aerial catch near the landing spot.
    if let Some((land, t_air)) = landing {
        if t_air > 0.75 && t_air < 4.5 {
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
    Prediction::Runs { n, gamble, risky, chaser: nearest.0 }
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
    phase_enum: &mut PhaseEnum,
    am: &mut ActiveMatch,
    outcome: BallOutcome,
    text: String,
) {
    am.state.innings.apply_ball(&outcome);
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

#[allow(clippy::too_many_arguments)]
pub fn sys_pending_watch(
    mut commands: Commands,
    time: Res<Time>,
    mut phase: ResMut<Phase>,
    mut am: ResMut<ActiveMatch>,
    _rel: Res<ReleaseInfo>,
    br: Res<BoundaryRadius>,
    mut pending: ResMut<Pending>,
    mut ball_q: Query<&mut BallState, With<CricketBall>>,
    fielders: Query<(&Fielder, &GlobalTransform)>,
) {
    let PhaseEnum::BallLive = phase.0 else { return };
    let Some(p) = pending.0.as_mut() else { return };

    // Physical early triggers.
    let Ok(bs) = ball_q.single() else { return };
    let flat = Vec2::new(bs.pos.x, bs.pos.z);

    if p.boundary && flat.length() > br.0 {
        let (o, _) = (p.outcome.clone(), p.text.clone());
        finish_pending(&mut commands, &mut phase, &mut am, &mut pending, &mut ball_q, o);
        return;
    }

    if p.aerial_catch
        && !bs.bounced
        && bs.vel.y < 0.0
        && bs.pos.y < 2.6
        && bs.pos.y > 0.2
    {
        if let Some(slot) = p.chaser_slot {
            for (f, gt) in &fielders {
                if f.slot == slot {
                    let fp = Vec2::new(gt.translation().x, gt.translation().z);
                    if (fp - flat).length() < 1.5 {
                        let o = p.outcome.clone();
                        finish_pending(&mut commands, &mut phase, &mut am,
                            &mut pending, &mut ball_q, o);
                        return;
                    }
                }
            }
        }
    }

    // Timer fallback.
    p.apply_in -= time.delta_secs();
    if p.apply_in <= 0.0 {
        let o = p.outcome.clone();
        finish_pending(&mut commands, &mut phase, &mut am, &mut pending, &mut ball_q, o);
    }
}

fn finish_pending(
    commands: &mut Commands,
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
    finalize_ball(commands, &mut phase.0, am, outcome, text);
}

// ---------------------------------------------------------------------------
// Runner animation (batters shuttling between wickets)
// ---------------------------------------------------------------------------

fn flerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
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

    for (fig, mut tf, mut anim) in &mut figs {
        match fig.kind {
            FigureKind::Batter => {
                if p.runs_anim == 0 {
                    continue;
                }
                let u = p.elapsed / RUN_SECONDS;
                let legs = u.floor();
                let bob = (p.elapsed * 9.5).sin().abs() * 0.09;
                if legs as i32 >= p.runs_anim as i32 {
                    let done = legs as i32 % 2 == 1;
                    tf.translation = Vec3::new(
                        if done { n_crease } else { s_crease },
                        0.0,
                        geo::BATSMAN_POS.y,
                    );
                    anim.state = AnimState::Idle;
                    continue;
                }
                let frac = u - legs;
                let tri = if legs as i32 % 2 == 0 { frac } else { 1.0 - frac };
                let x = flerp(s_crease, n_crease, tri);
                let z = geo::BATSMAN_POS.y
                    + if legs as i32 % 2 == 0 { 0.45 } else { -0.45 };
                tf.translation = Vec3::new(x, bob, z);
                anim.state = AnimState::Run { t: p.elapsed };
            }
            FigureKind::NonStriker => {
                if p.runs_anim == 0 {
                    continue;
                }
                let u = p.elapsed / RUN_SECONDS;
                let legs = u.floor();
                let bob = (p.elapsed * 9.5 + 0.6).sin().abs() * 0.09;
                if legs as i32 >= p.runs_anim as i32 {
                    let done = legs as i32 % 2 == 1;
                    tf.translation = Vec3::new(
                        if done { s_crease } else { n_crease },
                        0.0,
                        0.9,
                    );
                    anim.state = AnimState::Idle;
                    continue;
                }
                let frac = u - legs;
                let tri = if legs as i32 % 2 == 0 { frac } else { 1.0 - frac };
                let x = flerp(n_crease, s_crease, tri);
                let z = if legs as i32 % 2 == 0 { 0.45 } else { -0.45 };
                tf.translation = Vec3::new(x, bob, z);
                anim.state = AnimState::Run { t: p.elapsed };
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
    mut am: ResMut<ActiveMatch>,
) {
    let PhaseEnum::ResultPause { t, .. } = &mut phase.0 else { return };
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
    mut am: ResMut<ActiveMatch>,
    wd: Res<WorldData>,
) {
    let PhaseEnum::OverBreak { t } = &mut phase.0 else { return };
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
    mut am: ResMut<ActiveMatch>,
    wd: Res<WorldData>,
    mut rebuild: MessageWriter<RebuildScene>,
) {
    let PhaseEnum::InningsBreak = phase.0 else { return };
    if !input.pressed(Action::Confirm) {
        return;
    }

    // The chasing side is currently teams[1]; start_chase swaps them.
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

pub fn sys_camera_modes(
    phase: Res<Phase>,
    am: Res<ActiveMatch>,
    pending: Res<Pending>,
    mut rig: ResMut<CameraRig>,
) {
    match phase.0 {
        PhaseEnum::BallLive | PhaseEnum::ResultPause { .. } => {
            if pending.0.is_some() {
                rig.mode = CamMode::FollowBall;
            } else if am.user_bowling() {
                rig.mode = CamMode::BowlingEnd;
            } else {
                rig.mode = CamMode::BattingEnd;
            }
        }
        PhaseEnum::OverBreak { .. }
        | PhaseEnum::InningsBreak
        | PhaseEnum::MatchOver => rig.mode = CamMode::Broadcast,
        _ => {} // ready/aim/runup set their own mode in sys_ready
    }
}

/// Reset fielder brains when a new delivery cycle starts.
pub fn fielding_brain_reset(
    phase: Res<Phase>,
    mut last: Local<u8>,
    mut brains: Query<&mut Brain>,
) {
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

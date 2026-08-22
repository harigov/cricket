// Roadmap features (LBW, stumping, replays, extra cameras…) are modelled
// but not yet wired into gameplay, so some items are intentionally unused.
#![allow(dead_code)]

mod core;
mod game;
mod input;
mod render;
mod state;
mod ui;

use bevy::prelude::*;
use bevy::render::view::screenshot::Screenshot;
use game::match_flow::{self, MatchScene};
use game::*;
use render::camera_rig::CameraRig;
use state::{AppState, RebuildScene};

/// Gameplay systems only run while the match resources actually exist
/// (they are torn down slightly before the state flips on exit).
fn in_live_match() -> impl bevy::ecs::schedule::SystemCondition<()> + Clone {
    in_state(AppState::InMatch).and(resource_exists::<ActiveMatch>)
}

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Willow Cricket".into(),
                    ..default()
                }),
                ..default()
            }),
        )
        .init_state::<AppState>()
        .add_message::<RebuildScene>()
        .insert_resource(WorldData::new())
        .insert_resource(CameraRig::default())
        .add_plugins((
            input::InputPlugin,
            game::GameplayPlugin,
            game::audio::AudioPlugin,
            render::RenderPlugin,
            ui::UiPlugin,
        ))
        .add_systems(Startup, setup_basics)
        .add_systems(OnEnter(AppState::InMatch), enter_match)
        .add_systems(OnExit(AppState::InMatch), exit_match)
        .add_systems(
            Update,
            handle_rebuild_scene.run_if(in_state(AppState::InMatch)),
        )
        .add_systems(
            Update,
            (
                match_flow::sys_ball_physics,
                match_flow::sys_shot_input,
                match_flow::sys_contact_watch,
                match_flow::sys_pending_watch,
                match_flow::sys_runners,
            )
                .chain()
                .run_if(in_live_match()),
        )
        .add_systems(
            Update,
            (
                match_flow::sys_ready,
                match_flow::sys_aim,
                match_flow::sys_runup,
                match_flow::sys_result_pause,
                match_flow::sys_over_break,
                match_flow::sys_innings_break,
                match_flow::sys_camera_modes,
                match_flow::fielding_brain_reset,
            )
                .run_if(in_live_match()),
        )
        .add_systems(
            Update,
            game::fielding::chase_system.run_if(in_live_match()),
        )
        .add_systems(
            Update,
            (
                render::camera_rig::camera_toggle_system,
                game::match_flow::wicket_shake_trigger,
            )
                .run_if(in_live_match()),
        )
        .add_systems(
            Update,
            (
                game::ball::trail_spawn_system,
                game::ball::trail_fade_system,
            )
                .run_if(in_live_match()),
        )
        .add_systems(
            Update,
            ui::menus::handle_match_exit.run_if(in_state(AppState::InMatch)),
        )
        .add_systems(Update, debug_screenshot)
        .add_systems(PreUpdate, autotest_drive.after(input::poll_input))
        .run();
}

fn save_shot(commands: &mut Commands, path: String) {
    info!("Saving screenshot to {path}");
    commands
        .spawn(Screenshot::primary_window())
        .observe(bevy::render::view::screenshot::save_to_disk(path));
}

/// F12 saves a screenshot of the primary window.
fn debug_screenshot(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut counter: Local<u32>,
) {
    if keys.just_pressed(KeyCode::F12) {
        let n = *counter;
        *counter += 1;
        save_shot(&mut commands, format!("/tmp/opencode/shot-{n}.png"));
    }
}

// ---------------------------------------------------------------------------
// Automated smoke-test driver (enabled with CRICKET_AUTOTEST=1):
// walks the menus into a match, plays a few deliveries and saves
// screenshots along the way.
// ---------------------------------------------------------------------------

fn autotest_drive(
    time: Res<Time<bevy::time::Real>>,
    mut input: ResMut<input::PlayerInput>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
    mut t: Local<f32>,
    mut last_press: Local<u32>,
    mut last_milestone: Local<u32>,
    mut last_swing_t: Local<f32>,
) {
    let mode = std::env::var("CRICKET_AUTOTEST").unwrap_or_default();
    if mode != "1" && mode != "tournament" && mode != "settings" {
        return;
    }
    *t += time.delta_secs();
    let now = *t;

    // Scripted menu presses per mode
    let presses: [(f32, input::Action); 7] = if mode == "tournament" {
        [
            (2.0, input::Action::Next),    // highlight Tournament
            (3.5, input::Action::Confirm), // enter Tournament
            (5.0, input::Action::Confirm), // pick your team -> bracket
            (8.5, input::Action::Confirm), // play semifinal
            (12.0, input::Action::Confirm),
            (14.0, input::Action::Confirm),
            (16.0, input::Action::Confirm),
        ]
    } else if mode == "settings" {
        [
            (2.0, input::Action::Next),    // highlight Tournament
            (2.6, input::Action::Next),    // highlight Settings
            (3.5, input::Action::Confirm), // enter Settings
            (6.0, input::Action::Next),    // move to SFX row
            (6.5, input::Action::Right),   // bump volume
            (7.5, input::Action::Cancel),  // back to main
            (99.0, input::Action::Confirm),
        ]
    } else {
        [
            (2.0, input::Action::Confirm), // Quick Match -> team select
            (3.5, input::Action::Confirm), // pick your team
            (5.0, input::Action::Confirm), // pick opponent
            (6.5, input::Action::Confirm), // overs
            (8.0, input::Action::Confirm), // stadium (random)
            (9.5, input::Action::Confirm), // bat first -> match starts
            (99.0, input::Action::Confirm),
        ]
    };
    for (i, (when, action)) in presses.iter().enumerate() {
        let step = i as u32 + 1;
        if now >= *when && *last_press < step {
            input.just_pressed.push(*action);
            *last_press = step;
            info!("AUTOTEST: menu press #{step} ({action:?})");
        }
    }

    // In-match: swing periodically once play is under way.
    if now > 14.0 && now - *last_swing_t >= 3.0 {
        *last_swing_t = now;
        input.just_pressed.push(input::Action::Confirm);
        info!("AUTOTEST: shot swing @ {:.1}s", now);
    }

    // Milestones: screenshots + clean exit.
    let milestones = if mode == "tournament" {
        [1.5_f32, 5.0, 7.0, 14.0]
    } else if mode == "settings" {
        [1.5_f32, 5.0, 6.8, 8.5]
    } else {
        [1.5_f32, 16.0, 30.0, 45.0]
    };
    for (i, when) in milestones.iter().enumerate() {
        let step = 100 + i as u32;
        if now >= *when && *last_milestone < step {
            save_shot(&mut commands, format!("/tmp/opencode/auto-{i}.png"));
            *last_milestone = step;
        }
    }
    let end = if mode == "tournament" {
        20.0
    } else if mode == "settings" {
        10.0
    } else {
        50.0
    };
    if now >= end && *last_milestone < 200 {
        *last_milestone = 200;
        exit.write(AppExit::Success);
    }
}

fn setup_basics(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    use bevy::pbr::{DistanceFog, FogFalloff};

    // Warm late-afternoon sun
    commands.spawn((
        DirectionalLight {
            illuminance: 24_000.0,
            shadows_enabled: true,
            color: Color::srgb(1.0, 0.98, 0.92),
            ..default()
        },
        Transform::from_translation(Vec3::new(-45.0, 68.0, 22.0))
            .looking_at(Vec3::ZERO, Vec3::Y),
    ));
    // Main 3D camera (UI renders onto the primary window camera).
    commands.spawn((
        Camera {
            clear_color: bevy::camera::ClearColorConfig::Custom(
                Color::srgb(0.53, 0.81, 0.98),
            ),
            ..default()
        },
        Camera3d::default(),
        Transform::from_xyz(24.0, 9.0, 2.0)
            .looking_at(Vec3::new(-10.0, 1.0, 0.0), Vec3::Y),
        IsDefaultUiCamera,
        DistanceFog {
            color: Color::srgba(0.68, 0.80, 0.94, 1.0),
            falloff: FogFalloff::Linear {
                start: 55.0,
                end: 145.0,
            },
            ..default()
        },
        AmbientLight {
            color: Color::srgb(0.82, 0.88, 1.0),
            brightness: 1300.0,
            affects_lightmapped_meshes: true,
        },
    ));
    // Warm-up the mesh/material registries.
    let _ = meshes.add(Sphere::new(0.1));
    let _ = materials.add(Color::WHITE);
}

/// Enter the match state: create ActiveMatch + first scene.
fn enter_match(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    wd: Res<WorldData>,
    setup: Res<MatchSetup>,
) {
    info!("Entering match");
    let am = build_active_match(&setup, &wd);
    let scene =
        match_flow::spawn_match_scene(&mut commands, &mut meshes, &mut materials, &wd, &am);
    commands.insert_resource(am);
    commands.insert_resource(scene);
    commands.insert_resource(CurrentDelivery(None));
    commands.insert_resource(Phase(PhaseEnum::ReadyToBall { t: 0.0 }));
}

/// Tear down the live scene when leaving the match.
fn exit_match(
    mut commands: Commands,
    scene: Option<Res<MatchScene>>,
) {
    if let Some(s) = scene.as_deref() {
        match_flow::despawn_match_scene(&mut commands, s);
    }
    commands.remove_resource::<ActiveMatch>();
    commands.remove_resource::<CurrentDelivery>();
    commands.remove_resource::<Pending>();
    commands.insert_resource(Phase(PhaseEnum::Idle));
}

/// Innings changes rebuild the fielding/batting sides.
#[allow(clippy::too_many_arguments)]
fn handle_rebuild_scene(
    mut ev: MessageReader<RebuildScene>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    wd: Res<WorldData>,
    am: Option<Res<ActiveMatch>>,
    scene: Option<Res<MatchScene>>,
) {
    if ev.is_empty() {
        return;
    }
    ev.clear();
    if let Some(s) = scene.as_deref() {
        match_flow::despawn_match_scene(&mut commands, s);
    }
    if let Some(am) = am.as_deref() {
        let new_scene = match_flow::spawn_match_scene(
            &mut commands, &mut meshes, &mut materials, &wd, am);
        commands.insert_resource(new_scene);
        commands.insert_resource(Phase(PhaseEnum::ReadyToBall { t: 0.0 }));
    }
}

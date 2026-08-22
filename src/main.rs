mod core;
mod game;
mod input;
mod render;
mod state;
mod ui;

use bevy::prelude::*;
use game::match_flow::{self, MatchScene};
use game::*;
use render::camera_rig::{update_camera, CameraRig};
use state::{AppState, RebuildScene};

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
                .run_if(in_state(AppState::InMatch)),
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
                .run_if(in_state(AppState::InMatch)),
        )
        .add_systems(
            Update,
            game::fielding::chase_system.run_if(in_state(AppState::InMatch)),
        )
        .add_systems(
            Update,
            ui::menus::handle_match_exit.run_if(in_state(AppState::InMatch)),
        )
        .run();
}

fn setup_basics(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Main 3D camera (UI renders onto the primary window camera).
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(24.0, 9.0, 2.0)
            .looking_at(Vec3::new(-10.0, 1.0, 0.0), Vec3::Y),
    ));
    // Sun + soft ambient fill.
    commands.spawn((
        DirectionalLight {
            illuminance: 18_000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_translation(Vec3::new(-40.0, 80.0, 30.0))
            .looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        AmbientLight {
            color: Color::srgb(0.75, 0.82, 0.95),
            brightness: 900.0,
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

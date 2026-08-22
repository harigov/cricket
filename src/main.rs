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
        .register_type::<Transform>()
        .register_type::<GlobalTransform>()
        .register_type::<Name>()
        .register_type::<Visibility>()
        .register_type::<InheritedVisibility>()
        .register_type::<ViewVisibility>()
        .init_state::<AppState>()
        .add_message::<RebuildScene>()
        .insert_resource(WorldData::new())
        .insert_resource(CameraRig::default())
        .insert_resource(StadiumTime::Day)
        .add_plugins((
            input::InputPlugin,
            game::GameplayPlugin,
            game::audio::AudioPlugin,
            render::RenderPlugin,
            ui::UiPlugin,
        ))
        .add_systems(Startup, setup_basics)
        .add_systems(Update, (update_stadium_time, toggle_day_night))
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

#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug)]
pub enum StadiumTime { Day, Night }
impl Default for StadiumTime { fn default() -> Self { StadiumTime::Day } }

#[derive(Component)]
struct DayLight;
#[derive(Component)]
struct NightLight;
#[derive(Component)]
struct SkySphere;

fn setup_basics(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    use bevy::pbr::{DistanceFog, FogFalloff};

    // --- Sky sphere with procedural gradient texture (day: blue, night: starry) ---
    let sky_texture = images.add(create_sky_texture(false));
    let sky_mat = materials.add(StandardMaterial {
        base_color_texture: Some(sky_texture),
        unlit: true,
        cull_mode: None,
        double_sided: true,
        ..default()
    });
    commands.spawn((
        SkySphere,
        Mesh3d(meshes.add(Sphere::new(220.0).mesh().uv(32, 32))),
        MeshMaterial3d(sky_mat),
        Transform::from_translation(Vec3::Y * -6.0),
        Visibility::default(),
    ));

    // Day sun (warm late-afternoon)
    commands.spawn((
        DayLight,
        DirectionalLight {
            illuminance: 28_000.0,
            shadows_enabled: true,
            color: Color::srgb(1.0, 0.97, 0.88),
            ..default()
        },
        Transform::from_translation(Vec3::new(-45.0, 68.0, 22.0))
            .looking_at(Vec3::ZERO, Vec3::Y),
    ));
    // Night moon + floodlights (cool, dimmer sun + bright spots)
    commands.spawn((
        NightLight,
        DirectionalLight {
            illuminance: 2_200.0,
            shadows_enabled: true,
            color: Color::srgb(0.72, 0.78, 1.0),
            ..default()
        },
        Transform::from_translation(Vec3::new(30.0, 55.0, -18.0))
            .looking_at(Vec3::ZERO, Vec3::Y),
        Visibility::Hidden,
    ));
    for (x, z) in [(-42.0, 38.0), (42.0, 38.0), (-42.0, -38.0), (42.0, -38.0)] {
        commands.spawn((
            NightLight,
            PointLight {
                intensity: 1_800_000.0,
                range: 90.0,
                radius: 2.0,
                shadows_enabled: true,
                color: Color::srgb(1.0, 0.96, 0.88),
                ..default()
            },
            Transform::from_translation(Vec3::new(x, 26.0, z)),
            Visibility::Hidden,
        ));
    }

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
    ));
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.82, 0.88, 1.0),
        brightness: 1300.0,
        affects_lightmapped_meshes: true,
    });
    commands.insert_resource(StadiumTime::Day);
    // Warm-up the mesh/material registries.
    let _ = meshes.add(Sphere::new(0.1));
    let _ = materials.add(Color::WHITE);
}

fn create_sky_texture(night: bool) -> Image {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let size = 512u32;
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        let v = y as f32 / size as f32; // 0 bottom, 1 top
        let col = if night {
            // Night: deep navy top, horizon dark blue, stars
            let base = Color::srgb(
                0.04 + v * 0.06,
                0.06 + v * 0.08,
                0.14 + v * 0.18,
            );
            let mut srgba = base.to_srgba();
            // Add stars: pseudo-random white dots
            let hash = ((y * 7919 + (y % 7) * 997) % 512) as f32;
            if hash < 2.0 && v > 0.35 {
                srgba.red = 1.0; srgba.green = 1.0; srgba.blue = 1.0;
            }
            srgba
        } else {
            // Day: horizon pale blue -> zenith deep blue
            let t = v.powf(0.9);
            Color::srgb(
                0.62 + t * 0.18,
                0.78 + t * 0.12,
                0.95 + t * 0.03,
            ).to_srgba()
        };
        for _ in 0..size {
            data.extend_from_slice(&[
                (col.red * 255.0) as u8,
                (col.green * 255.0) as u8,
                (col.blue * 255.0) as u8,
                255,
            ]);
        }
    }
    Image::new(
        Extent3d { width: size, height: size, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

fn update_stadium_time(
    time: Res<StadiumTime>,
    mut day_lights: Query<&mut Visibility, (With<DayLight>, Without<NightLight>)>,
    mut night_lights: Query<&mut Visibility, (With<NightLight>, Without<DayLight>)>,
    mut sky_q: Query<&mut MeshMaterial3d<StandardMaterial>, With<SkySphere>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut fog_q: Query<&mut DistanceFog>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut cam_q: Query<&mut Camera>,
) {
    let is_night = *time == StadiumTime::Night;
    for mut v in &mut day_lights { *v = if is_night { Visibility::Hidden } else { Visibility::Visible }; }
    for mut v in &mut night_lights { *v = if is_night { Visibility::Visible } else { Visibility::Hidden }; }
    // Fog and ambient
    if let Ok(mut fog) = fog_q.single_mut() {
        fog.color = if is_night { Color::srgba(0.08, 0.10, 0.16, 1.0) } else { Color::srgba(0.68, 0.80, 0.94, 1.0) };
    }
    {
        let new = if is_night {
            GlobalAmbientLight { color: Color::srgb(0.35, 0.38, 0.55), brightness: 420.0, affects_lightmapped_meshes: true }
        } else {
            GlobalAmbientLight { color: Color::srgb(0.82, 0.88, 1.0), brightness: 1300.0, affects_lightmapped_meshes: true }
        };
        *ambient = new;
    }
    if let Ok(mut cam) = cam_q.single_mut() {
        cam.clear_color = bevy::camera::ClearColorConfig::Custom(
            if is_night { Color::srgb(0.02, 0.03, 0.08) } else { Color::srgb(0.53, 0.81, 0.98) }
        );
    }
    // Sky texture swap
    if let Ok(handle) = sky_q.single() {
        if let Some(mat) = materials.get_mut(&handle.0) {
            // Only recreate if needed (check current is not already correct type is hard, so just recreate)
            let tex = images.add(create_sky_texture(is_night));
            mat.base_color_texture = Some(tex);
        }
    }
}

fn toggle_day_night(
    keys: Res<ButtonInput<KeyCode>>,
    mut time: ResMut<StadiumTime>,
) {
    if keys.just_pressed(KeyCode::KeyN) {
        *time = match *time { StadiumTime::Day => StadiumTime::Night, StadiumTime::Night => StadiumTime::Day };
        info!("Stadium time: {:?}", *time);
    }
}

/// Enter the match state: create ActiveMatch + first scene.
fn enter_match(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    wd: Res<WorldData>,
    setup: Res<MatchSetup>,
) {
    info!("Entering match");
    let am = build_active_match(&setup, &wd);
    let scene =
        match_flow::spawn_match_scene(&mut commands, &asset_server, &mut meshes, &mut materials, &mut images, &wd, &am);
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
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
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
            &mut commands, &asset_server, &mut meshes, &mut materials, &mut images, &wd, am);
        commands.insert_resource(new_scene);
        commands.insert_resource(Phase(PhaseEnum::ReadyToBall { t: 0.0 }));
    }
}

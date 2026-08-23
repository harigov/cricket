// Roadmap features (LBW, stumping, replays, extra cameras…) are modelled
// but not yet wired into gameplay, so some items are intentionally unused.
#![allow(dead_code)]

mod core;
mod game;
mod input;
mod render;
mod state;
mod ui;

use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;
use bevy::render::view::screenshot::Screenshot;
use bevy::window::WindowResolution;
use game::match_flow::{self, MatchScene};
use game::*;
use render::camera_rig::CameraRig;
use render::sky::{create_sky_texture, sky_texture_for_time};
use render::{
    DayEnvironmentLight, FloodlightFixture, FloodlightMaterials, NightEnvironmentLight, SkyTextures,
};
use state::{AppState, MatchPaused, RebuildScene};
/// Gameplay systems only run while the match resources actually exist
/// (they are torn down slightly before the state flips on exit).
fn in_live_match() -> impl bevy::ecs::schedule::SystemCondition<()> + Clone {
    in_state(AppState::InMatch).and(resource_exists::<ActiveMatch>)
}

/// Ball physics, AI and timers freeze while the pause overlay is open.
fn gameplay_active(
    state: Res<State<AppState>>,
    am: Option<Res<ActiveMatch>>,
    paused: Res<MatchPaused>,
) -> bool {
    *state.get() == AppState::InMatch && am.is_some() && !paused.0
}

fn register_core_plugins(app: &mut App) {
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Willow Cricket".into(),
            resolution: WindowResolution::new(1920, 1080),
            ..default()
        }),
        ..default()
    }))
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
    ));
}

fn register_match_systems(app: &mut App) {
    app.add_systems(OnEnter(AppState::InMatch), enter_match)
        .add_systems(OnExit(AppState::InMatch), exit_match)
        .add_systems(
            Update,
            handle_rebuild_scene.run_if(in_state(AppState::InMatch)),
        )
        .add_systems(
            Update,
            (
                match_flow::sys_ball_physics,
                match_flow::sys_ball_trail,
                match_flow::sys_shot_input,
                match_flow::sys_contact_watch,
                match_flow::sys_pending_watch,
                match_flow::sys_runners,
            )
                .chain()
                .run_if(gameplay_active),
        )
        .add_systems(
            Update,
            (
                match_flow::record_ball_flight,
                match_flow::sys_match_intro,
                match_flow::sys_ready,
                match_flow::sys_aim,
                match_flow::sys_runup,
                match_flow::sys_result_pause,
                match_flow::sys_over_break,
                match_flow::sys_innings_break,
                match_flow::sys_camera_modes,
                match_flow::sys_stadium_qa_camera.after(match_flow::sys_camera_modes),
                match_flow::fielding_brain_reset,
                match_flow::clear_recent_on_innings_change,
            )
                .run_if(gameplay_active),
        )
        .add_systems(Update, game::fielding::chase_system.run_if(gameplay_active))
        .add_systems(
            Update,
            (
                render::camera_rig::camera_toggle_system,
                game::match_flow::wicket_shake_trigger,
            )
                .run_if(gameplay_active),
        )
        // Director sets rig.mode, QA may override for stadium captures, then apply transform.
        .add_systems(
            Update,
            render::camera_rig::update_camera
                .after(match_flow::sys_stadium_qa_camera)
                .run_if(in_live_match()),
        )
        // Tear down only after all gameplay + camera work for the frame (C1).
        .add_systems(
            Update,
            ui::menus::handle_match_exit
                .run_if(in_state(AppState::InMatch))
                .after(render::camera_rig::update_camera),
        );
}

fn main() {
    let mut app = App::new();
    register_core_plugins(&mut app);
    app.add_systems(Startup, setup_basics)
        .add_systems(Update, (update_stadium_time, toggle_day_night));
    register_match_systems(&mut app);
    app.add_systems(Update, debug_screenshot)
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AutotestMode {
    Quick,
    Tournament,
    Settings,
    Night,
    Stadium,
    StadiumNight,
    /// Team picker: exercises grid navigation (Right / Down) and captures it.
    Menus,
    /// Enters a match then opens the pause overlay with Esc.
    Pause,
    /// Captures the venue picker and every toss slide.
    Setup,
}

struct AutotestScript {
    presses: [(f32, input::Action); 12],
    milestones: &'static [f32],
    end_time: f32,
    switches_to_night: bool,
    swings: bool,
}

// The toss adds a flip, a result slide, a bat/bowl choice and a summary
// between the venue pick and the first ball; the flip and result slides
// auto-advance, the other two wait for Confirm.
const QUICK_MATCH_PRESSES: [(f32, input::Action); 12] = [
    (2.0, input::Action::Confirm),  // Quick Match -> team select
    (3.5, input::Action::Confirm),  // pick your team
    (5.0, input::Action::Confirm),  // pick opponent
    (6.5, input::Action::Confirm),  // overs
    (8.0, input::Action::Confirm),  // stadium (random)
    // The toss slides auto-advance on Time<Virtual>, whose delta Bevy clamps
    // to 0.25 s. An unfocused window updates about once a second, so the 3.5 s
    // of slides can take ~14 s of wall clock here. Spread the remaining
    // confirms wide enough to cover that rather than hitting exact instants.
    (22.0, input::Action::Confirm), // toss: elect to bat
    (25.0, input::Action::Confirm), // toss summary -> match starts
    (33.0, input::Action::Confirm), // spare, after intro walk-on
    (36.0, input::Action::Confirm), // spare
    (99.0, input::Action::Confirm),
    (99.5, input::Action::Confirm),
    (200.0, input::Action::Confirm),
];

impl AutotestMode {
    fn from_env() -> Option<Self> {
        match std::env::var("CRICKET_AUTOTEST")
            .unwrap_or_default()
            .as_str()
        {
            "1" => Some(Self::Quick),
            "tournament" => Some(Self::Tournament),
            "settings" => Some(Self::Settings),
            "night" => Some(Self::Night),
            "stadium" => Some(Self::Stadium),
            "stadium-night" => Some(Self::StadiumNight),
            "menus" => Some(Self::Menus),
            "pause" => Some(Self::Pause),
            "setup" => Some(Self::Setup),
            _ => None,
        }
    }

    fn script(self) -> AutotestScript {
        match self {
            Self::Tournament => AutotestScript {
                presses: [
                    (2.0, input::Action::Next),    // highlight Tournament
                    (3.5, input::Action::Confirm), // enter Tournament
                    (5.0, input::Action::Confirm), // pick your team -> bracket
                    (8.5, input::Action::Confirm), // play semifinal
                    (12.0, input::Action::Confirm),
                    (14.0, input::Action::Confirm),
                    (16.0, input::Action::Confirm),
                    (18.0, input::Action::Confirm),
                    (20.0, input::Action::Confirm),
                    (22.0, input::Action::Confirm),
                    (24.0, input::Action::Confirm),
                    (26.0, input::Action::Confirm),
                ],
                milestones: &[1.5, 5.0, 7.0, 14.0],
                end_time: 20.0,
                switches_to_night: false,
                swings: true,
            },
            Self::Settings => AutotestScript {
                presses: [
                    (2.0, input::Action::Next),    // highlight Tournament
                    (2.6, input::Action::Next),    // highlight Settings
                    (3.5, input::Action::Confirm), // enter Settings
                    (6.0, input::Action::Next),    // move to SFX row
                    (6.5, input::Action::Right),   // bump volume
                    (7.5, input::Action::Cancel),  // back to main
                    (99.0, input::Action::Confirm),
                    (99.5, input::Action::Confirm),
                    (100.0, input::Action::Confirm),
                    (100.5, input::Action::Confirm),
                    (101.0, input::Action::Confirm),
                    (101.5, input::Action::Confirm),
                ],
                milestones: &[1.5, 5.0, 6.8, 8.5],
                end_time: 10.0,
                switches_to_night: false,
                swings: true,
            },
            Self::Night => AutotestScript {
                presses: QUICK_MATCH_PRESSES,
                milestones: &[1.5, 26.5, 32.0, 45.0, 58.0],
                end_time: 62.0,
                switches_to_night: true,
                swings: true,
            },
            Self::Stadium => AutotestScript {
                presses: QUICK_MATCH_PRESSES,
                milestones: &[32.0],
                end_time: 36.0,
                switches_to_night: false,
                swings: false,
            },
            Self::StadiumNight => AutotestScript {
                presses: QUICK_MATCH_PRESSES,
                milestones: &[32.0],
                end_time: 36.0,
                switches_to_night: true,
                swings: false,
            },
            // Sits on the team grid and steps across a row then down a row, so
            // the capture shows whether the highlight tracks the arrow keys.
            Self::Menus => AutotestScript {
                presses: [
                    (2.0, input::Action::Confirm), // Quick Match -> team select
                    (3.0, input::Action::Right),   // across one column
                    (3.6, input::Action::Right),
                    (4.2, input::Action::Next), // down one row
                    (5.2, input::Action::Confirm), // lock team -> opponent grid
                    (6.2, input::Action::Right),
                    (6.8, input::Action::Next),
                    (99.0, input::Action::Confirm),
                    (99.5, input::Action::Confirm),
                    (100.0, input::Action::Confirm),
                    (100.5, input::Action::Confirm),
                    (101.0, input::Action::Confirm),
                ],
                milestones: &[3.4, 4.8, 5.8, 7.4],
                end_time: 9.0,
                switches_to_night: false,
                swings: false,
            },
            // Walks to the venue picker and through every toss slide, capturing
            // each one so the setup screens can be reviewed as images.
            Self::Setup => AutotestScript {
                presses: [
                    (2.0, input::Action::Confirm), // Quick Match -> team
                    (3.0, input::Action::Confirm), // team -> opponent
                    (4.0, input::Action::Confirm), // opponent -> overs
                    (5.0, input::Action::Confirm), // overs -> stadium
                    (8.0, input::Action::Confirm), // stadium -> toss flip
                    (14.0, input::Action::Confirm), // toss choice
                    (99.0, input::Action::Confirm),
                    (99.5, input::Action::Confirm),
                    (100.0, input::Action::Confirm),
                    (100.5, input::Action::Confirm),
                    (101.0, input::Action::Confirm),
                    (101.5, input::Action::Confirm),
                ],
                milestones: &[6.5, 9.5, 11.5, 13.0, 15.5],
                end_time: 17.0,
                switches_to_night: false,
                swings: false,
            },
            Self::Pause => AutotestScript {
                presses: [
                    (2.0, input::Action::Confirm),
                    (3.5, input::Action::Confirm),
                    (5.0, input::Action::Confirm),
                    (6.5, input::Action::Confirm),
                    (8.0, input::Action::Confirm),
                    (22.0, input::Action::Confirm), // toss: elect to bat
                    (25.0, input::Action::Confirm), // toss summary -> match
                    (28.0, input::Action::Confirm), // spare
                    (34.0, input::Action::Cancel),  // Esc: open pause overlay
                    (38.0, input::Action::Next),    // move down the pause list
                    (200.0, input::Action::Confirm),
                    (200.0, input::Action::Confirm),
                ],
                milestones: &[36.0, 40.0],
                end_time: 42.0,
                switches_to_night: false,
                swings: false,
            },
            Self::Quick => AutotestScript {
                presses: QUICK_MATCH_PRESSES,
                milestones: &[1.5, 26.5, 32.0, 45.0, 58.0],
                end_time: 62.0,
                switches_to_night: false,
                swings: true,
            },
        }
    }
}

fn autotest_drive(
    time: Res<Time<bevy::time::Real>>,
    mut input: ResMut<input::PlayerInput>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
    mut stadium_time: ResMut<StadiumTime>,
    mut t: Local<f32>,
    mut last_press: Local<u32>,
    mut last_milestone: Local<u32>,
    mut last_swing_t: Local<f32>,
    mut night_applied: Local<bool>,
) {
    let Some(mode) = AutotestMode::from_env() else {
        return;
    };
    let script = mode.script();
    *t += time.delta_secs();
    let now = *t;

    for (i, (when, action)) in script.presses.iter().enumerate() {
        let step = i as u32 + 1;
        if now >= *when && *last_press < step {
            input.just_pressed.push(*action);
            *last_press = step;
            info!("AUTOTEST: menu press #{step} ({action:?})");
        }
    }

    // Night / stadium-night: switch to floodlit mode once the match scene is live.
    if script.switches_to_night && now > 32.5 && !*night_applied {
        *stadium_time = StadiumTime::Night;
        *night_applied = true;
        info!("AUTOTEST: switched to night stadium lighting");
    }

    // In-match: swing periodically once play is under way (not stadium captures).
    // Starts after the toss slides and the opening walk-on so Confirm cannot
    // skip the intro before it is photographed.
    if script.swings && now > 33.0 && now - *last_swing_t >= 3.0 {
        *last_swing_t = now;
        input.just_pressed.push(input::Action::Confirm);
        info!("AUTOTEST: shot swing @ {:.1}s", now);
    }

    // Milestones: screenshots + clean exit.
    for (i, when) in script.milestones.iter().enumerate() {
        let step = 100 + i as u32;
        if now >= *when && *last_milestone < step {
            save_shot(&mut commands, format!("/tmp/opencode/auto-{i}.png"));
            *last_milestone = step;
        }
    }
    if now >= script.end_time && *last_milestone < 200 {
        *last_milestone = 200;
        exit.write(AppExit::Success);
    }
}

#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum StadiumTime {
    #[default]
    Day,
    Night,
}

#[derive(Component)]
struct SkySphere;

/// Day exposure (ev100). Lower = brighter outfield; keep headroom for sky.
const DAY_EV100: f32 = 10.2;
/// Night exposure — ~0.6 stop brighter than prior pass for TV floodlit readability.
const NIGHT_EV100: f32 = 8.8;

/// Aerial broadcast distances (~150–230 m); fog must not fully occlude the far oval.
/// Scaled with the multi-tier bowl, whose far stands now sit ~330 m from the
/// establishing camera.
const DAY_FOG_START: f32 = 300.0;
const DAY_FOG_END: f32 = 900.0;
const NIGHT_FOG_START: f32 = 190.0;
const NIGHT_FOG_END: f32 = 700.0;

/// Sky dome radius. Must stay comfortably larger than the furthest camera
/// distance from the origin: the establishing shot pulls back with the bowl,
/// and once the camera passes outside the dome the inside-out sphere fills the
/// frame and hides the whole ground.
const SKY_RADIUS: f32 = 600.0;

struct LightingPreset {
    ev100: f32,
    fog_color: Color,
    fog_start: f32,
    fog_end: f32,
    ambient_color: Color,
    ambient_brightness: f32,
    clear_color: Color,
}

fn lighting_preset(time: StadiumTime) -> LightingPreset {
    match time {
        StadiumTime::Day => LightingPreset {
            ev100: DAY_EV100,
            fog_color: Color::srgba(0.55, 0.70, 0.88, 1.0),
            fog_start: DAY_FOG_START,
            fog_end: DAY_FOG_END,
            ambient_color: Color::srgb(0.72, 0.78, 0.92),
            ambient_brightness: 520.0,
            clear_color: Color::srgb(0.50, 0.68, 0.90),
        },
        StadiumTime::Night => LightingPreset {
            ev100: NIGHT_EV100,
            fog_color: Color::srgba(0.04, 0.06, 0.12, 1.0),
            fog_start: NIGHT_FOG_START,
            fog_end: NIGHT_FOG_END,
            ambient_color: Color::srgb(0.36, 0.40, 0.54),
            ambient_brightness: 410.0,
            clear_color: Color::srgb(0.02, 0.03, 0.08),
        },
    }
}

fn distance_fog_falloff(time: StadiumTime) -> bevy::pbr::FogFalloff {
    use bevy::pbr::FogFalloff;
    let preset = lighting_preset(time);
    FogFalloff::Linear {
        start: preset.fog_start,
        end: preset.fog_end,
    }
}

fn setup_basics(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    stadium_time: Res<StadiumTime>,
) {
    use bevy::pbr::DistanceFog;

    // Sky textures generated once and cached — never per frame.
    let day_tex = images.add(create_sky_texture(false));
    let night_tex = images.add(create_sky_texture(true));
    commands.insert_resource(SkyTextures {
        day: day_tex.clone(),
        night: night_tex.clone(),
    });
    let sky_mat = materials.add(StandardMaterial {
        base_color_texture: Some(day_tex),
        unlit: true,
        cull_mode: None,
        double_sided: true,
        fog_enabled: false,
        ..default()
    });
    commands.spawn((
        SkySphere,
        Mesh3d(meshes.add(Sphere::new(SKY_RADIUS).mesh().uv(32, 32))),
        MeshMaterial3d(sky_mat),
        Transform::from_translation(Vec3::Y * -6.0),
        Visibility::default(),
        NotShadowCaster,
        NotShadowReceiver,
    ));

    // Day key sun (warm late-afternoon).
    commands.spawn((
        DayEnvironmentLight,
        DirectionalLight {
            illuminance: 54_000.0,
            shadows_enabled: true,
            color: Color::srgb(1.0, 0.94, 0.82),
            ..default()
        },
        Transform::from_translation(Vec3::new(-52.0, 82.0, 22.0)).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    // Cool skylight fill — readable shadows without flat wash.
    commands.spawn((
        DayEnvironmentLight,
        DirectionalLight {
            illuminance: 5_800.0,
            shadows_enabled: false,
            color: Color::srgb(0.58, 0.68, 0.92),
            ..default()
        },
        Transform::from_translation(Vec3::new(42.0, 48.0, -28.0)).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    // Night moon key (flood spots are built with the stadium).
    commands.spawn((
        NightEnvironmentLight,
        DirectionalLight {
            illuminance: 1_100.0,
            shadows_enabled: false,
            color: Color::srgb(0.62, 0.70, 0.92),
            ..default()
        },
        Transform::from_translation(Vec3::new(30.0, 55.0, -18.0)).looking_at(Vec3::ZERO, Vec3::Y),
        Visibility::Hidden,
    ));

    // Main 3D camera (UI renders onto the primary window camera).
    let preset = lighting_preset(*stadium_time);
    commands.spawn((
        Camera {
            clear_color: bevy::camera::ClearColorConfig::Custom(preset.clear_color),
            ..default()
        },
        Camera3d::default(),
        Msaa::Sample4,
        Transform::from_xyz(36.0, 13.8, 2.2).looking_at(Vec3::new(-0.6, 0.78, 0.0), Vec3::Y),
        IsDefaultUiCamera,
        bevy::core_pipeline::tonemapping::Tonemapping::TonyMcMapface,
        bevy::camera::Exposure {
            ev100: preset.ev100,
        },
        DistanceFog {
            color: preset.fog_color,
            falloff: distance_fog_falloff(*stadium_time),
            ..default()
        },
    ));
    commands.insert_resource(GlobalAmbientLight {
        color: preset.ambient_color,
        brightness: preset.ambient_brightness,
        affects_lightmapped_meshes: true,
    });
    commands.insert_resource(bevy::light::DirectionalLightShadowMap { size: 4096 });
    commands.insert_resource(StadiumTime::Day);
    let _ = meshes.add(Sphere::new(0.1));
    let _ = materials.add(Color::WHITE);
}

fn update_stadium_time(
    time: Res<StadiumTime>,
    sky_textures: Res<SkyTextures>,
    mut day_lights: Query<
        &mut Visibility,
        (With<DayEnvironmentLight>, Without<NightEnvironmentLight>),
    >,
    mut night_lights: Query<
        &mut Visibility,
        (With<NightEnvironmentLight>, Without<DayEnvironmentLight>),
    >,
    mut sky_q: Query<
        &mut MeshMaterial3d<StandardMaterial>,
        (With<SkySphere>, Without<FloodlightFixture>),
    >,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut fog_q: Query<&mut DistanceFog>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut cam_q: Query<&mut Camera>,
    mut exposure_q: Query<&mut bevy::camera::Exposure, With<Camera3d>>,
    mut fixtures: Query<
        &mut MeshMaterial3d<StandardMaterial>,
        (With<FloodlightFixture>, Without<SkySphere>),
    >,
    fixture_mats: Option<Res<FloodlightMaterials>>,
) {
    if !time.is_changed() {
        return;
    }
    let is_night = *time == StadiumTime::Night;
    let preset = lighting_preset(*time);
    if let Ok(mut exp) = exposure_q.single_mut() {
        exp.ev100 = preset.ev100;
    }
    for mut v in &mut day_lights {
        *v = if is_night {
            Visibility::Hidden
        } else {
            Visibility::Visible
        };
    }
    for mut v in &mut night_lights {
        *v = if is_night {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    if let Ok(mut fog) = fog_q.single_mut() {
        fog.color = preset.fog_color;
        fog.falloff = distance_fog_falloff(*time);
    }
    *ambient = GlobalAmbientLight {
        color: preset.ambient_color,
        brightness: preset.ambient_brightness,
        affects_lightmapped_meshes: true,
    };
    if let Ok(mut cam) = cam_q.single_mut() {
        cam.clear_color = bevy::camera::ClearColorConfig::Custom(preset.clear_color);
    }
    if let Ok(handle) = sky_q.single_mut()
        && let Some(mat) = materials.get_mut(&handle.0)
    {
        mat.base_color_texture =
            Some(sky_texture_for_time(is_night, &sky_textures.day, &sky_textures.night).clone());
    }
    if let Some(mats) = fixture_mats {
        let target = if is_night {
            mats.night.clone()
        } else {
            mats.day.clone()
        };
        for mut mat_handle in &mut fixtures {
            mat_handle.0 = target.clone();
        }
    }
}

fn toggle_day_night(keys: Res<ButtonInput<KeyCode>>, mut time: ResMut<StadiumTime>) {
    if keys.just_pressed(KeyCode::KeyN) {
        *time = match *time {
            StadiumTime::Day => StadiumTime::Night,
            StadiumTime::Night => StadiumTime::Day,
        };
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
    let scene = match_flow::spawn_match_scene(
        &mut commands,
        &asset_server,
        &mut meshes,
        &mut materials,
        &mut images,
        &wd,
        &am,
    );
    commands.insert_resource(am);
    commands.insert_resource(scene);
    commands.insert_resource(CurrentDelivery(None));
    commands.insert_resource(Phase(PhaseEnum::MatchIntro { t: 0.0 }));
    commands.insert_resource(MatchPaused(false));
}

/// Tear down the live scene when leaving the match.
fn exit_match(mut commands: Commands, scene: Option<Res<MatchScene>>) {
    if let Some(s) = scene.as_deref() {
        match_flow::despawn_match_scene(&mut commands, s);
    }
    commands.remove_resource::<ActiveMatch>();
    commands.remove_resource::<MatchPaused>();
    commands.remove_resource::<CurrentDelivery>();
    commands.remove_resource::<Pending>();
    commands.insert_resource(Phase(PhaseEnum::Idle));
}

/// Innings changes rebuild the fielding/batting sides.
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
            &mut commands,
            &asset_server,
            &mut meshes,
            &mut materials,
            &mut images,
            &wd,
            am,
        );
        commands.insert_resource(new_scene);
        commands.insert_resource(Phase(PhaseEnum::ReadyToBall { t: 0.0 }));
    }
}

#[cfg(test)]
mod fog_tests {
    use super::*;
    use bevy::pbr::FogFalloff;

    /// Regression: enlarging the stadium bowl pushed the establishing camera to
    /// ~231 m from the origin while the sky dome was 220 m, so the camera sat
    /// outside the sphere and the inside-out sky hid the entire ground.
    #[test]
    fn establishing_camera_stays_inside_the_sky_dome() {
        use render::camera_rig::broadcast_establishing_view;
        // Check across the range of stadium sizes the game ships.
        for boundary in [55.0_f32, 60.0, 65.0, 68.0, 75.0] {
            let (pos, _, _) = broadcast_establishing_view(boundary);
            let dist = pos.length();
            assert!(
                dist < SKY_RADIUS * 0.9,
                "establishing camera is {dist:.1} m from origin for boundary {boundary}, \
                 too close to the {SKY_RADIUS} m sky dome; the dome would occlude the ground"
            );
        }
    }

    #[test]
    fn distance_fog_falloff_matches_day_night_constants() {
        match distance_fog_falloff(StadiumTime::Day) {
            FogFalloff::Linear { start, end } => {
                assert_eq!(start, DAY_FOG_START);
                assert_eq!(end, DAY_FOG_END);
            }
            _ => panic!("expected linear day fog"),
        }
        match distance_fog_falloff(StadiumTime::Night) {
            FogFalloff::Linear { start, end } => {
                assert_eq!(start, NIGHT_FOG_START);
                assert_eq!(end, NIGHT_FOG_END);
            }
            _ => panic!("expected linear night fog"),
        }
    }
}

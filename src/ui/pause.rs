//! In-match pause overlay: resume, settings (audio / controls / camera), quit.

use crate::game::audio::{AudioSettings, CommentaryVoice};
use crate::game::*;
use crate::input::{Action, KeyBindings, PlayerInput, RebindState, key_label};
use crate::render::camera_rig::{CamMode, CameraRig};
use crate::state::{AppState, MatchPaused};
use crate::ui::theme::{self, UiFonts, UiPreferences, UiScale};
use bevy::prelude::*;

#[derive(Component)]
struct PauseRoot;
#[derive(Component)]
struct PauseList;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum PauseScreen {
    #[default]
    Root,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
enum SettingsTab {
    #[default]
    Audio,
    Controls,
    Camera,
}

impl SettingsTab {
    fn label(self) -> &'static str {
        match self {
            SettingsTab::Audio => "Audio",
            SettingsTab::Controls => "Controls",
            SettingsTab::Camera => "Camera",
        }
    }

    fn next(self) -> Self {
        match self {
            SettingsTab::Audio => SettingsTab::Controls,
            SettingsTab::Controls => SettingsTab::Camera,
            SettingsTab::Camera => SettingsTab::Audio,
        }
    }

    fn prev(self) -> Self {
        match self {
            SettingsTab::Audio => SettingsTab::Camera,
            SettingsTab::Controls => SettingsTab::Audio,
            SettingsTab::Camera => SettingsTab::Controls,
        }
    }
}

#[derive(Resource, Default)]
struct PauseMenuState {
    screen: PauseScreen,
    sel: usize,
    settings_tab: SettingsTab,
}

const SETTINGS_ACTIONS: &[(Action, &str)] = &[
    (Action::Confirm, "Confirm / Shot"),
    (Action::Cancel, "Cancel / Back"),
    (Action::Loft, "Loft"),
    (Action::Sprint, "Sprint"),
    (Action::Next, "Menu Down"),
    (Action::Prev, "Menu Up"),
    (Action::Left, "Aim Left"),
    (Action::Right, "Aim Right"),
    (Action::CycleType, "Cycle Delivery"),
    (Action::CycleCam, "Cycle Camera"),
];

const PLAYABLE_CAMERAS: [CamMode; 4] = [
    CamMode::BattingEnd,
    CamMode::BowlingEnd,
    CamMode::Broadcast,
    CamMode::FollowBall,
];

fn cam_mode_label(mode: CamMode) -> &'static str {
    match mode {
        CamMode::BattingEnd => "Behind batsman",
        CamMode::BowlingEnd => "Behind bowler",
        CamMode::Broadcast => "Broadcast wide",
        CamMode::FollowBall => "Follow ball",
        _ => "Custom",
    }
}

pub struct PausePlugin;

impl Plugin for PausePlugin {
    fn build(&self, app: &mut App) {
        theme::register_ui_font_assets(app);
        app.init_resource::<PauseMenuState>()
            .add_systems(OnEnter(AppState::InMatch), reset_pause_menu)
            .add_systems(OnExit(AppState::InMatch), (despawn_pause_ui, reset_pause_flag))
            .add_systems(
                Update,
                (
                    toggle_pause,
                    spawn_pause_ui,
                    refresh_pause_ui,
                    handle_pause_input,
                )
                    .run_if(in_state(AppState::InMatch).and(resource_exists::<ActiveMatch>)),
            );
    }
}

fn reset_pause_menu(mut menu: ResMut<PauseMenuState>) {
    *menu = PauseMenuState::default();
}

fn reset_pause_flag(paused: Option<ResMut<MatchPaused>>) {
    if let Some(mut paused) = paused {
        paused.0 = false;
    }
}

fn toggle_pause(
    input: Res<PlayerInput>,
    menu: Res<PauseMenuState>,
    mut paused: ResMut<MatchPaused>,
    phase: Res<Phase>,
) {
    if !input.pressed(Action::Cancel) {
        return;
    }
    if matches!(phase.0, PhaseEnum::Idle) {
        return;
    }
    if paused.0 {
        // Esc only resumes from the root page; sub-pages handle their own back.
        if menu.screen == PauseScreen::Root {
            paused.0 = false;
        }
    } else {
        paused.0 = true;
    }
}

fn spawn_pause_ui(
    mut commands: Commands,
    paused: Res<MatchPaused>,
    assets: Res<AssetServer>,
    existing: Query<(), With<PauseRoot>>,
) {
    if !paused.0 || !existing.is_empty() {
        return;
    }
    let fonts = UiFonts::load(&assets);
    commands
        .spawn((
            PauseRoot,
            fonts.clone(),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.04, 0.06, 0.72)),
            ZIndex(200),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(px(28.0)),
                    row_gap: px(10.0),
                    min_width: px(520.0),
                    border: UiRect::all(px(2.0)),
                    ..default()
                },
                BackgroundColor(theme::palette::panel_bg()),
                BorderColor::all(theme::palette::panel_border()),
            ))
            .with_children(|panel| {
                panel.spawn((
                    Text::new("PAUSED"),
                    TextFont {
                        font: fonts.display.clone(),
                        font_size: 34.0,
                        ..default()
                    },
                    TextColor(theme::palette::gold()),
                ));
                panel.spawn((
                    PauseList,
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(6.0),
                        margin: UiRect::vertical(px(12.0)),
                        ..default()
                    },
                ));
            });
        });
}

fn despawn_pause_ui(mut commands: Commands, roots: Query<Entity, With<PauseRoot>>) {
    for e in &roots {
        commands.entity(e).despawn();
    }
}

fn settings_tab_strip(active: SettingsTab) -> String {
    const TABS: [SettingsTab; 3] = [
        SettingsTab::Audio,
        SettingsTab::Controls,
        SettingsTab::Camera,
    ];
    let parts = TABS
        .iter()
        .map(|tab| {
            let label = tab.label();
            if *tab == active {
                format!("[ {label} ]")
            } else {
                format!("  {label}  ")
            }
        })
        .collect::<Vec<_>>();
    format!("{}   (←/→ or Q/C switch tab)", parts.join(""))
}

fn pause_lines(
    menu: &PauseMenuState,
    bindings: &KeyBindings,
    audio: &AudioSettings,
    rebind: &RebindState,
    ui: &UiPreferences,
    rig: &CameraRig,
) -> Vec<String> {
    match menu.screen {
        PauseScreen::Root => vec![
            "Resume".into(),
            "Settings".into(),
            "Controls".into(),
            "Quit to Main Menu".into(),
        ],
        PauseScreen::Settings => {
            let mut out = vec![settings_tab_strip(menu.settings_tab)];
            match menu.settings_tab {
                SettingsTab::Audio => {
                    out.push(format!(
                        "Master Volume : {:>3}%  (←/→ adjust)",
                        (audio.master * 100.0) as i32
                    ));
                    out.push(format!(
                        "SFX Volume    : {:>3}%  (←/→ adjust)",
                        (audio.sfx * 100.0) as i32
                    ));
                    out.push(format!(
                        "Music Volume  : {:>3}%  (←/→ adjust)",
                        (audio.music * 100.0) as i32
                    ));
                    out.push(format!(
                        "Commentary Vol: {:>3}%  (←/→ adjust)",
                        (audio.commentary_volume * 100.0) as i32
                    ));
                    let comm_label = match audio.commentary {
                        CommentaryVoice::Off => "Off",
                        CommentaryVoice::Male => "Ryan (M lead)",
                        CommentaryVoice::Female => "Natasha (F lead)",
                    };
                    out.push(format!("Commentary Voice : {comm_label:16} (←/→ cycle)"));
                }
                SettingsTab::Controls => {
                    out.push("SPACE  rebind selected     ←/→ or Q/C  switch tab".into());
                    for (action, label) in SETTINGS_ACTIONS {
                        let key_str = if rebind.0 == Some(*action) {
                            "Press any key...".to_string()
                        } else {
                            bindings
                                .map
                                .get(action)
                                .map(|k| key_label(*k))
                                .unwrap_or_else(|| "-".into())
                        };
                        out.push(format!("{label:16} : {key_str}"));
                    }
                    out.push("Reset controls to defaults".into());
                }
                SettingsTab::Camera => {
                    out.push(format!(
                        "Play camera   : {:16} (←/→ cycle)",
                        cam_mode_label(rig.mode)
                    ));
                    out.push(format!(
                        "UI Scale      : {:>4.0}%  (←/→ adjust)",
                        ui.ui_scale * 100.0
                    ));
                    out.push(format!(
                        "High Contrast : {}",
                        if ui.high_contrast { "On" } else { "Off" }
                    ));
                }
            }
            out.push("Back".into());
            out
        }
    }
}

fn settings_item_count(tab: SettingsTab) -> usize {
    match tab {
        SettingsTab::Audio => 7,
        SettingsTab::Controls => SETTINGS_ACTIONS.len() + 4,
        SettingsTab::Camera => 5,
    }
}

fn clear_pause_rows(commands: &mut Commands, root: Entity, children_q: &Query<&Children>) {
    if let Ok(children) = children_q.get(root) {
        for c in children.iter() {
            commands.entity(c).despawn();
        }
    }
}

fn refresh_pause_ui(
    paused: Res<MatchPaused>,
    menu: Res<PauseMenuState>,
    bindings: Res<KeyBindings>,
    audio: Res<AudioSettings>,
    rebind: Res<RebindState>,
    ui_prefs: Res<UiPreferences>,
    rig: Res<CameraRig>,
    ui_scale: Res<UiScale>,
    mut list_q: Query<Entity, With<PauseList>>,
    fonts_q: Query<&UiFonts, With<PauseRoot>>,
    children_q: Query<&Children>,
    mut commands: Commands,
) {
    if !paused.0 {
        return;
    }
    let Ok(list) = list_q.single_mut() else {
        return;
    };
    let Ok(fonts) = fonts_q.single() else {
        return;
    };
    clear_pause_rows(&mut commands, list, &children_q);
    let scale = ui_scale.0;
    let lines = pause_lines(&menu, &bindings, &audio, &rebind, &ui_prefs, &rig);
    commands.entity(list).with_children(|parent| {
        for (i, line) in lines.iter().enumerate() {
            let selected = i == menu.sel;
            parent
                .spawn((
                    Node {
                        padding: UiRect::axes(theme::spx(10.0, scale), theme::spx(5.0, scale)),
                        border: if selected {
                            UiRect::all(px(1.0))
                        } else {
                            UiRect::default()
                        },
                        ..default()
                    },
                    BackgroundColor(if selected {
                        theme::palette::selection_bg()
                    } else {
                        Color::NONE
                    }),
                    BorderColor::all(if selected {
                        theme::palette::selection_border()
                    } else {
                        Color::NONE
                    }),
                ))
                .with_children(|row| {
                    row.spawn((
                        Text::new(line.as_str()),
                        TextFont {
                            font: fonts.regular.clone(),
                            font_size: 18.0 * scale,
                            ..default()
                        },
                        TextColor(if selected {
                            theme::palette::text_primary()
                        } else {
                            theme::palette::text_muted()
                        }),
                    ));
                });
        }
    });
}

fn wrap_selection(sel: &mut usize, delta: i32, count: usize) {
    if count == 0 {
        return;
    }
    let n = count as i32;
    *sel = ((*sel as i32 + delta).rem_euclid(n)) as usize;
}

fn navigate_list(input: &PlayerInput, sel: &mut usize, count: usize) {
    if input.pressed(Action::Next) {
        wrap_selection(sel, 1, count);
    }
    if input.pressed(Action::Prev) {
        wrap_selection(sel, -1, count);
    }
}

fn switch_settings_tab(menu: &mut PauseMenuState, dir: i32) {
    menu.settings_tab = if dir > 0 {
        menu.settings_tab.next()
    } else {
        menu.settings_tab.prev()
    };
    menu.sel = 0;
}

fn settings_tab_switch_row(menu: &PauseMenuState) -> bool {
    menu.sel == 0 || (menu.settings_tab == SettingsTab::Controls && menu.sel == 1)
}

fn cycle_playable_camera(rig: &mut CameraRig, dir: i32) {
    let cur = PLAYABLE_CAMERAS
        .iter()
        .position(|m| *m == rig.mode)
        .unwrap_or(0);
    let n = PLAYABLE_CAMERAS.len() as i32;
    let next = (cur as i32 + dir).rem_euclid(n) as usize;
    rig.mode = PLAYABLE_CAMERAS[next];
}

fn handle_settings_input(
    menu: &mut PauseMenuState,
    input: &PlayerInput,
    keys: &ButtonInput<KeyCode>,
    bindings: &mut KeyBindings,
    rebind: &mut RebindState,
    audio: &mut AudioSettings,
    ui_prefs: &mut UiPreferences,
    rig: &mut CameraRig,
) -> bool {
    if let Some(action) = rebind.0 {
        if input.pressed(Action::Cancel) {
            rebind.0 = None;
        } else if let Some(&k) = keys.get_just_pressed().next()
            && !matches!(
                k,
                KeyCode::ShiftLeft
                    | KeyCode::ShiftRight
                    | KeyCode::ControlLeft
                    | KeyCode::ControlRight
                    | KeyCode::AltLeft
                    | KeyCode::AltRight
            )
        {
            bindings.map.insert(action, k);
            bindings.save();
            rebind.0 = None;
        }
        return true;
    }

    if input.pressed(Action::CycleType) {
        switch_settings_tab(menu, 1);
    }
    if input.pressed(Action::CycleCam) {
        switch_settings_tab(menu, -1);
    }

    if settings_tab_switch_row(menu) {
        if input.pressed(Action::Right) {
            switch_settings_tab(menu, 1);
        } else if input.pressed(Action::Left) {
            switch_settings_tab(menu, -1);
        }
    }

    let count = settings_item_count(menu.settings_tab);
    navigate_list(input, &mut menu.sel, count);
    if menu.sel >= count {
        menu.sel = count.saturating_sub(1);
    }

    let delta = if settings_tab_switch_row(menu) {
        0.0
    } else if input.pressed(Action::Right) {
        0.05
    } else if input.pressed(Action::Left) {
        -0.05
    } else {
        0.0
    };

    match menu.settings_tab {
        SettingsTab::Audio => {
            if (1..=4).contains(&menu.sel) && delta != 0.0 {
                match menu.sel {
                    1 => audio.master = (audio.master + delta).clamp(0.0, 1.0),
                    2 => audio.sfx = (audio.sfx + delta).clamp(0.0, 1.0),
                    3 => audio.music = (audio.music + delta).clamp(0.0, 1.0),
                    4 => audio.commentary_volume = (audio.commentary_volume + delta).clamp(0.0, 1.0),
                    _ => {}
                }
            } else if menu.sel == 5 && (input.pressed(Action::Right) || input.pressed(Action::Left))
            {
                let dir = if input.pressed(Action::Right) { 1 } else { -1 };
                let cur: i32 = match audio.commentary {
                    CommentaryVoice::Off => 0,
                    CommentaryVoice::Male => 1,
                    CommentaryVoice::Female => 2,
                };
                let next = (cur + dir).rem_euclid(3) as usize;
                audio.commentary = match next {
                    0 => CommentaryVoice::Off,
                    1 => CommentaryVoice::Male,
                    _ => CommentaryVoice::Female,
                };
            }
        }
        SettingsTab::Controls => {
            let action_base = 2;
            if menu.sel >= action_base
                && menu.sel < action_base + SETTINGS_ACTIONS.len()
                && input.pressed(Action::Confirm)
            {
                rebind.0 = Some(SETTINGS_ACTIONS[menu.sel - action_base].0);
            } else if menu.sel == action_base + SETTINGS_ACTIONS.len()
                && input.pressed(Action::Confirm)
            {
                *bindings = KeyBindings::default();
                bindings.save();
            }
        }
        SettingsTab::Camera => {
            if menu.sel == 1 && (input.pressed(Action::Right) || input.pressed(Action::Left)) {
                let dir = if input.pressed(Action::Right) { 1 } else { -1 };
                cycle_playable_camera(rig, dir);
            } else if menu.sel == 2 && delta != 0.0 {
                ui_prefs.ui_scale = (ui_prefs.ui_scale + delta).clamp(0.75, 1.5);
                ui_prefs.save();
            } else if menu.sel == 3 && input.pressed(Action::Confirm) {
                ui_prefs.high_contrast = !ui_prefs.high_contrast;
                ui_prefs.save();
            }
        }
    }

    if input.pressed(Action::Cancel) || (menu.sel == count - 1 && input.pressed(Action::Confirm)) {
        menu.screen = PauseScreen::Root;
        menu.sel = 0;
        return true;
    }

    input.pressed(Action::Confirm)
}

fn handle_pause_input(
    mut paused: ResMut<MatchPaused>,
    mut menu: ResMut<PauseMenuState>,
    input: Res<PlayerInput>,
    keys: Res<ButtonInput<KeyCode>>,
    mut bindings: ResMut<KeyBindings>,
    mut rebind: ResMut<RebindState>,
    mut audio: ResMut<AudioSettings>,
    mut ui_prefs: ResMut<UiPreferences>,
    mut rig: ResMut<CameraRig>,
    mut menu_state: ResMut<crate::ui::menus::MenuState>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    if !paused.0 {
        return;
    }

    match menu.screen {
        PauseScreen::Root => {
            navigate_list(&input, &mut menu.sel, 4);
            if input.pressed(Action::Confirm) {
                match menu.sel {
                    0 => paused.0 = false,
                    1 => {
                        menu.screen = PauseScreen::Settings;
                        menu.sel = 0;
                        menu.settings_tab = SettingsTab::Audio;
                    }
                    2 => {
                        menu.screen = PauseScreen::Settings;
                        menu.sel = 0;
                        menu.settings_tab = SettingsTab::Controls;
                    }
                    _ => {
                        paused.0 = false;
                        // The wizard is left on whatever setup screen started
                        // this match; "Quit to Main Menu" must actually land
                        // on the main menu, not back in the toss.
                        crate::ui::menus::back_to_main(&mut menu_state);
                        next_state.set(AppState::Menu);
                    }
                }
            }
        }
        PauseScreen::Settings => {
            let _ = handle_settings_input(
                &mut menu,
                &input,
                &keys,
                &mut bindings,
                &mut rebind,
                &mut audio,
                &mut ui_prefs,
                &mut rig,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exit_match;
    use crate::game::{MatchSetup, WorldData, build_active_match};
    use crate::gameplay_active;
    use bevy::state::app::StatesPlugin;

    fn sample_match_setup() -> (WorldData, MatchSetup) {
        let wd = WorldData::new();
        let setup = MatchSetup {
            teams: [0, 1],
            stadium: 0,
            overs: 20,
            user_bats_first: true,
            from_tournament: false,
        };
        (wd, setup)
    }

    fn sample_pause_lines(tab: SettingsTab) -> Vec<String> {
        let menu = PauseMenuState {
            screen: PauseScreen::Settings,
            sel: 0,
            settings_tab: tab,
        };
        pause_lines(
            &menu,
            &KeyBindings::default(),
            &AudioSettings::default(),
            &RebindState::default(),
            &UiPreferences::default(),
            &CameraRig::default(),
        )
    }

    #[test]
    fn settings_item_count_matches_pause_lines_rows() {
        for tab in [SettingsTab::Audio, SettingsTab::Controls, SettingsTab::Camera] {
            let lines = sample_pause_lines(tab);
            assert_eq!(lines.len(), settings_item_count(tab), "tab {tab:?}");
        }
    }

    #[test]
    fn settings_tab_strip_marks_active_tab() {
        let strip = settings_tab_strip(SettingsTab::Controls);
        assert!(strip.contains("[ Controls ]"));
        assert!(!strip.contains("[ Audio ]"));
    }

    #[test]
    fn settings_left_right_switch_tab_on_tab_strip_row() {
        let mut menu = PauseMenuState {
            screen: PauseScreen::Settings,
            sel: 0,
            settings_tab: SettingsTab::Audio,
        };
        let input = PlayerInput {
            just_pressed: vec![Action::Right],
            ..Default::default()
        };
        let mut bindings = KeyBindings::default();
        let mut rebind = RebindState::default();
        let mut audio = AudioSettings::default();
        let mut ui_prefs = UiPreferences::default();
        let mut rig = CameraRig::default();
        let keys = ButtonInput::<KeyCode>::default();

        handle_settings_input(
            &mut menu,
            &input,
            &keys,
            &mut bindings,
            &mut rebind,
            &mut audio,
            &mut ui_prefs,
            &mut rig,
        );

        assert_eq!(menu.settings_tab, SettingsTab::Controls);
        assert_eq!(menu.sel, 0);
    }

    #[test]
    fn gameplay_active_tolerates_missing_match_paused() {
        fn noop_system() {}

        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin));
        app.init_state::<AppState>();

        let (wd, setup) = sample_match_setup();
        app.insert_resource(build_active_match(&setup, &wd));
        app.add_systems(Update, noop_system.run_if(gameplay_active));

        app.world_mut()
            .resource_mut::<NextState<AppState>>()
            .set(AppState::InMatch);
        app.update();
    }

    #[test]
    fn exit_match_preserves_match_paused_resource() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin));
        app.init_state::<AppState>();
        app.init_resource::<MatchPaused>();
        app.add_systems(OnExit(AppState::InMatch), exit_match);

        let (wd, setup) = sample_match_setup();
        app.insert_resource(build_active_match(&setup, &wd));
        app.insert_resource(Phase(PhaseEnum::Idle));

        app.world_mut()
            .resource_mut::<NextState<AppState>>()
            .set(AppState::InMatch);
        app.update();
        assert!(app.world().contains_resource::<MatchPaused>());

        app.world_mut()
            .resource_mut::<NextState<AppState>>()
            .set(AppState::Menu);
        app.update();

        assert!(app.world().contains_resource::<MatchPaused>());
        assert!(!app.world().contains_resource::<ActiveMatch>());
        assert!(!app.world().get_resource::<MatchPaused>().unwrap().0);
    }
}

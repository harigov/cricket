//! Menus: main menu, match-setup wizard, controls help and the tournament
//! bracket screen. Keyboard/gamepad driven (see input::Action mapping).

use crate::core::tournament::{Entrant, Fixture, Stage, Tournament};
use crate::game::audio::AudioSettings;
use crate::game::*;
use crate::input::{key_label, Action, KeyBindings, PlayerInput, RebindState};
use crate::state::AppState;
use crate::ui::theme::{self, MenuTransition, UiPreferences, UiScale};
use bevy::prelude::*;

const OVERS_CHOICES: [u32; 3] = [5, 10, 20];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsTab {
    Audio,
    Controls,
    Display,
}

impl SettingsTab {
    fn label(self) -> &'static str {
        match self {
            SettingsTab::Audio => "AUDIO",
            SettingsTab::Controls => "CONTROLS",
            SettingsTab::Display => "DISPLAY",
        }
    }

    fn next(self) -> Self {
        match self {
            SettingsTab::Audio => SettingsTab::Controls,
            SettingsTab::Controls => SettingsTab::Display,
            SettingsTab::Display => SettingsTab::Audio,
        }
    }

    fn prev(self) -> Self {
        match self {
            SettingsTab::Audio => SettingsTab::Display,
            SettingsTab::Controls => SettingsTab::Audio,
            SettingsTab::Display => SettingsTab::Controls,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Main,
    SetupTeam,
    SetupOpp,
    SetupOvers,
    SetupStadium,
    SetupBatFirst,
    Settings,
    Bracket,
}

#[derive(Resource)]
pub struct MenuState {
    pub screen: Screen,
    pub sel: usize,
    pub team: usize,
    pub opp: usize,
    pub overs_idx: usize,
    /// Index into stadiums; usize::MAX means "random each match".
    pub stadium_idx: usize,
    pub bat_first: bool,
    /// Settings screen active tab.
    pub settings_tab: SettingsTab,
    /// Toss coin-flip animation timer (SetupBatFirst).
    pub toss_anim: f32,
    /// true when the current setup wizard leads into a tournament.
    pub tournament_mode: bool,
}

impl Default for MenuState {
    fn default() -> Self {
        MenuState {
            screen: Screen::Main,
            sel: 0,
            team: 0,
            opp: 1,
            overs_idx: 2,
            stadium_idx: usize::MAX,
            bat_first: true,
            settings_tab: SettingsTab::Audio,
            toss_anim: 0.0,
            tournament_mode: false,
        }
    }
}

/// The tournament currently in progress, if any.
#[derive(Resource, Default)]
pub struct CurrentTournament(pub Option<Tournament>);

/// Index of the tournament fixture currently being played.
#[derive(Resource, Default)]
pub struct ActiveFixture(pub Option<usize>);

#[derive(Component)]
struct MenuRoot;
#[derive(Component)]
struct MenuList;
#[derive(Component)]
struct MenuBackdrop {
    main: Handle<Image>,
    secondary: Handle<Image>,
}
#[derive(Component)]
struct MenuFonts {
    display: Handle<Font>,
    bold: Handle<Font>,
    regular: Handle<Font>,
}

pub struct MenusPlugin;

impl Plugin for MenusPlugin {
    fn build(&self, app: &mut App) {
        // Menu key art is embedded so `target/release/cricket` remains a
        // self-contained executable, matching the existing launch instructions.
        bevy::asset::embedded_asset!(app, "../../assets/ui/main-menu-hero.png");
        bevy::asset::embedded_asset!(app, "../../assets/ui/menu-stadium.png");
        bevy::asset::embedded_asset!(app, "../../assets/fonts/Lato-Black.ttf");
        bevy::asset::embedded_asset!(app, "../../assets/fonts/Lato-Bold.ttf");
        bevy::asset::embedded_asset!(app, "../../assets/fonts/Lato-Regular.ttf");
        app.init_resource::<MenuState>()
            .init_resource::<CurrentTournament>()
            .init_resource::<ActiveFixture>()
            .add_systems(OnEnter(AppState::Menu), spawn_menu_root)
            .add_systems(OnExit(AppState::Menu), despawn_menu_root)
            .add_systems(
                Update,
                (
                    sync_ui_scale,
                    tick_toss_anim,
                    refresh_menu,
                    handle_menu_input,
                    handle_match_exit,
                )
                    .run_if(in_state(AppState::Menu)),
            );
    }
}

fn sync_ui_scale(prefs: Res<UiPreferences>, mut scale: ResMut<UiScale>) {
    scale.0 = prefs.ui_scale.clamp(0.75, 1.5);
}

fn tick_toss_anim(time: Res<Time>, mut ms: ResMut<MenuState>) {
    if ms.screen == Screen::SetupBatFirst {
        ms.toss_anim += time.delta_secs();
    } else {
        ms.toss_anim = 0.0;
    }
}

fn trigger_screen_transition(trans: &mut MenuTransition, _screen: Screen) {
    trans.active = true;
    trans.t = 0.0;
}

// ---------------------------------------------------------------------------
// UI construction (immediate-mode style rebuild each frame)
// ---------------------------------------------------------------------------

fn spawn_menu_root(mut commands: Commands, assets: Res<AssetServer>) {
    info!("MENU ROOT SPAWNED");
    commands.spawn((
        MenuRoot,
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            ..default()
        },
        BackgroundColor(Color::srgb(0.015, 0.025, 0.022)),
    )).with_children(|p| {
        let main = bevy::asset::load_embedded_asset!(assets.as_ref(), "../../assets/ui/main-menu-hero.png");
        let secondary = bevy::asset::load_embedded_asset!(assets.as_ref(), "../../assets/ui/menu-stadium.png");
        let display = bevy::asset::load_embedded_asset!(assets.as_ref(), "../../assets/fonts/Lato-Black.ttf");
        let bold = bevy::asset::load_embedded_asset!(assets.as_ref(), "../../assets/fonts/Lato-Bold.ttf");
        let regular = bevy::asset::load_embedded_asset!(assets.as_ref(), "../../assets/fonts/Lato-Regular.ttf");
        p.spawn((
            MenuBackdrop {
                main: main.clone(),
                secondary,
            },
            ImageNode::new(main),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
        ));
        // A light global grade ties the two pieces of key art to the UI palette
        // and protects text contrast at unusual window aspect ratios.
        p.spawn((
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
            BackgroundColor(Color::srgba(0.01, 0.035, 0.025, 0.18)),
        ));
        p.spawn((
            MenuList,
            MenuFonts {
                display,
                bold,
                regular,
            },
            Node {
                position_type: PositionType::Absolute,
                flex_direction: FlexDirection::Column,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.015, 0.035, 0.028, 0.90)),
            BorderColor::all(Color::srgba(0.72, 0.82, 0.56, 0.28)),
        ));
    });
}

fn despawn_menu_root(mut commands: Commands, q: Query<Entity, With<MenuRoot>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

fn screen_title(ms: &MenuState) -> &'static str {
    match ms.screen {
        Screen::Main => "WILLOW CRICKET",
        Screen::SetupTeam => "SELECT YOUR TEAM",
        Screen::SetupOpp => "SELECT OPPONENT",
        Screen::SetupOvers => "MATCH LENGTH",
        Screen::SetupStadium => "SELECT STADIUM",
        Screen::SetupBatFirst => "TOSS",
        Screen::Settings => "SETTINGS",
        Screen::Bracket => "TOURNAMENT BRACKET",
    }
}

fn screen_kicker(ms: &MenuState) -> &'static str {
    match ms.screen {
        Screen::Main => "THE GENTLEMAN'S GAME. YOUR MOMENT.",
        Screen::SetupTeam => "MATCH SETUP  /  01",
        Screen::SetupOpp => "MATCH SETUP  /  02",
        Screen::SetupOvers => "MATCH SETUP  /  03",
        Screen::SetupStadium => "MATCH SETUP  /  04",
        Screen::SetupBatFirst => "MATCH SETUP  /  05",
        Screen::Settings => "AUDIO & CONTROLS",
        Screen::Bracket => "KNOCKOUT CHAMPIONSHIP",
    }
}

fn screen_items(
    ms: &MenuState,
    wd: &WorldData,
    ct: &CurrentTournament,
    bindings: &KeyBindings,
    audio: &AudioSettings,
    rebind: &RebindState,
    ui: &UiPreferences,
) -> Vec<String> {
    match ms.screen {
        Screen::Main => vec![
            "Quick Match".into(),
            "Tournament".into(),
            "Settings".into(),
            "Quit".into(),
        ],
        Screen::SetupTeam | Screen::SetupOpp => wd
            .teams
            .iter()
            .map(|t| format!("{} ({})", t.name, t.short))
            .collect(),
        Screen::SetupOvers => OVERS_CHOICES
            .iter()
            .map(|o| format!("{o} overs"))
            .collect(),
        Screen::SetupStadium => wd
            .stadiums
            .iter()
            .map(|s| format!("{} — {} [{}]", s.name, s.city, s.pitch.label()))
            .chain(std::iter::once("Random venue".into()))
            .collect(),
        Screen::SetupBatFirst => vec!["We will BAT first".into(), "We will BOWL first".into()],
        Screen::Settings => settings_lines(ms.settings_tab, bindings, audio, rebind, ui),
        Screen::Bracket => bracket_lines(ct),
    }
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

fn settings_lines(
    tab: SettingsTab,
    bindings: &KeyBindings,
    audio: &AudioSettings,
    rebind: &RebindState,
    ui: &UiPreferences,
) -> Vec<String> {
    let mut out = vec![format!("Tab: {}", tab.label())];
    match tab {
        SettingsTab::Audio => {
            out.push(format!("Master Volume : {:>3}%  (←/→ adjust)", (audio.master * 100.0) as i32));
            out.push(format!("SFX Volume    : {:>3}%  (←/→ adjust)", (audio.sfx * 100.0) as i32));
            out.push(format!("Music Volume  : {:>3}%  (←/→ adjust)", (audio.music * 100.0) as i32));
            out.push(format!(
                "Commentary Vol: {:>3}%  (←/→ adjust)",
                (audio.commentary_volume * 100.0) as i32
            ));
            let comm_label = match audio.commentary {
                crate::game::audio::CommentaryVoice::Off => "Off",
                crate::game::audio::CommentaryVoice::Male => "Ryan (M lead)",
                crate::game::audio::CommentaryVoice::Female => "Natasha (F lead)",
            };
            out.push(format!("Commentary Voice : {comm_label:16} (←/→ cycle)"));
        }
        SettingsTab::Controls => {
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
        SettingsTab::Display => {
            out.push(format!(
                "UI Scale      : {:>4.0}%  (←/→ adjust)",
                ui.ui_scale * 100.0
            ));
            out.push(format!(
                "High Contrast : {}",
                if ui.high_contrast { "On" } else { "Off" }
            ));
            out.push(format!(
                "Subtitle Size : {:>4.0}%  (←/→ adjust)",
                ui.subtitle_scale * 100.0
            ));
        }
    }
    out.push("Back".into());
    out
}

fn settings_item_count(tab: SettingsTab) -> usize {
    match tab {
        SettingsTab::Audio => 6,    // header + 5 + back
        SettingsTab::Controls => 12, // header + 10 + reset + back
        SettingsTab::Display => 5,  // header + 3 + back
    }
}

fn bracket_lines(ct: &CurrentTournament) -> Vec<String> {
    let Some(t) = &ct.0 else {
        return vec!["No tournament in progress.".into()];
    };
    let mut out = vec![t.name.clone(), String::new()];
    for f in &t.fixtures {
        let home = &t.teams[f.home];
        let away = &t.teams[f.away];
        let status = match f.result.as_ref() {
            Some(crate::core::rules::Result::Win { margin, .. }) => {
                let w = t
                    .fixture_winner(f)
                    .map(|i| t.teams[i].short.clone())
                    .unwrap_or_default();
                format!("{w} {margin}")
            }
            Some(crate::core::rules::Result::Tie) => "TIE".into(),
            None => "-".into(),
        };
        // Final round shows TBD placeholders until both semis resolve.
        if f.stage == Stage::Final && f.home == 0 && f.away == 1 && f.result.is_none() {
            out.push(format!("{}: TBD v TBD @ {}", f.stage.label(), t.stadiums[f.stadium].name));
            continue;
        }
        out.push(format!(
            "{}: {} v {}  |  {} @ {}",
            f.stage.label(),
            home.short,
            away.short,
            status,
            t.stadiums[f.stadium].name
        ));
    }
    out.push(String::new());
    match t.champion() {
        Some(c) => out.push(format!("CHAMPIONS: {}!", t.teams[c].name)),
        None if t.next_user_fixture().is_some() => {
            out.push("SPACE / A: play your next match".into())
        }
        None => out.push("Advancing the draw...".into()),
    }
    out
}

// ---------------------------------------------------------------------------
// Per-frame UI refresh
// ---------------------------------------------------------------------------

fn refresh_menu(
    ms: Res<MenuState>,
    wd: Res<WorldData>,
    mut ct: ResMut<CurrentTournament>,
    bindings: Res<KeyBindings>,
    audio: Res<AudioSettings>,
    rebind: Res<RebindState>,
    ui_prefs: Res<UiPreferences>,
    ui_scale: Res<UiScale>,
    trans: Res<MenuTransition>,
    assets: Res<AssetServer>,
    mut root_q: Query<(Entity, &mut Node, &MenuFonts), With<MenuList>>,
    mut backdrop_q: Query<(&mut ImageNode, &MenuBackdrop)>,
    children_q: Query<&Children>,
    mut commands: Commands,
) {
    let Ok((root, mut root_node, fonts)) = root_q.single_mut() else { return };
    let is_main = ms.screen == Screen::Main;
    let scale = ui_scale.0;
    let fade = if trans.active {
        (trans.t / 0.28).min(1.0)
    } else {
        0.0
    };

    if let Ok((mut image, art)) = backdrop_q.single_mut() {
        image.image = if is_main {
            art.main.clone()
        } else {
            art.secondary.clone()
        };
    }

    // The hero uses the deliberately quiet left half of its artwork. Dense
    // screens use a broad central panel over the pavilion vista.
    if is_main {
        root_node.left = percent(7);
        root_node.top = percent(17);
        root_node.width = px(470);
        root_node.max_height = percent(72);
        root_node.padding = UiRect::axes(px(34), px(30));
        root_node.align_items = AlignItems::Stretch;
        root_node.row_gap = px(9);
    } else {
        root_node.left = percent(16);
        root_node.top = percent(6);
        root_node.width = percent(68);
        root_node.max_height = percent(88);
        root_node.padding = UiRect::axes(px(30), px(20));
        root_node.align_items = AlignItems::Center;
        root_node.row_gap = px(6);
    }

    // Clear previous frame's rows.
    if let Ok(children) = children_q.get(root) {
        for c in children.iter() {
            commands.entity(c).despawn();
        }
    }

    // Advance the draw when nothing user-playable remains.
    if ms.screen == Screen::Bracket && let Some(t) = ct.0.as_mut() {
        if t.champion().is_none() && t.next_user_fixture().is_none() {
            for (idx, _f) in t.pending_fixtures() {
                t.sim_fixture(idx, 0xC0FFEE + idx as u64);
            }
        }
    }

    commands.entity(root).with_children(|p| {
        p.spawn((
            Text::new(screen_kicker(&ms)),
            TextFont {
                font: fonts.bold.clone(),
                font_size: if is_main { 12.0 } else { 11.0 },
                ..default()
            },
            TextColor(Color::srgb(0.88, 0.72, 0.29)),
        ));
        p.spawn((
            Text::new(if is_main {
                "WILLOW\nCRICKET"
            } else {
                screen_title(&ms)
            }),
            TextFont {
                font: fonts.display.clone(),
                font_size: if is_main { 54.0 } else { 38.0 },
                ..default()
            },
            TextColor(Color::srgb(0.97, 0.98, 0.94)),
            TextShadow {
                offset: Vec2::new(0.0, 3.0),
                color: Color::srgba(0.0, 0.0, 0.0, 0.72),
            },
        ));
        p.spawn((
            Node {
                width: if is_main { percent(45) } else { px(72) },
                height: px(3),
                margin: UiRect::vertical(px(8)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.78, 0.64, 0.25)),
        ));
        p.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                align_items: if is_main { AlignItems::Stretch } else { AlignItems::Center },
                row_gap: theme::spx(if ms.screen == Screen::Settings { 3.0 } else { 7.0 }, scale),
                width: if is_main { percent(100) } else { auto() },
                margin: UiRect::vertical(theme::spx(if is_main { 10.0 } else { 4.0 }, scale)),
                ..default()
            },
        )).with_children(|items| {
            // Settings tab bar
            if ms.screen == Screen::Settings {
                items.spawn((
                    Node {
                        flex_direction: FlexDirection::Row,
                        column_gap: theme::spx(8.0, scale),
                        margin: UiRect::bottom(theme::spx(6.0, scale)),
                        ..default()
                    },
                )).with_children(|tabs| {
                    for tab in [SettingsTab::Audio, SettingsTab::Controls, SettingsTab::Display] {
                        let active = tab == ms.settings_tab;
                        tabs.spawn((
                            Node {
                                padding: UiRect::axes(theme::spx(14.0, scale), theme::spx(5.0, scale)),
                                border_radius: BorderRadius::all(theme::spx(3.0, scale)),
                                ..default()
                            },
                            BackgroundColor(if active {
                                theme::palette::accent_blue()
                            } else {
                                Color::srgba(0.12, 0.14, 0.18, 0.85)
                            }),
                        )).with_children(|t| {
                            t.spawn((
                                Text::new(tab.label()),
                                TextFont {
                                    font: fonts.bold.clone(),
                                    font_size: 11.0 * scale,
                                    ..default()
                                },
                                TextColor(if active {
                                    Color::WHITE
                                } else {
                                    theme::palette::text_muted()
                                }),
                            ));
                        });
                    }
                });
            }

            // Visual setup: team cards, stadium cards, toss, versus banner
            if matches!(
                ms.screen,
                Screen::SetupTeam | Screen::SetupOpp | Screen::SetupStadium | Screen::SetupOvers
                    | Screen::SetupBatFirst
            ) {
                spawn_setup_visuals(
                    items,
                    &ms,
                    &wd,
                    &assets,
                    &fonts,
                    scale,
                    &ui_prefs,
                );
            }

            let lines = if ms.screen == Screen::Settings {
                settings_lines(ms.settings_tab, &bindings, &audio, &rebind, &ui_prefs)
            } else {
                screen_items(&ms, &wd, &ct, &bindings, &audio, &rebind, &ui_prefs)
            };

            for (i, line) in lines.into_iter().enumerate()
            {
                // Skip plain text rows on visual setup screens (cards replace them).
                if matches!(
                    ms.screen,
                    Screen::SetupTeam | Screen::SetupOpp | Screen::SetupStadium | Screen::SetupOvers
                ) && !line.starts_with("Tab:")
                {
                    continue;
                }
                if ms.screen == Screen::SetupBatFirst && i < 2 {
                    continue;
                }
                let selectable = !matches!(ms.screen, Screen::Bracket);
                let selected = i == ms.sel && selectable;
                let is_settings = ms.screen == Screen::Settings;
                items.spawn((
                    Node {
                        min_width: if is_main {
                            percent(100)
                        } else if is_settings {
                            px(570)
                        } else {
                            px(400)
                        },
                        padding: UiRect::axes(px(18), px(if is_main { 10 } else { 4 })),
                        border: UiRect {
                            left: px(if selected { 4 } else { 0 }),
                            ..default()
                        },
                        justify_content: if is_main {
                            JustifyContent::FlexStart
                        } else {
                            JustifyContent::Center
                        },
                        ..default()
                    },
                    BackgroundColor(if selected {
                        Color::srgba(0.23, 0.43, 0.23, 0.82)
                    } else {
                        Color::NONE
                    }),
                    BorderColor::all(if selected {
                        Color::srgb(0.84, 0.70, 0.29)
                    } else {
                        Color::NONE
                    }),
                )).with_children(|row| {
                    let text_color = if selected {
                        Color::srgb(1.0, 0.95, 0.76)
                    } else {
                        Color::srgb(0.78, 0.82, 0.77)
                    };
                    let face = if selected {
                        fonts.bold.clone()
                    } else {
                        fonts.regular.clone()
                    };

                    if is_main {
                        row.spawn((
                            Text::new(format!("{:02}", i + 1)),
                            TextFont {
                                font: fonts.bold.clone(),
                                font_size: 11.0,
                                ..default()
                            },
                            TextColor(if selected {
                                Color::srgb(1.0, 0.81, 0.34)
                            } else {
                                Color::srgba(0.72, 0.75, 0.68, 0.58)
                            }),
                            Node { width: px(38), ..default() },
                        ));
                        row.spawn((
                            Text::new(line.to_uppercase()),
                            TextFont { font: face, font_size: 20.0, ..default() },
                            TextColor(text_color),
                            TextShadow {
                                offset: Vec2::new(0.0, 2.0),
                                color: Color::srgba(0.0, 0.0, 0.0, 0.55),
                            },
                        ));
                    } else if is_settings {
                        if let Some((label, value)) = line.split_once(" : ") {
                            row.spawn((
                                Text::new(label.trim()),
                                TextFont {
                                    font: fonts.regular.clone(),
                                    font_size: 16.0,
                                    ..default()
                                },
                                TextColor(text_color),
                                Node { width: px(235), ..default() },
                            ));
                            row.spawn((
                                Text::new(value.trim()),
                                TextFont {
                                    font: fonts.bold.clone(),
                                    font_size: 16.0,
                                    ..default()
                                },
                                TextColor(if selected {
                                    Color::srgb(1.0, 0.86, 0.46)
                                } else {
                                    Color::srgb(0.90, 0.92, 0.87)
                                }),
                                Node { width: px(270), ..default() },
                            ));
                        } else {
                            row.spawn((
                                Text::new(line),
                                TextFont { font: face, font_size: 16.0, ..default() },
                                TextColor(text_color),
                            ));
                        }
                    } else {
                        row.spawn((
                            Text::new(line),
                            TextFont {
                                font: face,
                                font_size: if ms.screen == Screen::Bracket { 18.0 } else { 19.0 },
                                ..default()
                            },
                            TextColor(text_color),
                            TextShadow {
                                offset: Vec2::new(0.0, 1.5),
                                color: Color::srgba(0.0, 0.0, 0.0, 0.48),
                            },
                        ));
                    }
                });
            }
        });
        if ms.screen != Screen::Bracket {
            let hint = if ms.screen == Screen::Settings {
                "Q / E  SWITCH TAB     W / S  NAVIGATE     SPACE  SELECT     ESC  BACK"
            } else {
                "W / S  NAVIGATE     SPACE  SELECT     ESC  BACK"
            };
            p.spawn((
                Text::new(hint),
                TextFont {
                    font: fonts.bold.clone(),
                    font_size: 10.0 * scale,
                    ..default()
                },
                TextColor(Color::srgba(0.72, 0.76, 0.70, 0.72)),
                Node { margin: UiRect::top(theme::spx(8.0, scale)), ..default() },
            ));
        }

        // Transition wipe overlay
        if fade > 0.0 {
            p.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(100),
                    height: percent(100),
                    left: px(0),
                    top: px(0),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.02, 0.05, 0.04, fade * 0.55)),
            ));
        }
    });
}

/// Rich setup presentation: team crest cards, stadium info, toss sequence.
fn spawn_setup_visuals(
    parent: &mut ChildSpawnerCommands,
    ms: &MenuState,
    wd: &WorldData,
    assets: &AssetServer,
    fonts: &MenuFonts,
    scale: f32,
    ui_prefs: &UiPreferences,
) {
    let hc = ui_prefs.high_contrast;
    match ms.screen {
        Screen::SetupTeam | Screen::SetupOpp => {
            parent.spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    width: theme::spx(620.0, scale),
                    column_gap: theme::spx(10.0, scale),
                    row_gap: theme::spx(10.0, scale),
                    justify_content: JustifyContent::Center,
                    margin: UiRect::bottom(theme::spx(8.0, scale)),
                    ..default()
                },
            )).with_children(|grid| {
                for (i, team) in wd.teams.iter().enumerate() {
                    if ms.screen == Screen::SetupOpp && i == ms.team {
                        continue;
                    }
                    let selected = i == ms.sel;
                    let crest = crate::render::load_team_crest(assets, &team.crest_asset());
                    grid.spawn((
                        Node {
                            width: theme::spx(140.0, scale),
                            height: theme::spx(118.0, scale),
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::Center,
                            padding: UiRect::all(theme::spx(8.0, scale)),
                            border: UiRect::all(px(if selected { 3 } else { 1 })),
                            border_radius: BorderRadius::all(theme::spx(6.0, scale)),
                            ..default()
                        },
                        BackgroundColor(if selected {
                            if hc {
                                Color::srgba(0.35, 0.35, 0.40, 0.95)
                            } else {
                                theme::palette::selection_bg()
                            }
                        } else {
                            Color::srgba(0.08, 0.10, 0.12, 0.88)
                        }),
                        BorderColor::all(if selected {
                            theme::palette::selection_border()
                        } else {
                            Color::srgba(0.45, 0.50, 0.55, 0.35)
                        }),
                    )).with_children(|card| {
                        card.spawn((
                            ImageNode::new(crest),
                            Node {
                                width: theme::spx(52.0, scale),
                                height: theme::spx(52.0, scale),
                                ..default()
                            },
                        ));
                        card.spawn((
                            Text::new(team.short.to_uppercase()),
                            TextFont {
                                font: fonts.display.clone(),
                                font_size: 18.0 * scale,
                                ..default()
                            },
                            TextColor(theme::palette::text_primary()),
                        ));
                        card.spawn((
                            Node {
                                width: theme::spx(90.0, scale),
                                height: theme::spx(6.0, scale),
                                margin: UiRect::top(theme::spx(4.0, scale)),
                                ..default()
                            },
                            BackgroundColor(team.primary_color),
                        ));
                        card.spawn((
                            Text::new(&team.name),
                            TextFont {
                                font: fonts.regular.clone(),
                                font_size: 10.0 * scale,
                                ..default()
                            },
                            TextColor(theme::palette::text_muted()),
                        ));
                    });
                }
            });
        }
        Screen::SetupOvers => {
            parent.spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: theme::spx(16.0, scale),
                    margin: UiRect::bottom(theme::spx(10.0, scale)),
                    ..default()
                },
            )).with_children(|row| {
                for (i, overs) in OVERS_CHOICES.iter().enumerate() {
                    let selected = i == ms.sel;
                    row.spawn((
                        Node {
                            width: theme::spx(120.0, scale),
                            height: theme::spx(90.0, scale),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            border: UiRect::all(px(if selected { 3 } else { 1 })),
                            border_radius: BorderRadius::all(theme::spx(6.0, scale)),
                            ..default()
                        },
                        BackgroundColor(if selected {
                            theme::palette::selection_bg()
                        } else {
                            Color::srgba(0.08, 0.10, 0.12, 0.88)
                        }),
                        BorderColor::all(if selected {
                            theme::palette::gold()
                        } else {
                            Color::srgba(0.45, 0.50, 0.55, 0.35)
                        }),
                    )).with_children(|card| {
                        card.spawn((
                            Text::new(format!("{overs}")),
                            TextFont {
                                font: fonts.display.clone(),
                                font_size: 36.0 * scale,
                                ..default()
                            },
                            TextColor(Color::WHITE),
                        ));
                        card.spawn((
                            Text::new("OVERS"),
                            TextFont {
                                font: fonts.bold.clone(),
                                font_size: 11.0 * scale,
                                ..default()
                            },
                            TextColor(theme::palette::text_muted()),
                        ));
                    });
                }
            });
        }
        Screen::SetupStadium => {
            parent.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: theme::spx(8.0, scale),
                    width: theme::spx(560.0, scale),
                    margin: UiRect::bottom(theme::spx(8.0, scale)),
                    ..default()
                },
            )).with_children(|list| {
                let count = wd.stadiums.len() + 1;
                for i in 0..count {
                    let selected = i == ms.sel;
                    let (title, detail) = if i >= wd.stadiums.len() {
                        ("Random Venue".into(), "Surprise pick each match".into())
                    } else {
                        let s = &wd.stadiums[i];
                        (
                            s.name.clone(),
                            format!("{}  •  {} pitch  •  {} boundary",
                                s.city, s.pitch.label(), format!("{:.0}m", s.boundary_radius())),
                        )
                    };
                    list.spawn((
                        Node {
                            width: percent(100),
                            padding: UiRect::all(theme::spx(10.0, scale)),
                            border: UiRect::left(px(if selected { 4 } else { 0 })),
                            align_items: AlignItems::FlexStart,
                            ..default()
                        },
                        BackgroundColor(if selected {
                            theme::palette::selection_bg()
                        } else {
                            Color::srgba(0.06, 0.08, 0.10, 0.82)
                        }),
                        BorderColor::all(if selected {
                            theme::palette::gold()
                        } else {
                            Color::NONE
                        }),
                    )).with_children(|row| {
                        row.spawn((
                            Text::new(title),
                            TextFont {
                                font: fonts.bold.clone(),
                                font_size: 15.0 * scale,
                                ..default()
                            },
                            TextColor(if selected {
                                Color::WHITE
                            } else {
                                theme::palette::text_primary()
                            }),
                        ));
                        row.spawn((
                            Text::new(detail),
                            TextFont {
                                font: fonts.regular.clone(),
                                font_size: 11.0 * scale,
                                ..default()
                            },
                            TextColor(theme::palette::text_muted()),
                        ));
                    });
                }
            });
        }
        Screen::SetupBatFirst => {
            let user = &wd.teams[ms.team];
            let opp = &wd.teams[ms.opp];
            let flip = (ms.toss_anim * 8.0).sin();
            parent.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: theme::spx(10.0, scale),
                    margin: UiRect::bottom(theme::spx(12.0, scale)),
                    ..default()
                },
            )).with_children(|toss| {
                toss.spawn((
                    Text::new("TOSS"),
                    TextFont {
                        font: fonts.display.clone(),
                        font_size: 28.0 * scale,
                        ..default()
                    },
                    TextColor(theme::palette::gold()),
                ));
                toss.spawn((
                    Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: theme::spx(24.0, scale),
                        ..default()
                    },
                )).with_children(|versus| {
                    for team in [user, opp] {
                        versus.spawn((
                            Node {
                                flex_direction: FlexDirection::Column,
                                align_items: AlignItems::Center,
                                ..default()
                            },
                        )).with_children(|side| {
                            side.spawn((
                                ImageNode::new(crate::render::load_team_crest(
                                    assets,
                                    &team.crest_asset(),
                                )),
                                Node {
                                    width: theme::spx(64.0, scale),
                                    height: theme::spx(64.0, scale),
                                    ..default()
                                },
                            ));
                            side.spawn((
                                Text::new(team.short.to_uppercase()),
                                TextFont {
                                    font: fonts.bold.clone(),
                                    font_size: 16.0 * scale,
                                    ..default()
                                },
                                TextColor(team.primary_color),
                            ));
                        });
                    }
                    versus.spawn((
                        Text::new("VS"),
                        TextFont {
                            font: fonts.display.clone(),
                            font_size: 22.0 * scale,
                            ..default()
                        },
                        TextColor(Color::srgba(0.85, 0.85, 0.85, 0.65)),
                    ));
                });
                let coin_side = if flip > 0.0 { "HEADS" } else { "TAILS" };
                toss.spawn((
                    Text::new(format!("Coin: {coin_side}")),
                    TextFont {
                        font: fonts.bold.clone(),
                        font_size: 13.0 * scale,
                        ..default()
                    },
                    TextColor(theme::palette::text_muted()),
                ));
            });
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Input handling / navigation
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn handle_menu_input(
    mut ms: ResMut<MenuState>,
    input: Res<PlayerInput>,
    keys: Res<ButtonInput<KeyCode>>,
    wd: Res<WorldData>,
    mut ct: ResMut<CurrentTournament>,
    mut af: ResMut<ActiveFixture>,
    mut bindings: ResMut<KeyBindings>,
    mut rebind: ResMut<RebindState>,
    mut audio: ResMut<AudioSettings>,
    mut ui_prefs: ResMut<UiPreferences>,
    mut trans: ResMut<MenuTransition>,
    mut next_state: ResMut<NextState<AppState>>,
    mut exit: MessageWriter<AppExit>,
    mut commands: Commands,
) {
    use Screen::*;
    let _count = screen_item_count(&ms, &wd);
    let wrap = |sel: &mut usize, delta: i32, max: usize| {
        *sel = ((*sel as i32 + delta).rem_euclid(max as i32)) as usize;
    };

    match ms.screen {
        Main => {
            if input.pressed(Action::Next) { wrap(&mut ms.sel, 1, 4); }
            if input.pressed(Action::Prev) { wrap(&mut ms.sel, -1, 4); }
            if input.pressed(Action::Confirm) {
                match ms.sel {
                    0 => {
                        ms.tournament_mode = false;
                        ms.screen = SetupTeam;
                        ms.sel = ms.team;
                    }
                    1 => {
                        ms.tournament_mode = true;
                        ms.screen = SetupTeam;
                        ms.sel = ms.team;
                    }
                    2 => { ms.screen = Settings; }
                    _ => {
                        exit.write(AppExit::Success);
                    }
                }
            }
        }
        Settings => {
            if let Some(action) = rebind.0 {
                if input.pressed(Action::Cancel) {
                    rebind.0 = None;
                } else if let Some(&k) = keys.get_just_pressed().next() {
                    if !matches!(
                        k,
                        KeyCode::ShiftLeft
                            | KeyCode::ShiftRight
                            | KeyCode::ControlLeft
                            | KeyCode::ControlRight
                            | KeyCode::AltLeft
                            | KeyCode::AltRight
                    ) {
                        bindings.map.insert(action, k);
                        bindings.save();
                        rebind.0 = None;
                    }
                }
                return;
            }

            if input.pressed(Action::CycleType) {
                ms.settings_tab = ms.settings_tab.next();
                ms.sel = 0;
            }
            if input.pressed(Action::CycleCam) {
                ms.settings_tab = ms.settings_tab.prev();
                ms.sel = 0;
            }

            let count = settings_item_count(ms.settings_tab);
            navigate_list(&input, &mut ms.sel, count);

            let delta = if input.pressed(Action::Right) {
                0.05
            } else if input.pressed(Action::Left) {
                -0.05
            } else {
                0.0
            };

            match ms.settings_tab {
                SettingsTab::Audio => {
                    if ms.sel >= 1 && ms.sel <= 4 && delta != 0.0 {
                        match ms.sel {
                            1 => audio.master = (audio.master + delta).clamp(0.0, 1.0),
                            2 => audio.sfx = (audio.sfx + delta).clamp(0.0, 1.0),
                            3 => audio.music = (audio.music + delta).clamp(0.0, 1.0),
                            4 => {
                                audio.commentary_volume =
                                    (audio.commentary_volume + delta).clamp(0.0, 1.0)
                            }
                            _ => {}
                        }
                    } else if ms.sel == 5
                        && (input.pressed(Action::Right) || input.pressed(Action::Left))
                    {
                        let dir = if input.pressed(Action::Right) { 1 } else { -1 };
                        let cur = match audio.commentary {
                            crate::game::audio::CommentaryVoice::Off => 0,
                            crate::game::audio::CommentaryVoice::Male => 1,
                            crate::game::audio::CommentaryVoice::Female => 2,
                        };
                        let next = (cur as i32 + dir).rem_euclid(3) as usize;
                        audio.commentary = match next {
                            0 => crate::game::audio::CommentaryVoice::Off,
                            1 => crate::game::audio::CommentaryVoice::Male,
                            _ => crate::game::audio::CommentaryVoice::Female,
                        };
                    }
                }
                SettingsTab::Controls => {
                    if ms.sel >= 1 && ms.sel <= 10 && input.pressed(Action::Confirm) {
                        if let Some((action, _)) = SETTINGS_ACTIONS.get(ms.sel - 1) {
                            rebind.0 = Some(*action);
                        }
                    } else if ms.sel == 11 && input.pressed(Action::Confirm) {
                        *bindings = KeyBindings::default();
                        bindings.save();
                    }
                }
                SettingsTab::Display => {
                    if ms.sel == 1 && delta != 0.0 {
                        ui_prefs.ui_scale = (ui_prefs.ui_scale + delta).clamp(0.75, 1.5);
                        ui_prefs.save();
                    } else if ms.sel == 2
                        && (input.pressed(Action::Confirm)
                            || input.pressed(Action::Left)
                            || input.pressed(Action::Right))
                    {
                        ui_prefs.high_contrast = !ui_prefs.high_contrast;
                        ui_prefs.save();
                    } else if ms.sel == 3 && delta != 0.0 {
                        ui_prefs.subtitle_scale =
                            (ui_prefs.subtitle_scale + delta).clamp(0.8, 1.4);
                        ui_prefs.save();
                    }
                }
            }

            let back_idx = count - 1;
            if ms.sel == back_idx && input.pressed(Action::Confirm) {
                back_to_main(&mut ms);
                trigger_screen_transition(&mut trans, Screen::Main);
            } else if input.pressed(Action::Cancel) {
                back_to_main(&mut ms);
                trigger_screen_transition(&mut trans, Screen::Main);
            }
        }
        SetupTeam => {
            navigate_list(&input, &mut ms.sel, wd.teams.len());
            if input.pressed(Action::Confirm) {
                ms.team = ms.sel;
                if ms.tournament_mode {
                    let t = start_tournament(ms.team, &wd);
                    ct.0 = Some(t);
                    ms.screen = Screen::Bracket;
                    ms.sel = 0;
                } else {
                    if ms.opp == ms.team { ms.opp = (ms.team + 1) % wd.teams.len(); }
                    ms.screen = SetupOpp;
                    ms.sel = ms.opp;
                }
            }
            if input.pressed(Action::Cancel) { back_to_main(&mut ms); }
        }
        SetupOpp => {
            navigate_list(&input, &mut ms.sel, wd.teams.len() - 1);
            let effective = if ms.sel >= ms.team { ms.sel + 1 } else { ms.sel };
            if input.pressed(Action::Confirm) {
                ms.opp = effective;
                ms.screen = SetupOvers;
                ms.sel = ms.overs_idx;
            }
            if input.pressed(Action::Cancel) { ms.screen = SetupTeam; ms.sel = ms.team; }
        }
        SetupOvers => {
            navigate_list(&input, &mut ms.sel, OVERS_CHOICES.len());
            if input.pressed(Action::Confirm) {
                ms.overs_idx = ms.sel;
                ms.screen = SetupStadium;
                ms.sel = ms.stadium_idx.min(wd.stadiums.len());
            }
            if input.pressed(Action::Cancel) { ms.screen = SetupTeam; ms.sel = ms.team; }
        }
        SetupStadium => {
            navigate_list(&input, &mut ms.sel, wd.stadiums.len() + 1);
            if input.pressed(Action::Confirm) {
                ms.stadium_idx = if ms.sel >= wd.stadiums.len() {
                    usize::MAX // random venue
                } else {
                    ms.sel
                };
                ms.screen = SetupBatFirst;
                ms.sel = usize::from(!ms.bat_first);
            }
            if input.pressed(Action::Cancel) {
                ms.screen = SetupOvers;
                ms.sel = ms.overs_idx;
            }
        }
        SetupBatFirst => {
            navigate_list(&input, &mut ms.sel, 2);
            if input.pressed(Action::Confirm) {
                ms.bat_first = ms.sel == 0;
                start_quick_match(&ms, &wd, &mut commands, &mut next_state);
            }
            if input.pressed(Action::Cancel) {
                ms.screen = SetupStadium;
                ms.sel = ms.stadium_idx.min(wd.stadiums.len());
            }
        }
        Bracket => {
            if input.pressed(Action::Cancel) {
                back_to_main(&mut ms);
                ct.0 = None;
                return;
            }
            let Some(t) = ct.0.as_ref() else {
                back_to_main(&mut ms);
                return;
            };
            if input.pressed(Action::Confirm) {
                if let Some(champ) = t.champion() {
                    info!("Champions: {}", t.teams[champ].name);
                    back_to_main(&mut ms);
                    ct.0 = None;
                } else if let Some((idx, f)) = t.next_user_fixture() {
                    launch_tournament_match(
                        t, idx, &f, &wd, &mut af,
                        &mut commands, &mut next_state,
                    );
                }
            }
        }
    }
}

fn navigate_list(input: &PlayerInput, sel: &mut usize, max: usize) {
    if max == 0 { return; }
    if input.pressed(Action::Next) {
        *sel = (*sel + 1) % max;
    }
    if input.pressed(Action::Prev) {
        *sel = (*sel + max - 1) % max;
    }
}

fn screen_item_count(ms: &MenuState, wd: &WorldData) -> usize {
    match ms.screen {
        Screen::Main => 4,
        Screen::SetupTeam | Screen::SetupOpp => wd.teams.len(),
        Screen::SetupOvers => OVERS_CHOICES.len(),
        Screen::SetupStadium => wd.stadiums.len() + 1,
        Screen::SetupBatFirst => 2,
        Screen::Settings => settings_item_count(ms.settings_tab),
        Screen::Bracket => 0,
    }
}

fn back_to_main(ms: &mut MenuState) {
    ms.screen = Screen::Main;
    ms.sel = 0;
}

/// Create the tournament when the user picks a team from the main menu
/// with Tournament selected (handled here to keep state machine simple).
pub fn start_tournament(
    user_world: usize,
    wd: &WorldData,
) -> Tournament {
    // User's team + three others chosen round-robin.
    let others: Vec<usize> = (0..wd.teams.len())
        .filter(|&i| i != user_world)
        .cycle()
        .skip(user_world % wd.teams.len().max(1))
        .take(3)
        .collect();
    let mut entrants: Vec<Entrant> = std::iter::once(user_world)
        .chain(others)
        .map(|w| Entrant { world_idx: w, team: wd.teams[w].clone() })
        .collect();
    // Find the local slot of the user AFTER sorting happens inside knockout:
    let user_name = wd.teams[user_world].name.clone();
    let stadiums = crate::core::stadiums::builtin_stadiums();
    // Pre-seed so we can find the local index post-sort.
    entrants.sort_by_key(|e| (crate::core::teams::team_rating(&e.team) * 10.0) as i64);
    let user_local = entrants
        .iter()
        .position(|e| e.team.name == user_name);
    Tournament::knockout(entrants, stadiums, user_local)
}

// ---------------------------------------------------------------------------
// Match launching / completion plumbing
// ---------------------------------------------------------------------------

fn start_quick_match(
    ms: &MenuState,
    wd: &WorldData,
    commands: &mut Commands,
    next_state: &mut NextState<AppState>,
) {
    let stadium = if ms.stadium_idx == usize::MAX {
        use std::collections::hash_map::RandomState;
        use std::hash::{BuildHasher, Hasher};
        RandomState::new().build_hasher().finish() as usize % wd.stadiums.len()
    } else {
        ms.stadium_idx
    };
    commands.insert_resource(MatchSetup {
        teams: [ms.team, ms.opp],
        stadium,
        overs: OVERS_CHOICES[ms.overs_idx],
        user_bats_first: ms.bat_first,
        from_tournament: false,
    });
    info!("Starting quick match");
    next_state.set(AppState::InMatch);
}

fn launch_tournament_match(
    t: &Tournament,
    fixture_idx: usize,
    f: &Fixture,
    _wd: &WorldData,
    active_fixture: &mut ActiveFixture,
    commands: &mut Commands,
    next_state: &mut NextState<AppState>,
) {
    let Some(user_local) = t.user_team else { return };
    let user_is_home = f.home == user_local;
    let opp_local = if user_is_home { f.away } else { f.home };
    let user_world = t.world_idx[user_local];
    let opp_world = t.world_idx[opp_local];
    commands.insert_resource(MatchSetup {
        teams: [user_world, opp_world],
        stadium: f.stadium.min(t.stadiums.len() - 1),
        overs: f.overs,
        // Home side bats first.
        user_bats_first: user_is_home,
        from_tournament: true,
    });
    active_fixture.0 = Some(fixture_idx);
    info!("Launching tournament fixture {}", fixture_idx);
    next_state.set(AppState::InMatch);
}

/// Runs during InMatch: when the match is over and the user confirms,
/// record the result into the tournament and return to the bracket.
#[allow(clippy::too_many_arguments)]
pub fn handle_match_exit(
    mut commands: Commands,
    phase: Res<Phase>,
    input: Res<PlayerInput>,
    am: Option<Res<ActiveMatch>>,
    mut ct: ResMut<CurrentTournament>,
    mut af: ResMut<ActiveFixture>,
    mut ms: ResMut<MenuState>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let PhaseEnum::MatchOver = phase.0 else { return };
    if !input.pressed(Action::Confirm) {
        return;
    }

    if ms.screen != Screen::Bracket && af.0.is_none() {
        // Quick match: straight back to the main menu.
        cleanup_after_match(&mut commands, &mut af);
        next_state.set(AppState::Menu);
        return;
    }

    if let (Some(t), Some(idx), Some(am)) =
        (ct.0.as_mut(), af.0, am.as_deref())
    {
        t.record_result(idx, &am.state, true);
        // Sim any fixtures that don't involve the user anymore.
        loop {
            let pending = t.pending_fixtures();
            let Some((pidx, pf)) = pending.into_iter().find(|(_, f)| {
                !t.user_team
                    .map(|u| f.home == u || f.away == u)
                    .unwrap_or(true)
            }) else {
                break;
            };
            t.sim_fixture(pidx, 0xBEEF + pidx as u64 + pf.stage as u64);
        }
    }
    ms.screen = Screen::Bracket;
    ms.sel = 0;
    cleanup_after_match(&mut commands, &mut af);
    next_state.set(AppState::Menu);
}

fn cleanup_after_match(commands: &mut Commands, af: &mut ActiveFixture) {
    af.0 = None;
    commands.remove_resource::<ActiveMatch>();
}

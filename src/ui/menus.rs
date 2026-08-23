//! Menus: main menu, match-setup wizard, controls help and the tournament
//! bracket screen. Keyboard/gamepad driven (see input::Action mapping).

use crate::core::tournament::{Entrant, Fixture, Stage, Tournament};
use crate::game::audio::AudioSettings;
use crate::game::user_bats_first_from_toss;
use crate::game::*;
use crate::input::{Action, KeyBindings, PlayerInput, RebindState, key_label};
use crate::state::AppState;
use crate::ui::theme::{
    self, MenuTransition, UiFonts, UiPreferences, UiScale, register_ui_font_assets,
};
use bevy::prelude::*;

const OVERS_CHOICES: [u32; 3] = [5, 10, 20];

/// Fixed menu panel size (scaled via [`theme::spx`]).
pub const MENU_PANEL_WIDTH: f32 = 720.0;
pub const MENU_PANEL_HEIGHT: f32 = 560.0;
pub const MENU_MAIN_LEFT_PCT: f32 = 7.0;

/// Team picker grid: card width, gap and columns shared by layout and navigation.
pub const TEAM_CARD_WIDTH: f32 = 140.0;
pub const TEAM_GRID_GAP: f32 = 10.0;
pub const TEAM_GRID_COLUMNS: usize = 4;
pub const TEAM_GRID_WIDTH: f32 =
    TEAM_CARD_WIDTH * TEAM_GRID_COLUMNS as f32 + TEAM_GRID_GAP * (TEAM_GRID_COLUMNS as f32 - 1.0);

const TOSS_FLIP_DURATION: f32 = 2.0;
const TOSS_RESULT_PAUSE: f32 = 1.5;

/// Approximate row stride for scroll math (design pixels, pre-scale).
const STADIUM_ROW_HEIGHT: f32 = 100.0;
const STADIUM_ROW_GAP: f32 = 8.0;
const STADIUM_LIST_VIEWPORT_HEIGHT: f32 = 320.0;

const SETTINGS_ROW_HEIGHT: f32 = 28.0;
const SETTINGS_ROW_GAP: f32 = 3.0;
const SETTINGS_LIST_VIEWPORT_HEIGHT: f32 = 268.0;

const BRACKET_ROW_HEIGHT: f32 = 26.0;
const BRACKET_ROW_GAP: f32 = 7.0;
const BRACKET_LIST_VIEWPORT_HEIGHT: f32 = 310.0;

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
    SetupTossCall,
    SetupTossFlip,
    SetupTossResult,
    SetupTossChoice,
    SetupTossSummary,
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
    /// WorldData team index that won the toss.
    pub toss_winner: usize,
    /// What the toss winner elected (bat first).
    pub toss_elects_bat: bool,
    /// Coin flip outcome (heads = true); fixed when the toss begins.
    pub coin_heads: bool,
    /// true when the player called heads at the toss.
    pub toss_call_heads: bool,
    /// Settings screen active tab.
    pub settings_tab: SettingsTab,
    /// true when the current setup wizard leads into a tournament.
    pub tournament_mode: bool,
}

/// Animation timers that must not trigger full menu rebuilds each frame.
#[derive(Resource, Default)]
struct MenuAnimTime(pub f32);

/// Snapshot of every value that [`refresh_menu`] uses to build menu content.
/// Compared explicitly so spurious `ResMut` change ticks never force a rebuild.
#[derive(Debug, PartialEq, Clone)]
struct MenuContentSignature {
    screen: Screen,
    sel: usize,
    settings_tab: SettingsTab,
    team: usize,
    opp: usize,
    overs_idx: usize,
    stadium_idx: usize,
    bat_first: bool,
    toss_winner: usize,
    toss_elects_bat: bool,
    audio: AudioSignature,
    bindings: Vec<(Action, KeyCode)>,
    rebind: Option<Action>,
    ui_prefs: UiPrefsSignature,
    ui_scale: f32,
    bracket: BracketSignature,
}

#[derive(Debug, PartialEq, Clone, Copy)]
struct AudioSignature {
    master: f32,
    sfx: f32,
    music: f32,
    commentary: crate::game::audio::CommentaryVoice,
    commentary_volume: f32,
}

#[derive(Debug, PartialEq, Clone, Copy)]
struct UiPrefsSignature {
    ui_scale: f32,
    high_contrast: bool,
    subtitle_scale: f32,
}

#[derive(Debug, PartialEq, Clone)]
struct BracketSignature {
    inner: Option<BracketInnerSignature>,
}

#[derive(Debug, PartialEq, Clone)]
struct BracketInnerSignature {
    name: String,
    fixtures: Vec<BracketFixtureSignature>,
    user_team: Option<usize>,
}

#[derive(Debug, PartialEq, Clone)]
struct BracketFixtureSignature {
    stage: Stage,
    home: usize,
    away: usize,
    stadium: usize,
    result: Option<crate::core::rules::Result>,
}

#[derive(Resource, Default)]
struct MenuRebuildState {
    signature: Option<MenuContentSignature>,
}

fn menu_content_signature(
    ms: &MenuState,
    ct: &CurrentTournament,
    bindings: &KeyBindings,
    audio: &AudioSettings,
    rebind: &RebindState,
    ui_prefs: &UiPreferences,
    ui_scale: &UiScale,
) -> MenuContentSignature {
    let mut binding_entries: Vec<_> = bindings
        .map
        .iter()
        .map(|(&action, &key)| (action, key))
        .collect();
    binding_entries.sort_by(|(a, _), (b, _)| format!("{a:?}").cmp(&format!("{b:?}")));

    MenuContentSignature {
        screen: ms.screen,
        sel: ms.sel,
        settings_tab: ms.settings_tab,
        team: ms.team,
        opp: ms.opp,
        overs_idx: ms.overs_idx,
        stadium_idx: ms.stadium_idx,
        bat_first: ms.bat_first,
        toss_winner: ms.toss_winner,
        toss_elects_bat: ms.toss_elects_bat,
        audio: AudioSignature {
            master: audio.master,
            sfx: audio.sfx,
            music: audio.music,
            commentary: audio.commentary,
            commentary_volume: audio.commentary_volume,
        },
        bindings: binding_entries,
        rebind: rebind.0,
        ui_prefs: UiPrefsSignature {
            ui_scale: ui_prefs.ui_scale,
            high_contrast: ui_prefs.high_contrast,
            subtitle_scale: ui_prefs.subtitle_scale,
        },
        ui_scale: ui_scale.0,
        bracket: BracketSignature {
            inner: ct.0.as_ref().map(|t| BracketInnerSignature {
                name: t.name.clone(),
                fixtures: t
                    .fixtures
                    .iter()
                    .map(|f| BracketFixtureSignature {
                        stage: f.stage,
                        home: f.home,
                        away: f.away,
                        stadium: f.stadium,
                        result: f.result.clone(),
                    })
                    .collect(),
                user_team: t.user_team,
            }),
        },
    }
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
            toss_winner: 0,
            toss_elects_bat: true,
            coin_heads: true,
            toss_call_heads: true,
            settings_tab: SettingsTab::Audio,
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
struct MenuFadeOverlay;
#[derive(Component)]
struct MenuCoinLabel;
#[derive(Component)]
struct MenuCoinVisual;
#[derive(Component)]
struct MenuCoinFace {
    heads: bool,
}

pub struct MenusPlugin;

impl Plugin for MenusPlugin {
    fn build(&self, app: &mut App) {
        // Menu key art is embedded so `target/release/cricket` remains a
        // self-contained executable, matching the existing launch instructions.
        bevy::asset::embedded_asset!(app, "../../assets/ui/main-menu-hero.png");
        bevy::asset::embedded_asset!(app, "../../assets/ui/menu-stadium.png");
        register_ui_font_assets(app);
        app.init_resource::<MenuState>()
            .init_resource::<MenuAnimTime>()
            .init_resource::<MenuRebuildState>()
            .init_resource::<CurrentTournament>()
            .init_resource::<ActiveFixture>()
            .add_systems(OnEnter(AppState::Menu), spawn_menu_root)
            .add_systems(OnExit(AppState::Menu), despawn_menu_root)
            .add_systems(
                Update,
                (
                    sync_ui_scale,
                    tick_menu_anim,
                    update_menu_animations,
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

fn tick_menu_anim(time: Res<Time>, mut anim: ResMut<MenuAnimTime>, mut ms: ResMut<MenuState>) {
    match ms.screen {
        Screen::SetupTossFlip => {
            anim.0 += time.delta_secs();
            if anim.0 >= TOSS_FLIP_DURATION {
                ms.screen = Screen::SetupTossResult;
                anim.0 = 0.0;
            }
        }
        Screen::SetupTossResult => {
            anim.0 += time.delta_secs();
            if anim.0 >= TOSS_RESULT_PAUSE {
                ms.screen = Screen::SetupTossChoice;
                anim.0 = 0.0;
                if ms.toss_winner != ms.team {
                    ms.toss_elects_bat = ai_toss_election();
                    ms.bat_first =
                        user_bats_first_from_toss(ms.team, ms.toss_winner, ms.toss_elects_bat);
                } else {
                    ms.sel = 0;
                }
            }
        }
        _ => anim.0 = 0.0,
    }
}

fn ai_toss_election() -> bool {
    rand::random::<f32>() < 0.58
}

fn update_menu_animations(
    anim: Res<MenuAnimTime>,
    ms: Res<MenuState>,
    trans: Res<MenuTransition>,
    mut coin_q: Query<&mut Text, With<MenuCoinLabel>>,
    mut coin_visual_q: Query<&mut Transform, With<MenuCoinVisual>>,
    mut coin_face_q: Query<(&MenuCoinFace, &mut Visibility)>,
    mut fade_q: Query<&mut BackgroundColor, With<MenuFadeOverlay>>,
) {
    let coin_side_heads = if ms.screen == Screen::SetupTossFlip {
        (anim.0 * 8.0).sin() > 0.0
    } else {
        ms.coin_heads
    };

    if ms.screen == Screen::SetupTossFlip {
        let spin = anim.0 * 6.0;
        let squash = (anim.0 * 12.0).sin().abs().max(0.14);
        for mut transform in coin_visual_q.iter_mut() {
            transform.rotation = Quat::from_rotation_y(spin);
            transform.scale = Vec3::new(squash, 1.0, 1.0);
        }
    } else if matches!(
        ms.screen,
        Screen::SetupTossResult | Screen::SetupTossChoice | Screen::SetupTossSummary
    ) {
        for mut transform in coin_visual_q.iter_mut() {
            transform.rotation = Quat::IDENTITY;
            transform.scale = Vec3::ONE;
        }
    }

    for (face, mut vis) in coin_face_q.iter_mut() {
        *vis = if face.heads == coin_side_heads {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }

    if ms.screen == Screen::SetupTossFlip {
        let coin_side = if coin_side_heads { "HEADS" } else { "TAILS" };
        for mut text in coin_q.iter_mut() {
            **text = coin_side.to_string();
        }
    }

    let fade = if trans.active {
        (trans.t / 0.28).min(1.0)
    } else {
        0.0
    };
    for mut bg in fade_q.iter_mut() {
        bg.0 = Color::srgba(0.02, 0.05, 0.04, fade * 0.55);
    }
}

fn trigger_screen_transition(trans: &mut MenuTransition, _screen: Screen) {
    trans.active = true;
    trans.t = 0.0;
}

// ---------------------------------------------------------------------------
// UI construction (immediate-mode style rebuild each frame)
// ---------------------------------------------------------------------------

fn spawn_menu_root(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut built: ResMut<MenuRebuildState>,
) {
    built.signature = None;
    info!("MENU ROOT SPAWNED");
    commands
        .spawn((
            MenuRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
            BackgroundColor(Color::srgb(0.015, 0.025, 0.022)),
        ))
        .with_children(|p| {
            let main = bevy::asset::load_embedded_asset!(
                assets.as_ref(),
                "../../assets/ui/main-menu-hero.png"
            );
            let secondary = bevy::asset::load_embedded_asset!(
                assets.as_ref(),
                "../../assets/ui/menu-stadium.png"
            );
            let fonts = UiFonts::load(assets.as_ref());
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
                fonts,
                Node {
                    position_type: PositionType::Absolute,
                    flex_direction: FlexDirection::Column,
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(8)),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.015, 0.035, 0.028, 0.90)),
                BorderColor::all(Color::srgba(0.72, 0.82, 0.56, 0.28)),
            ));
            p.spawn((
                MenuFadeOverlay,
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(100),
                    height: percent(100),
                    left: px(0),
                    top: px(0),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.02, 0.05, 0.04, 0.0)),
                ZIndex(20),
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
        Screen::SetupTossCall
        | Screen::SetupTossFlip
        | Screen::SetupTossResult
        | Screen::SetupTossChoice => "TOSS",
        Screen::SetupTossSummary => "MATCH PREVIEW",
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
        Screen::SetupTossCall
        | Screen::SetupTossFlip
        | Screen::SetupTossResult
        | Screen::SetupTossChoice
        | Screen::SetupTossSummary => "MATCH SETUP  /  05",
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
        Screen::SetupOvers => OVERS_CHOICES.iter().map(|o| format!("{o} overs")).collect(),
        Screen::SetupStadium => wd
            .stadiums
            .iter()
            .map(|s| format!("{} — {} [{}]", s.name, s.city, s.pitch.label()))
            .chain(std::iter::once("Random venue".into()))
            .collect(),
        Screen::SetupTossCall
        | Screen::SetupTossFlip
        | Screen::SetupTossResult
        | Screen::SetupTossChoice
        | Screen::SetupTossSummary => Vec::new(),
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
        SettingsTab::Audio => 6,     // header + 5 + back
        SettingsTab::Controls => 12, // header + 10 + reset + back
        SettingsTab::Display => 5,   // header + 3 + back
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
            out.push(format!(
                "{}: TBD v TBD @ {}",
                f.stage.label(),
                t.stadiums[f.stadium].name
            ));
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

fn apply_menu_root_layout(root_node: &mut Node, is_main: bool, scale: f32) {
    let w = MENU_PANEL_WIDTH * scale;
    let h = MENU_PANEL_HEIGHT * scale;
    root_node.position_type = PositionType::Absolute;
    root_node.width = px(w);
    root_node.height = px(h);
    root_node.min_width = px(w);
    root_node.min_height = px(h);
    root_node.max_width = px(w);
    root_node.max_height = px(h);
    root_node.top = percent(50);
    root_node.margin = UiRect::new(px(0.0), px(-h * 0.5), px(0.0), px(0.0));
    if is_main {
        root_node.left = percent(MENU_MAIN_LEFT_PCT);
        root_node.padding = UiRect::axes(px(34.0 * scale), px(30.0 * scale));
        root_node.align_items = AlignItems::Stretch;
        root_node.row_gap = px(9.0 * scale);
    } else {
        root_node.left = percent(50);
        root_node.margin = UiRect::new(px(-w * 0.5), px(-h * 0.5), px(0.0), px(0.0));
        root_node.padding = UiRect::axes(px(30.0 * scale), px(20.0 * scale));
        root_node.align_items = AlignItems::Center;
        root_node.row_gap = px(6.0 * scale);
    }
}

fn clear_menu_rows(commands: &mut Commands, root: Entity, children_q: &Query<&Children>) {
    if let Ok(children) = children_q.get(root) {
        for c in children.iter() {
            commands.entity(c).despawn();
        }
    }
}

fn auto_advance_bracket_draw(ms: &MenuState, ct: &mut CurrentTournament) {
    if ms.screen == Screen::Bracket
        && let Some(t) = ct.0.as_mut()
        && t.champion().is_none()
        && t.next_user_fixture().is_none()
    {
        for (idx, _f) in t.pending_fixtures() {
            t.sim_fixture(idx, 0xC0FFEE + idx as u64);
        }
    }
}

fn spawn_menu_header_block(
    parent: &mut ChildSpawnerCommands,
    ms: &MenuState,
    fonts: &UiFonts,
    is_main: bool,
) {
    parent.spawn((
        Text::new(screen_kicker(ms)),
        TextFont {
            font: fonts.bold.clone(),
            font_size: if is_main { 12.0 } else { 11.0 },
            ..default()
        },
        TextColor(Color::srgb(0.88, 0.72, 0.29)),
    ));
    parent.spawn((
        Text::new(if is_main {
            "WILLOW\nCRICKET"
        } else {
            screen_title(ms)
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
    parent.spawn((
        Node {
            width: if is_main { percent(45) } else { px(72) },
            height: px(3),
            margin: UiRect::vertical(px(8)),
            ..default()
        },
        BackgroundColor(Color::srgb(0.78, 0.64, 0.25)),
    ));
}

fn list_content_height(item_count: usize, row_height: f32, row_gap: f32) -> f32 {
    if item_count == 0 {
        return 0.0;
    }
    row_height * item_count as f32 + row_gap * item_count.saturating_sub(1) as f32
}

fn spawn_scroll_edge_hint(
    parent: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    scale: f32,
    top: bool,
    show: bool,
) {
    if !show {
        return;
    }
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: theme::spx(8.0, scale),
                height: theme::spx(20.0, scale),
                top: if top { px(0) } else { Val::Auto },
                bottom: if top { Val::Auto } else { px(0) },
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(theme::palette::scroll_edge_fade()),
            ZIndex(3),
        ))
        .with_children(|hint| {
            hint.spawn((
                // U+2191/U+2193 rather than the filled triangles: the bundled
                // UI font has no glyph for those and renders a tofu box.
                Text::new(if top { "↑" } else { "↓" }),
                TextFont {
                    font: fonts.bold.clone(),
                    font_size: 10.0 * scale,
                    ..default()
                },
                TextColor(theme::palette::text_muted()),
            ));
        });
}

fn spawn_scroll_thumb(
    parent: &mut ChildSpawnerCommands,
    scale: f32,
    offset: f32,
    viewport_height: f32,
    content_height: f32,
) {
    let max_offset = (content_height - viewport_height).max(0.0);
    if max_offset <= 0.0 {
        return;
    }
    let track_h = viewport_height;
    let thumb_h = (viewport_height / content_height * track_h).clamp(24.0 * scale, track_h);
    let thumb_top = if max_offset > 0.0 {
        (offset / max_offset) * (track_h - thumb_h)
    } else {
        0.0
    };

    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: px(0),
                top: px(0),
                width: theme::spx(4.0, scale),
                height: px(viewport_height),
                ..default()
            },
            BackgroundColor(theme::palette::scroll_track()),
            ZIndex(2),
        ))
        .with_children(|track| {
            track.spawn((
                Node {
                    width: percent(100),
                    height: px(thumb_h),
                    margin: UiRect::top(px(thumb_top)),
                    border_radius: BorderRadius::all(theme::spx(2.0, scale)),
                    ..default()
                },
                BackgroundColor(theme::palette::scroll_thumb()),
            ));
        });
}

/// Clipped, keyboard-driven scroll region with edge hints and a thumb indicator.
fn spawn_scroll_viewport<R>(
    parent: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    scale: f32,
    viewport_height: f32,
    selected: usize,
    item_count: usize,
    row_height: f32,
    row_gap: f32,
    width: Val,
    build_rows: R,
) where
    R: FnOnce(&mut ChildSpawnerCommands),
{
    let row_h = theme::scaled_px(row_height, scale);
    let gap = theme::scaled_px(row_gap, scale);
    let content_h = list_content_height(item_count, row_h, gap);
    let max_offset = (content_h - viewport_height).max(0.0);
    let offset = theme::list_scroll_offset(selected, row_h, gap, viewport_height, item_count);
    let can_scroll = max_offset > 0.5;
    let show_top = offset > 0.5;
    let show_bottom = offset < max_offset - 0.5;

    parent
        .spawn((Node {
            position_type: PositionType::Relative,
            width,
            height: px(viewport_height),
            min_height: px(viewport_height),
            max_height: px(viewport_height),
            flex_shrink: 0.0,
            ..default()
        },))
        .with_children(|shell| {
            shell
                .spawn((
                    Node {
                        width: percent(100),
                        height: percent(100),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                    ScrollPosition(Vec2::new(0.0, offset)),
                ))
                .with_children(|viewport| {
                    viewport
                        .spawn((Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: theme::spx(row_gap, scale),
                            width: percent(100),
                            ..default()
                        },))
                        .with_children(build_rows);
                });

            if can_scroll {
                spawn_scroll_edge_hint(shell, fonts, scale, true, show_top);
                spawn_scroll_edge_hint(shell, fonts, scale, false, show_bottom);
                spawn_scroll_thumb(shell, scale, offset, viewport_height, content_h);
            }
        });
}

fn spawn_menu_settings_tabs(
    parent: &mut ChildSpawnerCommands,
    ms: &MenuState,
    fonts: &UiFonts,
    scale: f32,
) {
    parent
        .spawn((Node {
            flex_direction: FlexDirection::Row,
            column_gap: theme::spx(8.0, scale),
            margin: UiRect::bottom(theme::spx(6.0, scale)),
            ..default()
        },))
        .with_children(|tabs| {
            for tab in [
                SettingsTab::Audio,
                SettingsTab::Controls,
                SettingsTab::Display,
            ] {
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
                ))
                .with_children(|t| {
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

fn spawn_menu_item_rows(
    parent: &mut ChildSpawnerCommands,
    ms: &MenuState,
    fonts: &UiFonts,
    lines: Vec<String>,
    is_main: bool,
) {
    for (i, line) in lines.into_iter().enumerate() {
        if matches!(
            ms.screen,
            Screen::SetupTeam | Screen::SetupOpp | Screen::SetupStadium | Screen::SetupOvers
        ) && !line.starts_with("Tab:")
        {
            continue;
        }
        if matches!(
            ms.screen,
            Screen::SetupTossCall
                | Screen::SetupTossFlip
                | Screen::SetupTossResult
                | Screen::SetupTossChoice
                | Screen::SetupTossSummary
        ) {
            continue;
        }
        let selectable = !matches!(ms.screen, Screen::Bracket);
        let selected = i == ms.sel && selectable;
        let is_settings = ms.screen == Screen::Settings;
        parent
            .spawn((
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
                        Node {
                            width: px(38),
                            ..default()
                        },
                    ));
                    row.spawn((
                        Text::new(line.to_uppercase()),
                        TextFont {
                            font: face,
                            font_size: 20.0,
                            ..default()
                        },
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
                            Node {
                                width: px(235),
                                ..default()
                            },
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
                            Node {
                                width: px(270),
                                ..default()
                            },
                        ));
                    } else {
                        row.spawn((
                            Text::new(line),
                            TextFont {
                                font: face,
                                font_size: 16.0,
                                ..default()
                            },
                            TextColor(text_color),
                        ));
                    }
                } else {
                    row.spawn((
                        Text::new(line),
                        TextFont {
                            font: face,
                            font_size: if ms.screen == Screen::Bracket {
                                18.0
                            } else {
                                19.0
                            },
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
}

fn screen_footer_hint(ms: &MenuState) -> &'static str {
    match ms.screen {
        Screen::Settings => {
            "Q / E  SWITCH TAB     ↑↓ / W S  NAVIGATE     SPACE  SELECT     ESC  BACK"
        }
        Screen::Main => "↑↓ / W S  NAVIGATE     SPACE  SELECT     ESC  BACK",
        Screen::SetupTeam | Screen::SetupOpp => {
            "↑↓←→ / W A S D  NAVIGATE     SPACE  SELECT     ESC  BACK"
        }
        Screen::SetupOvers => "←→ / A D  NAVIGATE     SPACE  SELECT     ESC  BACK",
        Screen::SetupStadium => "↑↓ / W S  NAVIGATE     SPACE  SELECT     ESC  BACK",
        Screen::SetupTossCall => "←→ / A D  CHOOSE     SPACE  CALL     ESC  BACK",
        Screen::SetupTossFlip | Screen::SetupTossResult => "ESC  BACK",
        Screen::SetupTossChoice => "←→ / A D  CHOOSE     SPACE  CONFIRM     ESC  BACK",
        Screen::SetupTossSummary => "SPACE  CONTINUE     ESC  BACK",
        Screen::Bracket => "",
    }
}

fn spawn_menu_footer_hint(
    parent: &mut ChildSpawnerCommands,
    ms: &MenuState,
    fonts: &UiFonts,
    scale: f32,
) {
    if ms.screen == Screen::Bracket {
        return;
    }
    let hint = screen_footer_hint(ms);
    parent.spawn((
        Text::new(hint),
        TextFont {
            font: fonts.bold.clone(),
            font_size: 10.0 * scale,
            ..default()
        },
        TextColor(Color::srgba(0.72, 0.76, 0.70, 0.72)),
        Node {
            margin: UiRect::top(theme::spx(8.0, scale)),
            ..default()
        },
    ));
}

fn menu_content_dirty(built: &MenuRebuildState, signature: &MenuContentSignature) -> bool {
    built.signature.as_ref() != Some(signature)
}

fn refresh_menu(
    ms: Res<MenuState>,
    wd: Res<WorldData>,
    ct: Res<CurrentTournament>,
    bindings: Res<KeyBindings>,
    audio: Res<AudioSettings>,
    rebind: Res<RebindState>,
    ui_prefs: Res<UiPreferences>,
    ui_scale: Res<UiScale>,
    assets: Res<AssetServer>,
    mut root_q: Query<(Entity, &mut Node, &UiFonts), With<MenuList>>,
    mut backdrop_q: Query<(&mut ImageNode, &MenuBackdrop)>,
    children_q: Query<&Children>,
    mut built: ResMut<MenuRebuildState>,
    mut commands: Commands,
) {
    let Ok((root, mut root_node, fonts)) = root_q.single_mut() else {
        return;
    };
    let is_main = ms.screen == Screen::Main;
    let scale = ui_scale.0;
    let signature =
        menu_content_signature(&ms, &ct, &bindings, &audio, &rebind, &ui_prefs, &ui_scale);
    let dirty = menu_content_dirty(&built, &signature);

    if !dirty {
        return;
    }

    if let Ok((mut image, art)) = backdrop_q.single_mut() {
        image.image = if is_main {
            art.main.clone()
        } else {
            art.secondary.clone()
        };
    }
    apply_menu_root_layout(&mut root_node, is_main, scale);

    clear_menu_rows(&mut commands, root, &children_q);

    commands.entity(root).with_children(|p| {
        spawn_menu_header_block(p, &ms, fonts, is_main);

        p.spawn((Node {
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            align_items: if is_main {
                AlignItems::Stretch
            } else {
                AlignItems::Center
            },
            justify_content: if matches!(
                ms.screen,
                Screen::SetupTossCall
                    | Screen::SetupTossFlip
                    | Screen::SetupTossResult
                    | Screen::SetupTossChoice
                    | Screen::SetupTossSummary
            ) {
                JustifyContent::SpaceBetween
            } else {
                JustifyContent::Center
            },
            overflow: Overflow::clip(),
            row_gap: theme::spx(
                if ms.screen == Screen::Settings {
                    3.0
                } else {
                    7.0
                },
                scale,
            ),
            width: percent(100),
            min_height: px(0.0),
            margin: UiRect::vertical(theme::spx(if is_main { 10.0 } else { 4.0 }, scale)),
            ..default()
        },))
            .with_children(|items| {
                if ms.screen == Screen::Settings {
                    spawn_menu_settings_tabs(items, &ms, fonts, scale);
                }

                if matches!(
                    ms.screen,
                    Screen::SetupTeam
                        | Screen::SetupOpp
                        | Screen::SetupStadium
                        | Screen::SetupOvers
                        | Screen::SetupTossCall
                        | Screen::SetupTossFlip
                        | Screen::SetupTossResult
                        | Screen::SetupTossChoice
                        | Screen::SetupTossSummary
                ) {
                    spawn_setup_visuals(items, &ms, &wd, &assets, fonts, scale, &ui_prefs);
                }

                let lines = if ms.screen == Screen::Settings {
                    settings_lines(ms.settings_tab, &bindings, &audio, &rebind, &ui_prefs)
                } else {
                    screen_items(&ms, &wd, &ct, &bindings, &audio, &rebind, &ui_prefs)
                };

                if matches!(ms.screen, Screen::Settings | Screen::Bracket) {
                    let (row_h, row_gap, vp_design) = if ms.screen == Screen::Settings {
                        (
                            SETTINGS_ROW_HEIGHT,
                            SETTINGS_ROW_GAP,
                            SETTINGS_LIST_VIEWPORT_HEIGHT,
                        )
                    } else {
                        (
                            BRACKET_ROW_HEIGHT,
                            BRACKET_ROW_GAP,
                            BRACKET_LIST_VIEWPORT_HEIGHT,
                        )
                    };
                    let vp_h = theme::scaled_px(vp_design, scale);
                    spawn_scroll_viewport(
                        items,
                        fonts,
                        scale,
                        vp_h,
                        ms.sel,
                        lines.len(),
                        row_h,
                        row_gap,
                        percent(100),
                        |content| spawn_menu_item_rows(content, &ms, fonts, lines, is_main),
                    );
                } else {
                    spawn_menu_item_rows(items, &ms, fonts, lines, is_main);
                }
            });

        spawn_menu_footer_hint(p, &ms, fonts, scale);
    });

    built.signature = Some(signature);
}

/// Layout variant for [`spawn_setup_card`].
enum SetupCardStyle {
    Team { scale: f32, high_contrast: bool },
    Overs { scale: f32 },
}

/// Metadata for a stadium list row.
struct StadiumRowMeta<'a> {
    city: Option<&'a str>,
    pitch: Option<&'a str>,
    boundary_m: Option<f32>,
}

fn spawn_meta_chip(parent: &mut ChildSpawnerCommands, fonts: &UiFonts, scale: f32, label: &str) {
    parent
        .spawn((
            Node {
                padding: UiRect::axes(theme::spx(10.0, scale), theme::spx(4.0, scale)),
                border_radius: BorderRadius::all(theme::spx(4.0, scale)),
                ..default()
            },
            BackgroundColor(theme::palette::chip_bg()),
        ))
        .with_children(|chip| {
            chip.spawn((
                Text::new(label),
                TextFont {
                    font: fonts.bold.clone(),
                    font_size: 10.0 * scale,
                    ..default()
                },
                TextColor(theme::palette::text_muted()),
            ));
        });
}

fn spawn_stadium_row(
    parent: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    scale: f32,
    title: &str,
    meta: StadiumRowMeta<'_>,
    selected: bool,
) {
    let bg = if selected {
        theme::palette::selection_bg()
    } else {
        theme::palette::card_bg()
    };
    let border = if selected {
        theme::palette::selection_border()
    } else {
        theme::palette::card_border()
    };
    let title_color = if selected {
        Color::srgb(1.0, 0.95, 0.76)
    } else {
        theme::palette::text_primary()
    };

    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: theme::spx(STADIUM_ROW_HEIGHT, scale),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexStart,
                row_gap: theme::spx(6.0, scale),
                padding: UiRect::all(theme::spx(12.0, scale)),
                border: UiRect::all(px(if selected { 2 } else { 1 })),
                border_radius: BorderRadius::all(theme::spx(6.0, scale)),
                ..default()
            },
            BackgroundColor(bg),
            BorderColor::all(border),
        ))
        .with_children(|card| {
            card.spawn((
                Text::new(title),
                TextFont {
                    font: fonts.bold.clone(),
                    font_size: 16.0 * scale,
                    ..default()
                },
                TextColor(title_color),
            ));
            if let Some(city) = meta.city {
                card.spawn((
                    Text::new(city),
                    TextFont {
                        font: fonts.regular.clone(),
                        font_size: 12.0 * scale,
                        ..default()
                    },
                    TextColor(theme::palette::text_muted()),
                ));
            }
            if meta.pitch.is_some() || meta.boundary_m.is_some() {
                card.spawn((Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: theme::spx(8.0, scale),
                    ..default()
                },))
                    .with_children(|chips| {
                        if let Some(pitch) = meta.pitch {
                            spawn_meta_chip(chips, fonts, scale, pitch);
                        }
                        if let Some(boundary) = meta.boundary_m {
                            spawn_meta_chip(
                                chips,
                                fonts,
                                scale,
                                &format!("{boundary:.0}m boundary"),
                            );
                        }
                    });
            }
        });
}

fn spawn_toss_coin(parent: &mut ChildSpawnerCommands, fonts: &UiFonts, scale: f32, heads: bool) {
    let coin_size = theme::spx(72.0, scale);
    parent
        .spawn((
            MenuCoinVisual,
            Node {
                width: coin_size,
                height: coin_size,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            Transform::default(),
        ))
        .with_children(|coin| {
            for (is_heads, label) in [(true, "H"), (false, "T")] {
                coin.spawn((
                    MenuCoinFace { heads: is_heads },
                    Node {
                        position_type: PositionType::Absolute,
                        width: percent(100),
                        height: percent(100),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border: UiRect::all(px(3)),
                        border_radius: BorderRadius::all(theme::spx(36.0, scale)),
                        ..default()
                    },
                    BackgroundColor(theme::palette::coin_face()),
                    BorderColor::all(theme::palette::coin_edge()),
                    if is_heads == heads {
                        Visibility::Visible
                    } else {
                        Visibility::Hidden
                    },
                ))
                .with_children(|face| {
                    face.spawn((
                        Text::new(label),
                        TextFont {
                            font: fonts.display.clone(),
                            font_size: 28.0 * scale,
                            ..default()
                        },
                        TextColor(Color::srgb(0.18, 0.14, 0.06)),
                    ));
                });
            }
        });
    parent.spawn((
        MenuCoinLabel,
        Text::new(if heads { "HEADS" } else { "TAILS" }),
        TextFont {
            font: fonts.bold.clone(),
            font_size: 12.0 * scale,
            ..default()
        },
        TextColor(theme::palette::text_muted()),
        Node {
            margin: UiRect::top(theme::spx(6.0, scale)),
            ..default()
        },
    ));
}

fn spawn_toss_side(
    parent: &mut ChildSpawnerCommands,
    assets: &AssetServer,
    fonts: &UiFonts,
    team: &crate::core::teams::Team,
    scale: f32,
) {
    parent
        .spawn((Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: theme::spx(8.0, scale),
            width: theme::spx(120.0, scale),
            ..default()
        },))
        .with_children(|side| {
            side.spawn((
                ImageNode::new(crate::render::load_team_crest(assets, &team.crest_asset())),
                Node {
                    width: theme::spx(88.0, scale),
                    height: theme::spx(88.0, scale),
                    ..default()
                },
            ));
            side.spawn((
                Text::new(team.name.to_uppercase()),
                TextFont {
                    font: fonts.bold.clone(),
                    font_size: 13.0 * scale,
                    ..default()
                },
                TextColor(theme::palette::text_primary()),
                Node {
                    max_width: theme::spx(120.0, scale),
                    ..default()
                },
            ));
            side.spawn((
                Text::new(team.short.to_uppercase()),
                TextFont {
                    font: fonts.bold.clone(),
                    font_size: 18.0 * scale,
                    ..default()
                },
                TextColor(team.primary_color),
            ));
        });
}

/// Shared selectable card for team and overs pickers.
fn spawn_setup_card(
    parent: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    style: SetupCardStyle,
    label: &str,
    sub_label: Option<&str>,
    image: Option<Handle<Image>>,
    accent_color: Option<Color>,
    selected: bool,
) {
    let (bg, border) = match &style {
        SetupCardStyle::Team { high_contrast, .. } => (
            if selected {
                if *high_contrast {
                    Color::srgba(0.35, 0.35, 0.40, 0.95)
                } else {
                    theme::palette::selection_bg()
                }
            } else {
                theme::palette::card_bg()
            },
            if selected {
                theme::palette::selection_border()
            } else {
                theme::palette::card_border()
            },
        ),
        SetupCardStyle::Overs { .. } => (
            if selected {
                theme::palette::selection_bg()
            } else {
                theme::palette::card_bg()
            },
            if selected {
                theme::palette::gold()
            } else {
                theme::palette::card_border()
            },
        ),
    };

    match style {
        SetupCardStyle::Team { scale, .. } => {
            parent
                .spawn((
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
                    BackgroundColor(bg),
                    BorderColor::all(border),
                ))
                .with_children(|card| {
                    if let Some(img) = image {
                        card.spawn((
                            ImageNode::new(img),
                            Node {
                                width: theme::spx(52.0, scale),
                                height: theme::spx(52.0, scale),
                                ..default()
                            },
                        ));
                    }
                    card.spawn((
                        Text::new(label),
                        TextFont {
                            font: fonts.display.clone(),
                            font_size: 18.0 * scale,
                            ..default()
                        },
                        TextColor(theme::palette::text_primary()),
                    ));
                    if let Some(accent) = accent_color {
                        card.spawn((
                            Node {
                                width: theme::spx(90.0, scale),
                                height: theme::spx(6.0, scale),
                                margin: UiRect::top(theme::spx(4.0, scale)),
                                ..default()
                            },
                            BackgroundColor(accent),
                        ));
                    }
                    if let Some(sub) = sub_label {
                        card.spawn((
                            Text::new(sub),
                            TextFont {
                                font: fonts.regular.clone(),
                                font_size: 10.0 * scale,
                                ..default()
                            },
                            TextColor(theme::palette::text_muted()),
                        ));
                    }
                });
        }
        SetupCardStyle::Overs { scale } => {
            parent
                .spawn((
                    Node {
                        width: theme::spx(120.0, scale),
                        height: theme::spx(90.0, scale),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border: UiRect::all(px(if selected { 3 } else { 1 })),
                        border_radius: BorderRadius::all(theme::spx(6.0, scale)),
                        ..default()
                    },
                    BackgroundColor(bg),
                    BorderColor::all(border),
                ))
                .with_children(|card| {
                    card.spawn((
                        Text::new(label),
                        TextFont {
                            font: fonts.display.clone(),
                            font_size: 36.0 * scale,
                            ..default()
                        },
                        TextColor(Color::WHITE),
                    ));
                    if let Some(sub) = sub_label {
                        card.spawn((
                            Text::new(sub),
                            TextFont {
                                font: fonts.bold.clone(),
                                font_size: 11.0 * scale,
                                ..default()
                            },
                            TextColor(theme::palette::text_muted()),
                        ));
                    }
                });
        }
    }
}

/// Rich setup presentation: team crest cards, stadium info, toss sequence.
fn team_picker_indices(screen: Screen, user_team: usize, team_count: usize) -> Vec<usize> {
    if screen == Screen::SetupOpp {
        (0..team_count).filter(|i| *i != user_team).collect()
    } else {
        (0..team_count).collect()
    }
}

fn spawn_toss_crest_row(
    parent: &mut ChildSpawnerCommands,
    assets: &AssetServer,
    fonts: &UiFonts,
    user: &crate::core::teams::Team,
    opp: &crate::core::teams::Team,
    scale: f32,
) {
    parent
        .spawn((Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            column_gap: theme::spx(20.0, scale),
            width: percent(100),
            ..default()
        },))
        .with_children(|versus| {
            spawn_toss_side(versus, assets, fonts, user, scale);
            versus.spawn((
                Text::new("VS"),
                TextFont {
                    font: fonts.display.clone(),
                    font_size: 30.0 * scale,
                    ..default()
                },
                TextColor(theme::palette::gold()),
                Node {
                    margin: UiRect::horizontal(theme::spx(8.0, scale)),
                    ..default()
                },
            ));
            spawn_toss_side(versus, assets, fonts, opp, scale);
        });
}

fn spawn_toss_panel<F>(parent: &mut ChildSpawnerCommands, scale: f32, build: F)
where
    F: FnOnce(&mut ChildSpawnerCommands),
{
    parent
        .spawn((Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceEvenly,
            flex_grow: 1.0,
            align_self: AlignSelf::Stretch,
            width: percent(100),
            padding: UiRect::vertical(theme::spx(12.0, scale)),
            row_gap: theme::spx(14.0, scale),
            ..default()
        },))
        .with_children(build);
}

fn spawn_setup_visuals(
    parent: &mut ChildSpawnerCommands,
    ms: &MenuState,
    wd: &WorldData,
    assets: &AssetServer,
    fonts: &UiFonts,
    scale: f32,
    ui_prefs: &UiPreferences,
) {
    let hc = ui_prefs.high_contrast;
    match ms.screen {
        Screen::SetupTeam | Screen::SetupOpp => {
            let indices = team_picker_indices(ms.screen, ms.team, wd.teams.len());
            parent
                .spawn((Node {
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    width: theme::spx(TEAM_GRID_WIDTH, scale),
                    column_gap: theme::spx(TEAM_GRID_GAP, scale),
                    row_gap: theme::spx(TEAM_GRID_GAP, scale),
                    justify_content: JustifyContent::Center,
                    margin: UiRect::bottom(theme::spx(8.0, scale)),
                    ..default()
                },))
                .with_children(|grid| {
                    for (sel_idx, team_idx) in indices.iter().enumerate() {
                        let team = &wd.teams[*team_idx];
                        let selected = sel_idx == ms.sel;
                        let crest = crate::render::load_team_crest(assets, &team.crest_asset());
                        spawn_setup_card(
                            grid,
                            fonts,
                            SetupCardStyle::Team {
                                scale,
                                high_contrast: hc,
                            },
                            &team.short.to_uppercase(),
                            Some(&team.name),
                            Some(crest),
                            Some(team.primary_color),
                            selected,
                        );
                    }
                });
        }
        Screen::SetupOvers => {
            parent
                .spawn((Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: theme::spx(16.0, scale),
                    margin: UiRect::bottom(theme::spx(10.0, scale)),
                    ..default()
                },))
                .with_children(|row| {
                    for (i, overs) in OVERS_CHOICES.iter().enumerate() {
                        spawn_setup_card(
                            row,
                            fonts,
                            SetupCardStyle::Overs { scale },
                            &format!("{overs}"),
                            Some("OVERS"),
                            None,
                            None,
                            i == ms.sel,
                        );
                    }
                });
        }
        Screen::SetupStadium => {
            let count = wd.stadiums.len() + 1;
            let vp_h = theme::scaled_px(STADIUM_LIST_VIEWPORT_HEIGHT, scale);
            parent
                .spawn((Node {
                    margin: UiRect::bottom(theme::spx(8.0, scale)),
                    ..default()
                },))
                .with_children(|wrap| {
                    spawn_scroll_viewport(
                        wrap,
                        fonts,
                        scale,
                        vp_h,
                        ms.sel,
                        count,
                        STADIUM_ROW_HEIGHT,
                        STADIUM_ROW_GAP,
                        theme::spx(560.0, scale),
                        |list| {
                            for i in 0..count {
                                let selected = i == ms.sel;
                                if i >= wd.stadiums.len() {
                                    spawn_stadium_row(
                                        list,
                                        fonts,
                                        scale,
                                        "Random Venue",
                                        StadiumRowMeta {
                                            city: Some("Surprise pick each match"),
                                            pitch: None,
                                            boundary_m: None,
                                        },
                                        selected,
                                    );
                                } else {
                                    let s = &wd.stadiums[i];
                                    let pitch_chip = format!("{} pitch", s.pitch.label());
                                    spawn_stadium_row(
                                        list,
                                        fonts,
                                        scale,
                                        &s.name,
                                        StadiumRowMeta {
                                            city: Some(&s.city),
                                            pitch: Some(&pitch_chip),
                                            boundary_m: Some(s.boundary_radius()),
                                        },
                                        selected,
                                    );
                                }
                            }
                        },
                    );
                });
        }
        Screen::SetupTossCall => {
            let user = &wd.teams[ms.team];
            let opp = &wd.teams[ms.opp];
            spawn_toss_panel(parent, scale, |call| {
                spawn_toss_crest_row(call, assets, fonts, user, opp, scale);
                call.spawn((
                    Text::new(format!("{} TO CALL", user.name.to_uppercase())),
                    TextFont {
                        font: fonts.bold.clone(),
                        font_size: 15.0 * scale,
                        ..default()
                    },
                    TextColor(theme::palette::text_muted()),
                ));
                call.spawn((Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: theme::spx(16.0, scale),
                    ..default()
                },))
                    .with_children(|row| {
                        for (i, label) in ["HEADS", "TAILS"].iter().enumerate() {
                            spawn_setup_card(
                                row,
                                fonts,
                                SetupCardStyle::Overs { scale },
                                label,
                                None,
                                None,
                                None,
                                i == ms.sel,
                            );
                        }
                    });
            });
        }
        Screen::SetupTossFlip => {
            let user = &wd.teams[ms.team];
            let opp = &wd.teams[ms.opp];
            spawn_toss_panel(parent, scale, |toss| {
                spawn_toss_crest_row(toss, assets, fonts, user, opp, scale);
                toss.spawn((Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: theme::spx(8.0, scale),
                    ..default()
                },))
                    .with_children(|coin_block| {
                        spawn_toss_coin(coin_block, fonts, scale, ms.coin_heads);
                    });
                toss.spawn((
                    Text::new("FLIPPING..."),
                    TextFont {
                        font: fonts.display.clone(),
                        font_size: 22.0 * scale,
                        ..default()
                    },
                    TextColor(theme::palette::gold()),
                ));
            });
        }
        Screen::SetupTossResult => {
            let user = &wd.teams[ms.team];
            let opp = &wd.teams[ms.opp];
            let winner = &wd.teams[ms.toss_winner];
            let coin_face = if ms.coin_heads { "HEADS" } else { "TAILS" };
            spawn_toss_panel(parent, scale, |toss| {
                spawn_toss_crest_row(toss, assets, fonts, user, opp, scale);
                toss.spawn((Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: theme::spx(10.0, scale),
                    ..default()
                },))
                    .with_children(|coin_block| {
                        spawn_toss_coin(coin_block, fonts, scale, ms.coin_heads);
                    });
                toss.spawn((
                    Text::new(format!(
                        "IT'S {coin_face} — {} WINS THE TOSS",
                        winner.name.to_uppercase()
                    )),
                    TextFont {
                        font: fonts.display.clone(),
                        font_size: 24.0 * scale,
                        ..default()
                    },
                    TextColor(theme::palette::gold()),
                    TextShadow {
                        offset: Vec2::new(0.0, 2.0),
                        color: Color::srgba(0.0, 0.0, 0.0, 0.55),
                    },
                ));
            });
        }
        Screen::SetupTossChoice => {
            let user_won = ms.toss_winner == ms.team;
            let user = &wd.teams[ms.team];
            let opp = &wd.teams[ms.opp];
            spawn_toss_panel(parent, scale, |choice| {
                spawn_toss_crest_row(choice, assets, fonts, user, opp, scale);
                let winner = &wd.teams[ms.toss_winner];
                let prompt = if user_won {
                    format!("{} WON — ELECT TO", winner.name.to_uppercase())
                } else {
                    format!("{} ELECTED TO", winner.name.to_uppercase())
                };
                choice.spawn((
                    Text::new(prompt),
                    TextFont {
                        font: fonts.bold.clone(),
                        font_size: 15.0 * scale,
                        ..default()
                    },
                    TextColor(theme::palette::text_muted()),
                ));
                choice
                    .spawn((Node {
                        flex_direction: FlexDirection::Row,
                        column_gap: theme::spx(16.0, scale),
                        ..default()
                    },))
                    .with_children(|row| {
                        for (i, label) in ["BAT", "BOWL"].iter().enumerate() {
                            let selected = if user_won {
                                i == ms.sel
                            } else {
                                i == usize::from(!ms.toss_elects_bat)
                            };
                            spawn_setup_card(
                                row,
                                fonts,
                                SetupCardStyle::Overs { scale },
                                label,
                                Some("FIRST"),
                                None,
                                None,
                                selected,
                            );
                        }
                    });
            });
        }
        Screen::SetupTossSummary => {
            let user = &wd.teams[ms.team];
            let opp = &wd.teams[ms.opp];
            let winner = &wd.teams[ms.toss_winner];
            let choice = if ms.toss_elects_bat { "BAT" } else { "BOWL" };
            spawn_toss_panel(parent, scale, |summary| {
                spawn_toss_crest_row(summary, assets, fonts, user, opp, scale);
                summary
                    .spawn((Node {
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        row_gap: theme::spx(10.0, scale),
                        ..default()
                    },))
                    .with_children(|coin_block| {
                        spawn_toss_coin(coin_block, fonts, scale, ms.coin_heads);
                    });
                summary.spawn((
                    Text::new(format!(
                        "{} WON THE TOSS AND ELECTED TO {}",
                        winner.name.to_uppercase(),
                        choice
                    )),
                    TextFont {
                        font: fonts.display.clone(),
                        font_size: 22.0 * scale,
                        ..default()
                    },
                    TextColor(Color::srgb(0.97, 0.98, 0.94)),
                    TextShadow {
                        offset: Vec2::new(0.0, 2.0),
                        color: Color::srgba(0.0, 0.0, 0.0, 0.55),
                    },
                ));
            });
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Input handling / navigation
// ---------------------------------------------------------------------------

fn wrap_selection(sel: &mut usize, delta: i32, max: usize) {
    *sel = ((*sel as i32 + delta).rem_euclid(max as i32)) as usize;
}

fn handle_main_menu_input(
    ms: &mut MenuState,
    input: &PlayerInput,
    exit: &mut MessageWriter<AppExit>,
) {
    if input.pressed(Action::Next) {
        wrap_selection(&mut ms.sel, 1, 4);
    }
    if input.pressed(Action::Prev) {
        wrap_selection(&mut ms.sel, -1, 4);
    }
    if input.pressed(Action::Confirm) {
        match ms.sel {
            0 => {
                ms.tournament_mode = false;
                ms.screen = Screen::SetupTeam;
                ms.sel = ms.team;
            }
            1 => {
                ms.tournament_mode = true;
                ms.screen = Screen::SetupTeam;
                ms.sel = ms.team;
            }
            2 => {
                ms.screen = Screen::Settings;
            }
            _ => {
                exit.write(AppExit::Success);
            }
        }
    }
}

fn handle_settings_input(
    ms: &mut MenuState,
    input: &PlayerInput,
    keys: &ButtonInput<KeyCode>,
    bindings: &mut KeyBindings,
    rebind: &mut RebindState,
    audio: &mut AudioSettings,
    ui_prefs: &mut UiPreferences,
    trans: &mut MenuTransition,
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
        ms.settings_tab = ms.settings_tab.next();
        ms.sel = 0;
    }
    if input.pressed(Action::CycleCam) {
        ms.settings_tab = ms.settings_tab.prev();
        ms.sel = 0;
    }

    let count = settings_item_count(ms.settings_tab);
    navigate_list(input, &mut ms.sel, count);

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
                        audio.commentary_volume = (audio.commentary_volume + delta).clamp(0.0, 1.0)
                    }
                    _ => {}
                }
            } else if ms.sel == 5 && (input.pressed(Action::Right) || input.pressed(Action::Left)) {
                let dir = if input.pressed(Action::Right) { 1 } else { -1 };
                let cur: i32 = match audio.commentary {
                    crate::game::audio::CommentaryVoice::Off => 0,
                    crate::game::audio::CommentaryVoice::Male => 1,
                    crate::game::audio::CommentaryVoice::Female => 2,
                };
                let next = (cur + dir).rem_euclid(3) as usize;
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
                ui_prefs.subtitle_scale = (ui_prefs.subtitle_scale + delta).clamp(0.8, 1.4);
                ui_prefs.save();
            }
        }
    }

    let back_idx = count - 1;
    if (ms.sel == back_idx && input.pressed(Action::Confirm)) || input.pressed(Action::Cancel) {
        back_to_main(ms);
        trigger_screen_transition(trans, Screen::Main);
    }
    false
}

fn handle_setup_team_input(
    ms: &mut MenuState,
    input: &PlayerInput,
    wd: &WorldData,
    ct: &mut CurrentTournament,
) {
    navigate_grid(input, &mut ms.sel, wd.teams.len(), TEAM_GRID_COLUMNS);
    if input.pressed(Action::Confirm) {
        ms.team = ms.sel;
        if ms.tournament_mode {
            let t = start_tournament(ms.team, wd);
            ct.0 = Some(t);
            ms.screen = Screen::Bracket;
            ms.sel = 0;
        } else {
            if ms.opp == ms.team {
                ms.opp = (ms.team + 1) % wd.teams.len();
            }
            ms.screen = Screen::SetupOpp;
            let indices = team_picker_indices(Screen::SetupOpp, ms.team, wd.teams.len());
            ms.sel = indices.iter().position(|&i| i == ms.opp).unwrap_or(0);
        }
    }
    if input.pressed(Action::Cancel) {
        back_to_main(ms);
    }
}

fn handle_setup_opp_input(ms: &mut MenuState, input: &PlayerInput, wd: &WorldData) {
    let indices = team_picker_indices(Screen::SetupOpp, ms.team, wd.teams.len());
    navigate_grid(input, &mut ms.sel, indices.len(), TEAM_GRID_COLUMNS);
    if input.pressed(Action::Confirm) {
        ms.opp = indices[ms.sel];
        ms.screen = Screen::SetupOvers;
        ms.sel = ms.overs_idx;
    }
    if input.pressed(Action::Cancel) {
        ms.screen = Screen::SetupTeam;
        ms.sel = ms.team;
    }
}

fn handle_setup_overs_input(ms: &mut MenuState, input: &PlayerInput, wd: &WorldData) {
    navigate_horizontal_row(input, &mut ms.sel, OVERS_CHOICES.len());
    if input.pressed(Action::Confirm) {
        ms.overs_idx = ms.sel;
        ms.screen = Screen::SetupStadium;
        ms.sel = ms.stadium_idx.min(wd.stadiums.len());
    }
    if input.pressed(Action::Cancel) {
        ms.screen = Screen::SetupTeam;
        ms.sel = ms.team;
    }
}

fn handle_setup_stadium_input(ms: &mut MenuState, input: &PlayerInput, wd: &WorldData) {
    navigate_vertical_list(input, &mut ms.sel, wd.stadiums.len() + 1);
    if input.pressed(Action::Confirm) {
        ms.stadium_idx = if ms.sel >= wd.stadiums.len() {
            usize::MAX
        } else {
            ms.sel
        };
        begin_toss(ms);
    }
    if input.pressed(Action::Cancel) {
        ms.screen = Screen::SetupOvers;
        ms.sel = ms.overs_idx;
    }
}

fn stadium_menu_sel(stadium_idx: usize, stadium_count: usize) -> usize {
    if stadium_idx == usize::MAX {
        stadium_count
    } else {
        stadium_idx
    }
}

fn begin_toss(ms: &mut MenuState) {
    ms.toss_call_heads = true;
    ms.toss_elects_bat = false;
    ms.screen = Screen::SetupTossCall;
    ms.sel = 0;
}

/// The calling side wins the toss when the coin lands on the face they called.
pub fn toss_winner_from_call(
    user_team: usize,
    opp_team: usize,
    call_heads: bool,
    coin_heads: bool,
) -> usize {
    if call_heads == coin_heads {
        user_team
    } else {
        opp_team
    }
}

fn handle_setup_toss_call_input(ms: &mut MenuState, input: &PlayerInput, wd: &WorldData) {
    navigate_horizontal_row(input, &mut ms.sel, 2);
    if input.pressed(Action::Confirm) {
        ms.toss_call_heads = ms.sel == 0;
        ms.coin_heads = rand::random::<bool>();
        ms.toss_winner = toss_winner_from_call(ms.team, ms.opp, ms.toss_call_heads, ms.coin_heads);
        ms.screen = Screen::SetupTossFlip;
    }
    if input.pressed(Action::Cancel) {
        ms.screen = Screen::SetupStadium;
        ms.sel = stadium_menu_sel(ms.stadium_idx, wd.stadiums.len());
    }
}

fn handle_setup_toss_choice_input(ms: &mut MenuState, input: &PlayerInput, wd: &WorldData) {
    let user_won = ms.toss_winner == ms.team;
    if user_won {
        navigate_horizontal_row(input, &mut ms.sel, 2);
        if input.pressed(Action::Confirm) {
            ms.toss_elects_bat = ms.sel == 0;
            ms.bat_first = user_bats_first_from_toss(ms.team, ms.toss_winner, ms.toss_elects_bat);
            ms.screen = Screen::SetupTossSummary;
        }
    } else if input.pressed(Action::Confirm) {
        ms.screen = Screen::SetupTossSummary;
    }
    if input.pressed(Action::Cancel) {
        ms.screen = Screen::SetupStadium;
        ms.sel = stadium_menu_sel(ms.stadium_idx, wd.stadiums.len());
    }
}

fn handle_setup_toss_summary_input(
    ms: &mut MenuState,
    input: &PlayerInput,
    wd: &WorldData,
    commands: &mut Commands,
    next_state: &mut NextState<AppState>,
) {
    if input.pressed(Action::Confirm) {
        start_quick_match(ms, wd, commands, next_state);
    }
    if input.pressed(Action::Cancel) {
        ms.screen = Screen::SetupStadium;
        ms.sel = stadium_menu_sel(ms.stadium_idx, wd.stadiums.len());
    }
}

fn handle_setup_toss_back(ms: &mut MenuState, input: &PlayerInput, wd: &WorldData) {
    if input.pressed(Action::Cancel) {
        ms.screen = Screen::SetupStadium;
        ms.sel = stadium_menu_sel(ms.stadium_idx, wd.stadiums.len());
    }
}

fn handle_bracket_input(
    ms: &mut MenuState,
    input: &PlayerInput,
    wd: &WorldData,
    ct: &mut CurrentTournament,
    af: &mut ActiveFixture,
    commands: &mut Commands,
    next_state: &mut NextState<AppState>,
) {
    auto_advance_bracket_draw(ms, ct);
    if input.pressed(Action::Cancel) {
        back_to_main(ms);
        ct.0 = None;
        return;
    }
    let Some(t) = ct.0.as_ref() else {
        back_to_main(ms);
        return;
    };
    if input.pressed(Action::Confirm) {
        if let Some(champ) = t.champion() {
            info!("Champions: {}", t.teams[champ].name);
            back_to_main(ms);
            ct.0 = None;
        } else if let Some((idx, f)) = t.next_user_fixture() {
            launch_tournament_match(t, idx, &f, wd, af, commands, next_state);
        }
    }
}

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
    let _count = screen_item_count(&ms, &wd);

    match ms.screen {
        Screen::Main => handle_main_menu_input(&mut ms, &input, &mut exit),
        Screen::Settings => {
            handle_settings_input(
                &mut ms,
                &input,
                &keys,
                &mut bindings,
                &mut rebind,
                &mut audio,
                &mut ui_prefs,
                &mut trans,
            );
        }
        Screen::SetupTeam => handle_setup_team_input(&mut ms, &input, &wd, &mut ct),
        Screen::SetupOpp => handle_setup_opp_input(&mut ms, &input, &wd),
        Screen::SetupOvers => handle_setup_overs_input(&mut ms, &input, &wd),
        Screen::SetupStadium => handle_setup_stadium_input(&mut ms, &input, &wd),
        Screen::SetupTossCall => handle_setup_toss_call_input(&mut ms, &input, &wd),
        Screen::SetupTossFlip | Screen::SetupTossResult => {
            handle_setup_toss_back(&mut ms, &input, &wd);
        }
        Screen::SetupTossChoice => handle_setup_toss_choice_input(&mut ms, &input, &wd),
        Screen::SetupTossSummary => {
            handle_setup_toss_summary_input(&mut ms, &input, &wd, &mut commands, &mut next_state)
        }
        Screen::Bracket => handle_bracket_input(
            &mut ms,
            &input,
            &wd,
            &mut ct,
            &mut af,
            &mut commands,
            &mut next_state,
        ),
    }
}

fn grid_row_len(count: usize, cols: usize, row: usize) -> usize {
    let start = row * cols;
    if start >= count {
        return 0;
    }
    cols.min(count - start)
}

fn navigate_grid(input: &PlayerInput, sel: &mut usize, count: usize, cols: usize) {
    if count == 0 {
        return;
    }
    let cols = cols.max(1);
    let rows = count.div_ceil(cols);
    let row = *sel / cols;
    let col = *sel % cols;

    if input.pressed(Action::Right) {
        let len = grid_row_len(count, cols, row);
        if len > 0 {
            let new_col = (col + 1) % len;
            *sel = row * cols + new_col;
        }
    }
    if input.pressed(Action::Left) {
        let len = grid_row_len(count, cols, row);
        if len > 0 {
            let new_col = (col + len - 1) % len;
            *sel = row * cols + new_col;
        }
    }
    if input.pressed(Action::Next) {
        let new_row = (row + 1) % rows;
        let len = grid_row_len(count, cols, new_row);
        let new_col = col.min(len.saturating_sub(1));
        *sel = new_row * cols + new_col;
    }
    if input.pressed(Action::Prev) {
        let new_row = (row + rows - 1) % rows;
        let len = grid_row_len(count, cols, new_row);
        let new_col = col.min(len.saturating_sub(1));
        *sel = new_row * cols + new_col;
    }
}

fn navigate_horizontal_row(input: &PlayerInput, sel: &mut usize, count: usize) {
    navigate_grid(input, sel, count, count.max(1));
}

fn navigate_vertical_list(input: &PlayerInput, sel: &mut usize, count: usize) {
    navigate_list(input, sel, count);
}

fn navigate_list(input: &PlayerInput, sel: &mut usize, max: usize) {
    if max == 0 {
        return;
    }
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
        Screen::SetupTossCall | Screen::SetupTossChoice => 2,
        Screen::SetupTossFlip | Screen::SetupTossResult | Screen::SetupTossSummary => 0,
        Screen::Settings => settings_item_count(ms.settings_tab),
        Screen::Bracket => 0,
    }
}

/// Return the wizard to the main menu. Also used by the in-match pause
/// overlay, which leaves the setup screens behind when the player quits.
pub fn back_to_main(ms: &mut MenuState) {
    ms.screen = Screen::Main;
    ms.sel = 0;
}

/// Create the tournament when the user picks a team from the main menu
/// with Tournament selected (handled here to keep state machine simple).
pub fn start_tournament(user_world: usize, wd: &WorldData) -> Tournament {
    // User's team + three others chosen round-robin.
    let others: Vec<usize> = (0..wd.teams.len())
        .filter(|&i| i != user_world)
        .cycle()
        .skip(user_world % wd.teams.len().max(1))
        .take(3)
        .collect();
    let mut entrants: Vec<Entrant> = std::iter::once(user_world)
        .chain(others)
        .map(|w| Entrant {
            world_idx: w,
            team: wd.teams[w].clone(),
        })
        .collect();
    // Find the local slot of the user AFTER sorting happens inside knockout:
    let user_name = wd.teams[user_world].name.clone();
    let stadiums = crate::core::stadiums::builtin_stadiums();
    // Pre-seed so we can find the local index post-sort.
    entrants.sort_by_key(|e| (crate::core::teams::team_rating(&e.team) * 10.0) as i64);
    let user_local = entrants.iter().position(|e| e.team.name == user_name);
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
    // Smoke tests pick a venue randomly, which makes it impossible to capture a
    // named ground on purpose. CRICKET_STADIUM pins one by index.
    let stadium = forced_stadium_index(wd.stadiums.len()).unwrap_or(stadium);
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

/// Venue override for automated captures: `CRICKET_STADIUM=<index>`.
/// Out-of-range or unparseable values are ignored.
fn forced_stadium_index(count: usize) -> Option<usize> {
    std::env::var("CRICKET_STADIUM")
        .ok()?
        .trim()
        .parse::<usize>()
        .ok()
        .filter(|idx| *idx < count)
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
    let Some(user_local) = t.user_team else {
        return;
    };
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
    let PhaseEnum::MatchOver = phase.0 else {
        return;
    };
    if !input.pressed(Action::Confirm) {
        return;
    }

    if ms.screen != Screen::Bracket && af.0.is_none() {
        // Quick match: straight back to the main menu.
        cleanup_after_match(&mut commands, &mut af);
        next_state.set(AppState::Menu);
        return;
    }

    if let (Some(t), Some(idx), Some(am)) = (ct.0.as_mut(), af.0, am.as_deref()) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn nav_grid(sel: usize, count: usize, cols: usize, action: Action) -> usize {
        let mut sel = sel;
        let input = PlayerInput {
            just_pressed: vec![action],
            ..Default::default()
        };
        navigate_grid(&input, &mut sel, count, cols);
        sel
    }

    #[test]
    fn grid_right_moves_within_row() {
        assert_eq!(nav_grid(0, 10, TEAM_GRID_COLUMNS, Action::Right), 1);
        assert_eq!(nav_grid(3, 10, TEAM_GRID_COLUMNS, Action::Right), 0);
    }

    #[test]
    fn grid_down_moves_one_row() {
        assert_eq!(nav_grid(1, 10, TEAM_GRID_COLUMNS, Action::Next), 5);
        assert_eq!(nav_grid(5, 10, TEAM_GRID_COLUMNS, Action::Prev), 1);
    }

    #[test]
    fn grid_wraps_partial_last_row() {
        assert_eq!(nav_grid(8, 10, TEAM_GRID_COLUMNS, Action::Right), 9);
        assert_eq!(nav_grid(9, 10, TEAM_GRID_COLUMNS, Action::Right), 8);
    }

    #[test]
    fn opposition_picker_indices_skip_user_team() {
        let indices = team_picker_indices(Screen::SetupOpp, 3, 10);
        assert_eq!(indices.len(), 9);
        assert!(!indices.contains(&3));
        assert_eq!(indices[3], 4);
    }

    #[test]
    fn opposition_selection_matches_compacted_index() {
        let indices = team_picker_indices(Screen::SetupOpp, 3, 10);
        let sel = 4;
        assert_eq!(indices[sel], 5);
    }

    #[test]
    fn toss_call_matching_coin_gives_user_win() {
        assert_eq!(toss_winner_from_call(1, 4, true, true), 1);
        assert_eq!(toss_winner_from_call(1, 4, false, false), 1);
    }

    #[test]
    fn toss_call_mismatch_gives_opp_win() {
        assert_eq!(toss_winner_from_call(1, 4, true, false), 4);
        assert_eq!(toss_winner_from_call(1, 4, false, true), 4);
    }

    #[test]
    fn toss_election_maps_to_user_bats_first() {
        assert!(user_bats_first_from_toss(1, 1, true));
        assert!(!user_bats_first_from_toss(1, 1, false));
        assert!(!user_bats_first_from_toss(1, 4, true));
        assert!(user_bats_first_from_toss(1, 4, false));
    }

    fn signature_from_world(world: &World) -> MenuContentSignature {
        menu_content_signature(
            world.resource::<MenuState>(),
            world.resource::<CurrentTournament>(),
            world.resource::<KeyBindings>(),
            world.resource::<AudioSettings>(),
            world.resource::<RebindState>(),
            world.resource::<UiPreferences>(),
            world.resource::<UiScale>(),
        )
    }

    fn insert_signature_resources(world: &mut World) {
        world.insert_resource(MenuState::default());
        world.insert_resource(CurrentTournament::default());
        world.insert_resource(KeyBindings::default());
        world.insert_resource(AudioSettings::default());
        world.insert_resource(RebindState::default());
        world.insert_resource(UiPreferences::default());
        world.insert_resource(UiScale::default());
    }

    fn noop_menu_input(ms: &mut MenuState) {
        let _ = ms.screen;
    }

    #[test]
    fn menu_signature_unchanged_after_resmut_deref_without_write() {
        let mut world = World::new();
        insert_signature_resources(&mut world);
        world.clear_trackers();

        let before = signature_from_world(&world);

        {
            let mut ms = world.resource_mut::<MenuState>();
            noop_menu_input(&mut ms);
        }

        let after = signature_from_world(&world);
        assert_eq!(before, after);
        assert!(
            world.is_resource_changed::<MenuState>(),
            "ResMut deref should still mark MenuState changed"
        );
        let rebuild = MenuRebuildState {
            signature: Some(before),
        };
        assert!(!menu_content_dirty(&rebuild, &after));
    }

    #[test]
    fn menu_signature_changes_when_sel_changes() {
        let mut world = World::new();
        insert_signature_resources(&mut world);
        let before = signature_from_world(&world);

        world.resource_mut::<MenuState>().sel = 2;
        let after = signature_from_world(&world);

        assert_ne!(before, after);
    }

    #[test]
    fn menu_signature_changes_when_audio_volume_changes() {
        let mut world = World::new();
        insert_signature_resources(&mut world);
        let before = signature_from_world(&world);

        world.resource_mut::<AudioSettings>().master = 0.42;
        let after = signature_from_world(&world);

        assert_ne!(before, after);
    }

    #[test]
    fn menu_signature_ignores_anim_timer() {
        let mut world = World::new();
        insert_signature_resources(&mut world);
        world.resource_mut::<MenuState>().screen = Screen::SetupTossFlip;
        let before = signature_from_world(&world);

        world.insert_resource(MenuAnimTime(0.25));
        let mid_flip = signature_from_world(&world);
        world.insert_resource(MenuAnimTime(1.75));
        let late_flip = signature_from_world(&world);

        assert_eq!(before, mid_flip);
        assert_eq!(before, late_flip);
    }

    #[test]
    fn list_scroll_offset_keeps_selected_row_visible() {
        let row_h = 100.0;
        let gap = 8.0;
        let vp = 320.0;
        let count = 5;

        for sel in 0..count {
            let offset = theme::list_scroll_offset(sel, row_h, gap, vp, count);
            let stride = row_h + gap;
            let row_top = sel as f32 * stride;
            let row_bottom = row_top + row_h;
            assert!(
                row_top + 0.01 >= offset,
                "sel {sel}: row top {row_top} above offset {offset}"
            );
            assert!(
                row_bottom <= offset + vp + 0.01,
                "sel {sel}: row bottom {row_bottom} below viewport end {}",
                offset + vp
            );
        }
    }
}

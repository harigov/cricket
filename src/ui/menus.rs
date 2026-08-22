//! Menus: main menu, match-setup wizard, controls help and the tournament
//! bracket screen. Keyboard/gamepad driven (see input::Action mapping).

use crate::core::tournament::{Entrant, Fixture, Stage, Tournament};
use crate::game::*;
use crate::input::{Action, PlayerInput};
use crate::state::AppState;
use bevy::prelude::*;

const OVERS_CHOICES: [u32; 3] = [5, 10, 20];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Main,
    SetupTeam,
    SetupOpp,
    SetupOvers,
    SetupStadium,
    SetupBatFirst,
    Controls,
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

pub struct MenusPlugin;

impl Plugin for MenusPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MenuState>()
            .init_resource::<CurrentTournament>()
            .init_resource::<ActiveFixture>()
            .add_systems(OnEnter(AppState::Menu), spawn_menu_root)
            .add_systems(OnExit(AppState::Menu), despawn_menu_root)
            .add_systems(
                Update,
                (refresh_menu, handle_menu_input, handle_match_exit)
                    .run_if(in_state(AppState::Menu)),
            );
    }
}

// ---------------------------------------------------------------------------
// UI construction (immediate-mode style rebuild each frame)
// ---------------------------------------------------------------------------

fn spawn_menu_root(mut commands: Commands) {
    info!("MENU ROOT SPAWNED");
    commands.spawn((
        MenuRoot,
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            row_gap: px(18),
            ..default()
        },
        BackgroundColor(Color::srgb(0.03, 0.05, 0.04)),
    )).with_children(|p| {
        p.spawn((
            MenuList,
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(10),
                ..default()
            },
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
        Screen::Controls => "CONTROLS",
        Screen::Bracket => "TOURNAMENT BRACKET",
    }
}

fn screen_items(
    ms: &MenuState,
    wd: &WorldData,
    ct: &CurrentTournament,
) -> Vec<String> {
    match ms.screen {
        Screen::Main => vec![
            "Quick Match".into(),
            "Tournament".into(),
            "Controls".into(),
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
        Screen::Controls => CONTROLS_LINES.iter().map(|s| s.to_string()).collect(),
        Screen::Bracket => bracket_lines(ct),
    }
}

const CONTROLS_LINES: &[&str] = &[
    "BATTING:",
    "  SPACE / A ......... play shot (time it as the ball arrives)",
    "  hold SHIFT / LT ... loft the shot (risky)",
    "  ← → / stick ....... aim leg side or off side",
    "",
    "BOWLING:",
    "  SPACE / A ......... lock length, then lock line",
    "",
    "GENERAL:",
    "  W/S or ↑↓ ......... navigate menus",
    "  ESC / B ........... back",
];

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
    root_q: Query<Entity, With<MenuList>>,
    children_q: Query<&Children>,
    mut commands: Commands,
) {
    let Ok(root) = root_q.single() else { return };

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
            Text::new(screen_title(&ms)),
            TextFont { font_size: 42.0, ..default() },
            TextColor(Color::srgb(0.55, 0.9, 0.45)),
        ));
        p.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(8),
                margin: UiRect::vertical(px(14)),
                ..default()
            },
        )).with_children(|items| {
            for (i, line) in
                screen_items(&ms, &wd, &ct).into_iter().enumerate()
            {
                let selectable =
                    !matches!(ms.screen, Screen::Bracket | Screen::Controls);
                let selected = i == ms.sel && selectable;
                items.spawn((
                    Node {
                        padding: UiRect::horizontal(px(18)),
                        ..default()
                    },
                    BackgroundColor(if selected {
                        Color::srgba(0.25, 0.5, 0.2, 0.5)
                    } else {
                        Color::NONE
                    }),
                )).with_children(|row| {
                    row.spawn((
                        Text::new(line),
                        TextFont { font_size: 22.0, ..default() },
                        TextColor(if selected {
                            Color::WHITE
                        } else {
                            Color::srgb(0.75, 0.78, 0.75)
                        }),
                    ));
                });
            }
        });
    });
}

// ---------------------------------------------------------------------------
// Input handling / navigation
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn handle_menu_input(
    mut ms: ResMut<MenuState>,
    input: Res<PlayerInput>,
    wd: Res<WorldData>,
    mut ct: ResMut<CurrentTournament>,
    mut af: ResMut<ActiveFixture>,
    mut next_state: ResMut<NextState<AppState>>,
    mut exit: MessageWriter<AppExit>,
    mut commands: Commands,
) {
    use Screen::*;
    let count = screen_item_count(&ms, &wd);
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
                    2 => { ms.screen = Controls; }
                    _ => {
                        exit.write(AppExit::Success);
                    }
                }
            }
        }
        Controls => {
            if input.pressed(Action::Confirm) || input.pressed(Action::Cancel) {
                ms.screen = Main;
                ms.sel = 0;
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
        _ => {}
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
        Screen::Controls | Screen::Bracket => 0,
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

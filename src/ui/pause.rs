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

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
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
            .add_systems(
                OnExit(AppState::InMatch),
                (despawn_pause_ui, reset_pause_flag),
            )
            .add_systems(
                Update,
                // Chained (not an unordered tuple) so each frame is
                // deterministic: read input -> mutate pause/menu state ->
                // spawn/despawn the overlay -> redraw its rows. See
                // `toggle_pause` and `sync_pause_overlay` for why the order
                // specifically matters here.
                (
                    toggle_pause,
                    handle_pause_input,
                    sync_pause_overlay,
                    refresh_pause_ui,
                )
                    .chain()
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
        // Esc only resumes from the root page; sub-pages handle their own
        // back. This reads `menu.screen` as it was at the *start* of the
        // frame (this system runs first in the chain) so an Esc that
        // `handle_pause_input` uses to step back out of Settings can never
        // also be seen as "Esc from Root" and unpause in the same frame.
        if menu.screen == PauseScreen::Root {
            paused.0 = false;
        }
    } else {
        paused.0 = true;
    }
}

/// Spawn the overlay entity tree. Callers must ensure it isn't already
/// present (see `sync_pause_overlay`).
fn spawn_pause_root(commands: &mut Commands, assets: &AssetServer) {
    let fonts = UiFonts::load(assets);
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
                    min_width: px(560.0),
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

/// Keep the overlay entity in sync with `MatchPaused`, whichever upstream
/// system flipped it this frame (`toggle_pause` on Esc, or a Confirm on the
/// "Resume" row inside `handle_pause_input`). This is what actually fixes
/// the "settings menu stays around after resuming" bug: previously nothing
/// ever despawned `PauseRoot` except leaving `AppState::InMatch` entirely,
/// so unpausing mid-match left the overlay on screen forever.
///
/// Runs last-but-one in the chain (before `refresh_pause_ui`) so it always
/// observes the frame's final `paused.0`, regardless of which system in
/// this chain changed it.
fn sync_pause_overlay(
    mut commands: Commands,
    paused: Res<MatchPaused>,
    assets: Res<AssetServer>,
    roots: Query<Entity, With<PauseRoot>>,
    mut menu: ResMut<PauseMenuState>,
    mut rebind: ResMut<RebindState>,
) {
    if paused.0 {
        if roots.is_empty() {
            spawn_pause_root(&mut commands, &assets);
        }
        return;
    }

    // Not paused (any longer): drop the overlay if it's still there, and
    // reset the menu so the *next* time the player opens Esc they land on
    // the root page rather than wherever they last left Settings.
    for e in &roots {
        commands.entity(e).despawn();
    }
    reset_pause_state(&mut menu, &mut rebind);
}

/// Reset menu navigation back to the root page and clear any in-flight key
/// rebind. Pulled out of `sync_pause_overlay` so it can be unit tested
/// without needing a full `App` (and an `AssetServer`, which the system
/// also requires but this reset logic doesn't touch).
fn reset_pause_state(menu: &mut PauseMenuState, rebind: &mut RebindState) {
    *menu = PauseMenuState::default();
    rebind.0 = None;
}

/// One renderable row of the pause / settings menu.
///
/// This replaced a `Vec<String>` built with manual `format!` space padding
/// (`format!("{label:16} : {key_str}")`) — that only lines up in a
/// monospace font, and this UI uses a proportional one. Each variant here
/// maps to a real flexbox row in `spawn_pause_row` instead of padded text.
enum PauseRow {
    /// Full-width selectable action button (Resume, Back, Reset…).
    Action(String),
    /// The Audio / Controls / Camera tab strip.
    Tabs(SettingsTab),
    /// Non-interactive info strip. Still occupies a selectable slot so
    /// ←/→ can switch tabs from it (see `settings_tab_switch_row`).
    Hint(String),
    /// label | value chip | optional hint, laid out as a grid row.
    KeyValue {
        label: String,
        value: String,
        hint: Option<String>,
        /// Highlight the value chip (a rebind is in progress for this row).
        active: bool,
    },
}

fn pause_rows(
    menu: &PauseMenuState,
    bindings: &KeyBindings,
    audio: &AudioSettings,
    rebind: &RebindState,
    ui: &UiPreferences,
    rig: &CameraRig,
) -> Vec<PauseRow> {
    match menu.screen {
        PauseScreen::Root => vec![
            PauseRow::Action("Resume".into()),
            PauseRow::Action("Settings".into()),
            PauseRow::Action("Controls".into()),
            PauseRow::Action("Quit to Main Menu".into()),
        ],
        PauseScreen::Settings => {
            // Row order/index here MUST match `settings_item_count` and the
            // `menu.sel` arithmetic in `handle_settings_input` (tab strip is
            // always row 0; Controls additionally reserves row 1 for the
            // rebind hint).
            let mut out = vec![PauseRow::Tabs(menu.settings_tab)];
            match menu.settings_tab {
                SettingsTab::Audio => {
                    out.push(PauseRow::KeyValue {
                        label: "Master Volume".into(),
                        value: format!("{:>3}%", (audio.master * 100.0) as i32),
                        hint: Some("←/→ adjust".into()),
                        active: false,
                    });
                    out.push(PauseRow::KeyValue {
                        label: "SFX Volume".into(),
                        value: format!("{:>3}%", (audio.sfx * 100.0) as i32),
                        hint: Some("←/→ adjust".into()),
                        active: false,
                    });
                    out.push(PauseRow::KeyValue {
                        label: "Music Volume".into(),
                        value: format!("{:>3}%", (audio.music * 100.0) as i32),
                        hint: Some("←/→ adjust".into()),
                        active: false,
                    });
                    out.push(PauseRow::KeyValue {
                        label: "Commentary Vol".into(),
                        value: format!("{:>3}%", (audio.commentary_volume * 100.0) as i32),
                        hint: Some("←/→ adjust".into()),
                        active: false,
                    });
                    let comm_label = match audio.commentary {
                        CommentaryVoice::Off => "Off",
                        CommentaryVoice::Male => "Ryan (M lead)",
                        CommentaryVoice::Female => "Natasha (F lead)",
                    };
                    out.push(PauseRow::KeyValue {
                        label: "Commentary Voice".into(),
                        value: comm_label.into(),
                        hint: Some("←/→ cycle".into()),
                        active: false,
                    });
                }
                SettingsTab::Controls => {
                    out.push(PauseRow::Hint(
                        "SPACE rebind selected   ·   ←/→ or Q/C switch tab".into(),
                    ));
                    for (action, label) in SETTINGS_ACTIONS {
                        let row_idx = out.len();
                        let rebinding = rebind.0 == Some(*action);
                        let value = if rebinding {
                            "Press any key...".to_string()
                        } else {
                            bindings
                                .map
                                .get(action)
                                .map(|k| key_label(*k))
                                .unwrap_or_else(|| "-".into())
                        };
                        // Only surface the rebind hint on the selected row,
                        // and not while it's already mid-rebind (the chip
                        // text itself says "Press any key..." then).
                        let hint = (!rebinding && row_idx == menu.sel)
                            .then(|| "Press Space to rebind".to_string());
                        out.push(PauseRow::KeyValue {
                            label: (*label).into(),
                            value,
                            hint,
                            active: rebinding,
                        });
                    }
                    out.push(PauseRow::Action("Reset controls to defaults".into()));
                }
                SettingsTab::Camera => {
                    out.push(PauseRow::KeyValue {
                        label: "Play camera".into(),
                        value: cam_mode_label(rig.mode).into(),
                        hint: Some("←/→ cycle".into()),
                        active: false,
                    });
                    out.push(PauseRow::KeyValue {
                        label: "UI Scale".into(),
                        value: format!("{:>4.0}%", ui.ui_scale * 100.0),
                        hint: Some("←/→ adjust".into()),
                        active: false,
                    });
                    out.push(PauseRow::KeyValue {
                        label: "High Contrast".into(),
                        value: if ui.high_contrast { "On" } else { "Off" }.into(),
                        hint: None,
                        active: false,
                    });
                }
            }
            out.push(PauseRow::Action("Back".into()));
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

/// Row-level selection chrome (background/border), shared by every row
/// variant so the highlight look stays identical to the old single-Text
/// rows.
fn row_container_node(scale: f32, selected: bool) -> impl Bundle {
    (
        Node {
            width: percent(100),
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            padding: UiRect::axes(theme::spx(10.0, scale), theme::spx(5.0, scale)),
            column_gap: theme::spx(12.0, scale),
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
    )
}

fn spawn_tab_strip(
    parent: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    scale: f32,
    active: SettingsTab,
) {
    const TABS: [SettingsTab; 3] = [
        SettingsTab::Audio,
        SettingsTab::Controls,
        SettingsTab::Camera,
    ];
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: theme::spx(8.0, scale),
            ..default()
        })
        .with_children(|tabs| {
            for tab in TABS {
                let is_active = tab == active;
                tabs.spawn((
                    Node {
                        padding: UiRect::axes(theme::spx(12.0, scale), theme::spx(4.0, scale)),
                        border_radius: BorderRadius::all(theme::spx(4.0, scale)),
                        ..default()
                    },
                    BackgroundColor(if is_active {
                        theme::palette::selection_bg()
                    } else {
                        theme::palette::chip_bg()
                    }),
                ))
                .with_children(|chip| {
                    chip.spawn((
                        Text::new(tab.label()),
                        TextFont {
                            font: fonts.bold.clone(),
                            font_size: 15.0 * scale,
                            ..default()
                        },
                        TextColor(if is_active {
                            theme::palette::gold()
                        } else {
                            theme::palette::text_muted()
                        }),
                    ));
                });
            }
            tabs.spawn((
                Text::new("(←/→ or Q/C switch tab)"),
                TextFont {
                    font: fonts.regular.clone(),
                    font_size: 12.0 * scale,
                    ..default()
                },
                TextColor(theme::palette::text_dim()),
            ));
        });
}

/// A keycap-styled chip for a bound key ("Column 2" of the grid).
fn spawn_key_chip(
    parent: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    scale: f32,
    text: &str,
    active: bool,
) {
    parent
        .spawn((
            Node {
                min_width: theme::spx(120.0, scale),
                padding: UiRect::axes(theme::spx(10.0, scale), theme::spx(4.0, scale)),
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(theme::spx(4.0, scale)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(if active {
                theme::palette::selection_bg()
            } else {
                theme::palette::chip_bg()
            }),
            BorderColor::all(if active {
                theme::palette::selection_border()
            } else {
                theme::palette::card_border()
            }),
        ))
        .with_children(|chip| {
            chip.spawn((
                Text::new(text),
                TextFont {
                    font: fonts.bold.clone(),
                    font_size: 14.0 * scale,
                    ..default()
                },
                TextColor(if active {
                    theme::palette::gold()
                } else {
                    theme::palette::text_primary()
                }),
            ));
        });
}

fn spawn_key_value_row(
    parent: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    scale: f32,
    label: &str,
    value: &str,
    hint: Option<&str>,
    active: bool,
    text_color: Color,
) {
    // Column 1: fixed-width label so every chip in the tab lines up,
    // regardless of proportional-font label width.
    parent.spawn((
        Text::new(label),
        TextFont {
            font: fonts.regular.clone(),
            font_size: 16.0 * scale,
            ..default()
        },
        TextColor(text_color),
        Node {
            width: theme::spx(190.0, scale),
            flex_shrink: 0.0,
            ..default()
        },
    ));
    // Column 2: the bound key / current value, as a keycap chip.
    spawn_key_chip(parent, fonts, scale, value, active);
    // Column 3 (optional): a hint, e.g. "Press Space to rebind".
    if let Some(hint) = hint {
        parent.spawn((
            Text::new(hint),
            TextFont {
                font: fonts.regular.clone(),
                font_size: 12.0 * scale,
                ..default()
            },
            TextColor(theme::palette::text_dim()),
            Node {
                flex_grow: 1.0,
                justify_content: JustifyContent::FlexEnd,
                ..default()
            },
        ));
    }
}

fn spawn_pause_row(
    parent: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    scale: f32,
    row: &PauseRow,
    selected: bool,
) {
    let text_color = if selected {
        theme::palette::text_primary()
    } else {
        theme::palette::text_muted()
    };
    parent
        .spawn(row_container_node(scale, selected))
        .with_children(|cell| match row {
            PauseRow::Action(text) => {
                cell.spawn((
                    Text::new(text.as_str()),
                    TextFont {
                        font: fonts.regular.clone(),
                        font_size: 18.0 * scale,
                        ..default()
                    },
                    TextColor(text_color),
                ));
            }
            PauseRow::Hint(text) => {
                cell.spawn((
                    Text::new(text.as_str()),
                    TextFont {
                        font: fonts.regular.clone(),
                        font_size: 14.0 * scale,
                        ..default()
                    },
                    TextColor(theme::palette::text_dim()),
                ));
            }
            PauseRow::Tabs(active) => spawn_tab_strip(cell, fonts, scale, *active),
            PauseRow::KeyValue {
                label,
                value,
                hint,
                active,
            } => spawn_key_value_row(
                cell,
                fonts,
                scale,
                label,
                value,
                hint.as_deref(),
                *active,
                text_color,
            ),
        });
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
    list_q: Query<Entity, With<PauseList>>,
    fonts_q: Query<&UiFonts, With<PauseRoot>>,
    children_q: Query<&Children>,
    mut commands: Commands,
) {
    if !paused.0 {
        return;
    }
    let Ok(list) = list_q.single() else {
        return;
    };
    let Ok(fonts) = fonts_q.single() else {
        return;
    };
    clear_pause_rows(&mut commands, list, &children_q);
    let scale = ui_scale.0;
    let rows = pause_rows(&menu, &bindings, &audio, &rebind, &ui_prefs, &rig);
    commands.entity(list).with_children(|parent| {
        for (i, row) in rows.iter().enumerate() {
            spawn_pause_row(parent, fonts, scale, row, i == menu.sel);
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
                    4 => {
                        audio.commentary_volume = (audio.commentary_volume + delta).clamp(0.0, 1.0)
                    }
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

    fn sample_pause_rows(tab: SettingsTab) -> Vec<PauseRow> {
        let menu = PauseMenuState {
            screen: PauseScreen::Settings,
            sel: 0,
            settings_tab: tab,
        };
        pause_rows(
            &menu,
            &KeyBindings::default(),
            &AudioSettings::default(),
            &RebindState::default(),
            &UiPreferences::default(),
            &CameraRig::default(),
        )
    }

    #[test]
    fn settings_item_count_matches_pause_rows() {
        for tab in [
            SettingsTab::Audio,
            SettingsTab::Controls,
            SettingsTab::Camera,
        ] {
            let rows = sample_pause_rows(tab);
            assert_eq!(rows.len(), settings_item_count(tab), "tab {tab:?}");
        }
    }

    #[test]
    fn pause_rows_tab_strip_is_always_row_zero() {
        for tab in [
            SettingsTab::Audio,
            SettingsTab::Controls,
            SettingsTab::Camera,
        ] {
            let rows = sample_pause_rows(tab);
            assert!(matches!(rows[0], PauseRow::Tabs(t) if t == tab));
        }
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
    fn reset_pause_state_returns_to_root_and_clears_rebind() {
        // Regression test for the "settings menu stays around after
        // resuming" bug: closing the overlay must not leave the player
        // deep in Settings (or mid-rebind) the next time they open it.
        let mut menu = PauseMenuState {
            screen: PauseScreen::Settings,
            sel: 3,
            settings_tab: SettingsTab::Controls,
        };
        let mut rebind = RebindState(Some(Action::Confirm));

        reset_pause_state(&mut menu, &mut rebind);

        assert_eq!(menu.screen, PauseScreen::Root);
        assert_eq!(menu.sel, 0);
        assert_eq!(menu.settings_tab, SettingsTab::Audio);
        assert_eq!(rebind.0, None);
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

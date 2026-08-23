//! In-match HUD: scoreboard, batter/bowler stats, phase prompts, outcome
//! banner and the shot-timing meter.

use crate::core::{footwork_from_move_y, select_shot};
use crate::game::*;
use crate::input::{Action, KeyBindings, PlayerInput, action_label};
use crate::ui::theme::{UiFonts, palette, register_ui_font_assets};
use crate::state::AppState;
use bevy::prelude::*;

#[derive(Component)]
struct HudRoot;
#[derive(Component)]
struct ScoreBug;
#[derive(Component)]
struct ScoreAccent;
#[derive(Component)]
struct ScoreCrest;
#[derive(Component)]
enum ScoreField {
    Innings,
    Team,
    Runs,
    Overs,
    Equation,
    Batters,
    Bowler,
    Delivery,
    LastSix,
    Partnership,
}
#[derive(Component)]
struct PromptRoot;
#[derive(Component)]
enum PromptField {
    Kind,
    Message,
}
#[derive(Component)]
struct OutcomeRoot;
#[derive(Component)]
struct OutcomePanel;
#[derive(Component)]
struct OutcomeAccent;
#[derive(Component)]
enum OutcomeField {
    Kind,
    Message,
}
#[derive(Component)]
struct MeterRoot;
#[derive(Component)]
struct MeterMarker;
#[derive(Component)]
struct MeterZoneEarly;
#[derive(Component)]
struct MeterZonePerfect;
#[derive(Component)]
struct MeterZoneLate;
#[derive(Component)]
struct MeterLabel;
#[derive(Component)]
struct BroadcastChip;
#[derive(Component)]
struct ShotDirRoot;
#[derive(Component)]
struct ShotPreviewRoot;
#[derive(Component)]
struct ShotPreviewText;
#[derive(Component)]
struct ShotLegendRoot;
#[derive(Component)]
struct ShotLegendText;
#[derive(Component)]
struct SummaryRoot;
#[derive(Component)]
struct SummaryText;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        register_ui_font_assets(app);
        app.add_systems(OnEnter(AppState::InMatch), spawn_hud)
            .add_systems(OnExit(AppState::InMatch), despawn_hud)
            .add_systems(
                Update,
                (
                    update_scoreboard,
                    update_prompt,
                    update_outcome,
                    update_meter,
                    update_summary,
                    update_broadcast_chip,
                    update_shot_direction,
                    update_shot_preview,
                )
                    .run_if(in_state(AppState::InMatch)),
            );
    }
}

// ---------------------------------------------------------------------------
// Build / teardown
// ---------------------------------------------------------------------------

fn meter_track_bg() -> BackgroundColor {
    BackgroundColor(Color::srgba(0.05, 0.05, 0.08, 0.72))
}

fn text_shadow() -> TextShadow {
    TextShadow {
        offset: Vec2::new(0.0, 2.0),
        color: Color::srgba(0.0, 0.0, 0.0, 0.7),
    }
}

fn label(font: Handle<Font>, size: f32, color: Color) -> impl Bundle {
    (
        TextFont {
            font,
            font_size: size,
            ..default()
        },
        TextColor(color),
    )
}

fn label_text(
    content: impl Into<String>,
    font: Handle<Font>,
    size: f32,
    color: Color,
) -> impl Bundle {
    (Text::new(content), label(font, size, color))
}

fn row(node: Node, bg: Color) -> impl Bundle {
    (node, BackgroundColor(bg))
}

fn bordered_panel(node: Node, bg: Color, border: Color) -> impl Bundle {
    (node, BackgroundColor(bg), BorderColor::all(border))
}

fn spawn_hud(mut commands: Commands, assets: Res<AssetServer>) {
    let fonts = UiFonts::load(assets.as_ref());
    let default_crest = crate::render::load_team_crest(&assets, "branding/teams/ind.png");

    commands
        .spawn((
            HudRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
            // HUD must never intercept input.
        ))
        .with_children(|p| {
            spawn_scorebug(p, &fonts, default_crest);
            spawn_boundary_chip(p, &fonts);
            spawn_outcome_panel(p, &fonts);
            spawn_prompt_row(p, &fonts);
            spawn_summary_panel(p, &fonts);
            spawn_timing_meter(p, &fonts);
            spawn_shot_direction_indicator(p, &fonts);
            spawn_shot_preview(p, &fonts);
            spawn_shot_legend(p, &fonts);
        });
}

fn spawn_scorebug(
    parent: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    default_crest: Handle<Image>,
) {
    parent
        .spawn((
            ScoreBug,
            Node {
                position_type: PositionType::Absolute,
                top: px(18),
                left: px(18),
                width: px(445),
                flex_direction: FlexDirection::Column,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(palette::panel_bg()),
            BorderColor::all(palette::panel_border()),
        ))
        .with_children(|c| {
            c.spawn((
                ScoreAccent,
                Node {
                    width: percent(100),
                    height: px(4),
                    ..default()
                },
                BackgroundColor(palette::accent_blue()),
            ));
            c.spawn(row(
                Node {
                    width: percent(100),
                    height: px(25),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                    padding: UiRect::horizontal(px(12)),
                    ..default()
                },
                palette::surface_header(),
            ))
            .with_children(|header| {
                header.spawn((
                    ScoreField::Innings,
                    label_text(
                        "1ST INNINGS",
                        fonts.bold.clone(),
                        10.0,
                        Color::srgb(0.76, 0.81, 0.86),
                    ),
                ));
                header.spawn(label_text(
                    "WILLOW  •  LIVE",
                    fonts.bold.clone(),
                    10.0,
                    palette::gold(),
                ));
            });
            c.spawn(Node {
                width: percent(100),
                height: px(75),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(12)),
                column_gap: px(11),
                ..default()
            })
            .with_children(|main| {
                main.spawn((
                    ScoreCrest,
                    ImageNode::new(default_crest.clone()),
                    Node {
                        width: px(48),
                        height: px(48),
                        ..default()
                    },
                ));
                main.spawn((
                    ScoreField::Team,
                    label_text("IND", fonts.display.clone(), 22.0, palette::text_primary()),
                    text_shadow(),
                    Node {
                        width: px(52),
                        ..default()
                    },
                ));
                main.spawn((
                    ScoreField::Runs,
                    label_text("0/0", fonts.display.clone(), 39.0, Color::WHITE),
                    text_shadow(),
                    Node {
                        flex_grow: 1.0,
                        ..default()
                    },
                ));
                main.spawn(row(
                    Node {
                        height: px(31),
                        padding: UiRect::horizontal(px(11)),
                        align_items: AlignItems::Center,
                        border_radius: BorderRadius::all(px(3)),
                        ..default()
                    },
                    palette::chip_bg(),
                ))
                .with_children(|overs| {
                    overs.spawn((
                        ScoreField::Overs,
                        label_text(
                            "0.0 OV",
                            fonts.bold.clone(),
                            14.0,
                            Color::srgb(0.94, 0.95, 0.97),
                        ),
                    ));
                });
            });
            c.spawn((
                ScoreAccent,
                row(
                    Node {
                        width: percent(100),
                        height: px(27),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(px(12)),
                        ..default()
                    },
                    palette::accent_blue(),
                ),
            ))
            .with_children(|ribbon| {
                ribbon.spawn((
                    ScoreField::Equation,
                    label_text(
                        "CURRENT RUN RATE  0.00",
                        fonts.bold.clone(),
                        11.0,
                        Color::WHITE,
                    ),
                    text_shadow(),
                ));
            });
            c.spawn(row(
                Node {
                    width: percent(100),
                    height: px(31),
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(px(12)),
                    ..default()
                },
                palette::surface_row(),
            ))
            .with_children(|batters| {
                batters.spawn((
                    ScoreField::Batters,
                    label_text("", fonts.bold.clone(), 13.0, Color::srgb(0.92, 0.94, 0.96)),
                ));
            });
            c.spawn(row(
                Node {
                    width: percent(100),
                    height: px(29),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                    padding: UiRect::horizontal(px(12)),
                    ..default()
                },
                palette::surface_row_alt(),
            ))
            .with_children(|delivery| {
                delivery.spawn((
                    ScoreField::Bowler,
                    label_text("", fonts.regular.clone(), 12.0, palette::text_muted()),
                ));
                delivery.spawn((
                    ScoreField::Delivery,
                    label_text("", fonts.bold.clone(), 11.0, palette::gold()),
                ));
            });
            c.spawn(row(
                Node {
                    width: percent(100),
                    height: px(24),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                    padding: UiRect::horizontal(px(12)),
                    ..default()
                },
                palette::surface_strip(),
            ))
            .with_children(|strip| {
                strip.spawn((
                    ScoreField::LastSix,
                    label_text(
                        "—  —  —  —  —  —",
                        fonts.bold.clone(),
                        12.0,
                        Color::srgb(0.86, 0.88, 0.90),
                    ),
                    text_shadow(),
                ));
            });
            c.spawn(row(
                Node {
                    width: percent(100),
                    height: px(22),
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(px(12)),
                    ..default()
                },
                palette::surface_deep(),
            ))
            .with_children(|part| {
                part.spawn((
                    ScoreField::Partnership,
                    label_text(
                        "",
                        fonts.regular.clone(),
                        11.0,
                        Color::srgb(0.72, 0.76, 0.80),
                    ),
                ));
            });
        });
}

fn spawn_boundary_chip(parent: &mut ChildSpawnerCommands, fonts: &UiFonts) {
    parent
        .spawn((
            BroadcastChip,
            Node {
                position_type: PositionType::Absolute,
                top: px(18),
                right: px(18),
                padding: UiRect::axes(px(14), px(6)),
                border_radius: BorderRadius::all(px(3)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.92, 0.14, 0.18, 0.92)),
            Visibility::Hidden,
        ))
        .with_children(|chip| {
            chip.spawn((
                label_text("REPLAY", fonts.bold.clone(), 11.0, Color::WHITE),
                text_shadow(),
            ));
        });
}

fn spawn_outcome_panel(parent: &mut ChildSpawnerCommands, fonts: &UiFonts) {
    parent
        .spawn((
            OutcomeRoot,
            Node {
                position_type: PositionType::Absolute,
                top: percent(27),
                width: percent(100),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Visibility::Hidden,
        ))
        .with_children(|root| {
            root.spawn((
                OutcomePanel,
                bordered_panel(
                    Node {
                        min_width: px(390),
                        height: px(76),
                        align_items: AlignItems::Stretch,
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(5)),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    palette::boundary_gold_bg(),
                    Color::srgba(1.0, 0.78, 0.30, 0.55),
                ),
            ))
            .with_children(|panel| {
                panel.spawn((
                    OutcomeAccent,
                    Node {
                        width: px(7),
                        height: percent(100),
                        ..default()
                    },
                    BackgroundColor(palette::boundary_gold()),
                ));
                panel
                    .spawn(row(
                        Node {
                            width: px(96),
                            height: percent(100),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            ..default()
                        },
                        Color::srgba(0.0, 0.0, 0.0, 0.22),
                    ))
                    .with_children(|kind| {
                        kind.spawn((
                            OutcomeField::Kind,
                            label_text(
                                "BOUNDARY",
                                fonts.bold.clone(),
                                11.0,
                                palette::boundary_gold(),
                            ),
                        ));
                    });
                panel
                    .spawn(Node {
                        flex_grow: 1.0,
                        height: percent(100),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(px(22)),
                        ..default()
                    })
                    .with_children(|message| {
                        message.spawn((
                            OutcomeField::Message,
                            label_text("FOUR", fonts.display.clone(), 31.0, Color::WHITE),
                            text_shadow(),
                        ));
                    });
            });
        });
}

fn spawn_prompt_row(parent: &mut ChildSpawnerCommands, fonts: &UiFonts) {
    parent
        .spawn((
            PromptRoot,
            Node {
                position_type: PositionType::Absolute,
                bottom: px(24),
                width: percent(100),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Visibility::Hidden,
        ))
        .with_children(|root| {
            root.spawn(bordered_panel(
                Node {
                    min_width: px(520),
                    height: px(39),
                    align_items: AlignItems::Stretch,
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(4)),
                    overflow: Overflow::clip(),
                    ..default()
                },
                Color::srgba(0.02, 0.03, 0.045, 0.94),
                Color::srgba(0.78, 0.84, 0.90, 0.30),
            ))
            .with_children(|panel| {
                panel
                    .spawn(row(
                        Node {
                            width: px(112),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            ..default()
                        },
                        Color::srgb(0.76, 0.55, 0.16),
                    ))
                    .with_children(|kind| {
                        kind.spawn((
                            PromptField::Kind,
                            label_text(
                                "BATTING",
                                fonts.bold.clone(),
                                11.0,
                                Color::srgb(0.04, 0.05, 0.06),
                            ),
                        ));
                    });
                panel
                    .spawn(Node {
                        flex_grow: 1.0,
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(px(17)),
                        ..default()
                    })
                    .with_children(|message| {
                        message.spawn((
                            PromptField::Message,
                            label_text("", fonts.bold.clone(), 13.0, Color::srgb(0.94, 0.96, 0.98)),
                        ));
                    });
            });
        });
}

fn spawn_summary_panel(parent: &mut ChildSpawnerCommands, fonts: &UiFonts) {
    parent
        .spawn((
            SummaryRoot,
            Node {
                position_type: PositionType::Absolute,
                top: percent(38),
                left: percent(18),
                width: percent(64),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                padding: UiRect::all(px(14)),
                row_gap: px(6),
                ..default()
            },
            BackgroundColor(Color::srgba(0.04, 0.06, 0.05, 0.88)),
            Visibility::Hidden,
        ))
        .with_children(|c| {
            c.spawn((
                SummaryText,
                label_text("", fonts.bold.clone(), 20.0, Color::WHITE),
                TextLayout::new_with_justify(Justify::Center),
            ));
        });
}

fn spawn_timing_meter(parent: &mut ChildSpawnerCommands, fonts: &UiFonts) {
    parent
        .spawn((
            MeterRoot,
            Node {
                position_type: PositionType::Absolute,
                bottom: percent(13),
                left: percent(32),
                width: percent(36),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(4),
                ..default()
            },
            Visibility::Hidden,
        ))
        .with_children(|m| {
            m.spawn((
                MeterLabel,
                label_text(
                    "TIMING",
                    fonts.bold.clone(),
                    10.0,
                    Color::srgba(0.82, 0.86, 0.90, 0.88),
                ),
            ));
            m.spawn((
                Node {
                    width: percent(100),
                    height: px(32),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(4)),
                    overflow: Overflow::clip(),
                    ..default()
                },
                meter_track_bg(),
                BorderColor::all(Color::srgba(0.55, 0.62, 0.72, 0.35)),
            ))
            .with_children(|track| {
                track.spawn((
                    MeterZoneEarly,
                    Node {
                        position_type: PositionType::Absolute,
                        left: percent(0),
                        width: percent(38),
                        height: percent(100),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.85, 0.22, 0.18, 0.35)),
                ));
                track.spawn((
                    MeterZonePerfect,
                    Node {
                        position_type: PositionType::Absolute,
                        left: percent(38),
                        width: percent(24),
                        height: percent(100),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.18, 0.78, 0.32, 0.55)),
                ));
                track.spawn((
                    MeterZoneLate,
                    Node {
                        position_type: PositionType::Absolute,
                        left: percent(62),
                        width: percent(38),
                        height: percent(100),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.85, 0.55, 0.12, 0.35)),
                ));
                track.spawn((
                    MeterMarker,
                    Node {
                        position_type: PositionType::Absolute,
                        top: px(0),
                        left: percent(50),
                        width: px(5),
                        height: percent(100),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(1.0, 0.98, 0.88)),
                    BorderColor::all(Color::srgb(0.12, 0.12, 0.14)),
                ));
            });
        });
}

fn spawn_shot_direction_indicator(parent: &mut ChildSpawnerCommands, fonts: &UiFonts) {
    parent
        .spawn((
            ShotDirRoot,
            Node {
                position_type: PositionType::Absolute,
                bottom: percent(19),
                left: percent(42),
                width: percent(16),
                height: px(18),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                ..default()
            },
            Visibility::Hidden,
        ))
        .with_children(|dir| {
            dir.spawn(label_text(
                "LEG",
                fonts.bold.clone(),
                9.0,
                palette::text_dim(),
            ));
            dir.spawn((label_text(
                "▲",
                fonts.bold.clone(),
                12.0,
                Color::srgb(0.98, 0.82, 0.28),
            ),));
            dir.spawn(label_text(
                "OFF",
                fonts.bold.clone(),
                9.0,
                palette::text_dim(),
            ));
        });
}

fn spawn_shot_preview(parent: &mut ChildSpawnerCommands, fonts: &UiFonts) {
    parent
        .spawn((
            ShotPreviewRoot,
            Node {
                position_type: PositionType::Absolute,
                bottom: percent(22),
                left: percent(32),
                width: percent(36),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            Visibility::Hidden,
        ))
        .with_children(|p| {
            p.spawn((
                ShotPreviewText,
                label_text(
                    "Straight Drive",
                    fonts.bold.clone(),
                    13.0,
                    palette::gold(),
                ),
                text_shadow(),
            ));
        });
}

fn spawn_shot_legend(parent: &mut ChildSpawnerCommands, fonts: &UiFonts) {
    parent
        .spawn((
            ShotLegendRoot,
            Node {
                position_type: PositionType::Absolute,
                bottom: px(24),
                right: px(18),
                padding: UiRect::axes(px(10), px(5)),
                border_radius: BorderRadius::all(px(3)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.03, 0.04, 0.06, 0.82)),
            Visibility::Hidden,
        ))
        .with_children(|p| {
            p.spawn((
                ShotLegendText,
                label_text(
                    "W/S FOOT  •  A/D AIM  •  SHIFT LOFT",
                    fonts.regular.clone(),
                    9.0,
                    palette::text_dim(),
                ),
            ));
        });
}

fn despawn_hud(mut commands: Commands, q: Query<Entity, With<HudRoot>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

// ---------------------------------------------------------------------------
// Per-frame updates
// ---------------------------------------------------------------------------

const METER_WINDOW: f32 = 0.5; // maps ±0.25 s around perfect contact

fn update_scoreboard(
    am: Option<Res<ActiveMatch>>,
    wd: Res<WorldData>,
    del: Res<CurrentDelivery>,
    recent: Res<RecentBalls>,
    assets: Res<AssetServer>,
    mut text_q: Query<(&ScoreField, &mut Text)>,
    mut accent_q: Query<&mut BackgroundColor, With<ScoreAccent>>,
    mut crest_q: Query<&mut ImageNode, With<ScoreCrest>>,
) {
    let Some(am) = am else { return };
    let inns = &am.state.innings;
    let bat = am.batting_team(&wd);
    if bat.players.get(inns.striker).is_none() || bat.players.get(inns.non_striker).is_none() {
        return;
    }
    let s = &bat.players[inns.striker];
    let ns = &bat.players[inns.non_striker];
    let sc = inns.card_of(inns.striker);
    let nc = inns.card_of(inns.non_striker);

    let bowler = match inns.current_bowler {
        Some(b) => {
            let bp = &am.fielding_team(&wd).players[b];
            let bc = inns.bowler_card_of(b);
            format!(
                "BOWLER  {}   {}/{}  ({}.{})",
                bp.name.to_uppercase(),
                bc.wickets,
                bc.runs,
                bc.balls / 6,
                bc.balls % 6
            )
        }
        None => String::new(),
    };

    let equation = match inns.target {
        Some(tg) => {
            let need = tg.saturating_sub(inns.runs);
            let balls_left = (am.state.overs * 6).saturating_sub(inns.legal_balls);
            let required = if balls_left == 0 {
                0.0
            } else {
                need as f32 * 6.0 / balls_left as f32
            };
            format!("TARGET {tg}   •   NEED {need} FROM {balls_left}   •   RRR {required:.2}")
        }
        None => format!("CURRENT RUN RATE   {:.2}", inns.run_rate()),
    };

    let innings_label = if inns.target.is_some() {
        "2ND INNINGS  •  CHASE"
    } else {
        "1ST INNINGS"
    };
    let batters = format!(
        "●  {}  {} ({})       {}  {} ({})",
        s.name.to_uppercase(),
        sc.runs,
        sc.balls,
        ns.name.to_uppercase(),
        nc.runs,
        nc.balls,
    );
    let delivery = del
        .0
        .as_ref()
        .map(|plan| plan.label.to_uppercase())
        .unwrap_or_default();

    let partnership = format!(
        "PARTNERSHIP  {} ({} balls)",
        sc.runs + nc.runs,
        sc.balls + nc.balls,
    );
    let last_six = recent.display();

    for (field, mut text) in &mut text_q {
        **text = match field {
            ScoreField::Innings => innings_label.into(),
            ScoreField::Team => bat.short.to_uppercase(),
            ScoreField::Runs => format!("{}/{}", inns.runs, inns.wickets),
            ScoreField::Overs => format!("{}.{} OV", inns.legal_balls / 6, inns.legal_balls % 6),
            ScoreField::Equation => equation.clone(),
            ScoreField::Batters => batters.clone(),
            ScoreField::Bowler => bowler.clone(),
            ScoreField::Delivery => delivery.clone(),
            ScoreField::LastSix => last_six.clone(),
            ScoreField::Partnership => partnership.clone(),
        };
    }
    for mut background in &mut accent_q {
        background.0 = bat.primary_color;
    }
    if let Ok(mut crest) = crest_q.single_mut() {
        *crest = ImageNode::new(crate::render::load_team_crest(&assets, &bat.crest_asset()));
    }
}

fn update_prompt(
    phase: Res<Phase>,
    am: Option<Res<ActiveMatch>>,
    attempt: Res<ShotAttempt>,
    bindings: Res<KeyBindings>,
    input: Res<PlayerInput>,
    mut root_q: Query<&mut Visibility, With<PromptRoot>>,
    mut text_q: Query<(&PromptField, &mut Text)>,
) {
    let confirm = action_label(Action::Confirm, &bindings, input.gamepad_connected);
    let loft = action_label(Action::Loft, &bindings, input.gamepad_connected);
    let prompt = match &phase.0 {
        PhaseEnum::ReadyToBall { .. } => match am.as_deref().map(|m| (m.user_bowling(), m.user_batting())) {
            Some((true, _)) => Some((
                "BOWLING",
                format!("PRESS {confirm} TO START RUN-UP"),
            )),
            Some((false, true)) => Some((
                "BATTING",
                format!("PRESS {confirm} WHEN READY"),
            )),
            _ => None,
        },
        PhaseEnum::AimLength { lock, .. } => match lock {
            None => Some((
                "BOWLING",
                format!("PRESS {confirm} TO LOCK LENGTH"),
            )),
            Some(_) => Some((
                "BOWLING",
                format!("PRESS {confirm} TO LOCK LINE"),
            )),
        },
        PhaseEnum::BallLive => {
            if am.as_deref().map(|m| m.user_batting()).unwrap_or(false) {
                Some((
                    "BATTING",
                    format!(
                        "{confirm} SHOT  •  {loft} LOFT  •  W/S FOOT  •  A/D AIM"
                    ),
                ))
            } else {
                None
            }
        }
        PhaseEnum::ResultPause { t, text } if *t < 1.35 && attempt.pressed => Some((
            "SHOT",
            format!(
                "{}  •  {}",
                attempt.kind.label().to_uppercase(),
                text.to_uppercase()
            ),
        )),
        PhaseEnum::OverBreak { .. } => Some(("END OF OVER", "NEXT BOWLER COMING ON".into())),
        PhaseEnum::InningsBreak => Some((
            "INNINGS BREAK",
            format!("PRESS {confirm} TO BEGIN THE CHASE"),
        )),
        PhaseEnum::MatchOver => Some((
            "MATCH RESULT",
            format!("PRESS {confirm} TO CONTINUE"),
        )),
        _ => None,
    };

    let Ok(mut visibility) = root_q.single_mut() else {
        return;
    };
    let Some((kind, message)) = prompt else {
        *visibility = Visibility::Hidden;
        return;
    };
    *visibility = Visibility::Visible;
    for (field, mut text) in &mut text_q {
        **text = match field {
            PromptField::Kind => kind.into(),
            PromptField::Message => message.clone(),
        };
    }
}

fn update_outcome(
    phase: Res<Phase>,
    mut root_q: Query<&mut Visibility, With<OutcomeRoot>>,
    mut panel_q: Query<(&mut BackgroundColor, &mut BorderColor), With<OutcomePanel>>,
    mut accent_q: Query<&mut BackgroundColor, (With<OutcomeAccent>, Without<OutcomePanel>)>,
    mut text_q: Query<(&OutcomeField, &mut Text, &mut TextColor)>,
) {
    let Ok(mut visibility) = root_q.single_mut() else {
        return;
    };
    let PhaseEnum::ResultPause { text, .. } = &phase.0 else {
        *visibility = Visibility::Hidden;
        return;
    };
    *visibility = Visibility::Visible;
    let (kind, accent, panel) = outcome_style(text);
    if let Ok((mut background, mut border)) = panel_q.single_mut() {
        background.0 = panel;
        *border = BorderColor::all(accent.with_alpha(0.58));
    }
    if let Ok(mut background) = accent_q.single_mut() {
        background.0 = accent;
    }
    for (field, mut value, mut color) in &mut text_q {
        match field {
            OutcomeField::Kind => {
                **value = kind.into();
                color.0 = accent;
            }
            OutcomeField::Message => {
                **value = text.to_uppercase();
                color.0 = Color::WHITE;
            }
        }
    }
}

fn outcome_style(text: &str) -> (&'static str, Color, Color) {
    let upper = text.to_uppercase();
    if ["BOWLED", "CAUGHT", "RUN OUT", "WICKET", "TAKEN"]
        .iter()
        .any(|word| upper.contains(word))
    {
        (
            "WICKET",
            Color::srgb(0.98, 0.28, 0.24),
            Color::srgba(0.16, 0.025, 0.03, 0.96),
        )
    } else if ["FOUR", "SIX", "MAXIMUM", "BOUNDARY"]
        .iter()
        .any(|word| upper.contains(word))
    {
        (
            "BOUNDARY",
            palette::boundary_gold(),
            palette::boundary_gold_bg(),
        )
    } else if upper.contains("WIDE") || upper.contains("NO BALL") {
        (
            "EXTRAS",
            Color::srgb(0.25, 0.82, 0.92),
            Color::srgba(0.015, 0.09, 0.12, 0.96),
        )
    } else if upper.contains("RUN") {
        (
            "RUNS",
            Color::srgb(0.30, 0.84, 0.48),
            Color::srgba(0.02, 0.11, 0.055, 0.96),
        )
    } else {
        (
            "DELIVERY",
            Color::srgb(0.64, 0.72, 0.82),
            Color::srgba(0.035, 0.05, 0.075, 0.96),
        )
    }
}

fn update_meter(
    phase: Res<Phase>,
    am: Option<Res<ActiveMatch>>,
    rel: Res<ReleaseInfo>,
    attempt: Res<ShotAttempt>,
    time: Res<Time>,
    mut root_q: Query<&mut Visibility, With<MeterRoot>>,
    mut marker_q: Query<&mut Node, With<MeterMarker>>,
    mut label_q: Query<&mut Text, With<MeterLabel>>,
) {
    let Ok(mut vis) = root_q.single_mut() else {
        return;
    };
    let show = matches!(phase.0, PhaseEnum::BallLive)
        && am.as_deref().map(|m| m.user_batting()).unwrap_or(false)
        && rel.active;
    *vis = if show {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    if !show {
        return;
    }
    let t = rel.t / rel.t_arrive.max(0.01);
    let sweep = (time.elapsed_secs() * 2.8).sin() * 0.5 + 0.5;
    let frac = if attempt.pressed {
        ((attempt.offset.unwrap_or(rel.t - rel.t_arrive) / METER_WINDOW) + 0.5).clamp(0.0, 1.0)
    } else {
        sweep
    };
    if let Ok(mut node) = marker_q.single_mut() {
        node.left = percent(frac * 100.0);
    }
    if let Ok(mut label) = label_q.single_mut() {
        **label = if attempt.pressed {
            let off = attempt.offset.unwrap_or(0.0);
            if off.abs() < 0.055 {
                "PERFECT".into()
            } else if off.abs() < 0.12 {
                "GOOD".into()
            } else if off < 0.0 {
                "TOO EARLY".into()
            } else {
                "TOO LATE".into()
            }
        } else {
            format!("CONTACT IN {:.1}s", (rel.t_arrive - rel.t).max(0.0))
        };
    }
    let _ = t;
}

fn update_broadcast_chip(
    pres: Res<crate::render::camera_rig::PresentationState>,
    mut chip_q: Query<(&mut Visibility, &mut BackgroundColor, &Children), With<BroadcastChip>>,
    mut text_q: Query<&mut Text>,
) {
    let Ok((mut vis, mut bg, children)) = chip_q.single_mut() else {
        return;
    };
    let show = pres.replay_on || pres.impact_on;
    *vis = if show {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    if !show {
        return;
    }
    bg.0 = if pres.impact_on {
        palette::wicket_red()
    } else {
        Color::srgba(0.08, 0.38, 0.82, 0.94)
    };
    for child in children.iter() {
        if let Ok(mut text) = text_q.get_mut(child) {
            **text = if pres.impact_on { "IMPACT" } else { "REPLAY" }.into();
        }
    }
}

fn update_shot_direction(
    phase: Res<Phase>,
    am: Option<Res<ActiveMatch>>,
    input: Res<crate::input::PlayerInput>,
    bindings: Res<KeyBindings>,
    // Disjoint filters: both write Visibility, so Bevy needs proof the sets
    // cannot overlap (B0001).
    mut root_q: Query<(&mut Visibility, &Children), (With<ShotDirRoot>, Without<ShotLegendRoot>)>,
    mut legend_q: Query<(&mut Visibility, &Children), (With<ShotLegendRoot>, Without<ShotDirRoot>)>,
    mut legend_text_q: Query<&mut Text, With<ShotLegendText>>,
    mut arrow_q: Query<&mut Node>,
) {
    let Ok((mut vis, children)) = root_q.single_mut() else {
        return;
    };
    let show = matches!(phase.0, PhaseEnum::BallLive)
        && am.as_deref().map(|m| m.user_batting()).unwrap_or(false);
    *vis = if show {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    if let Ok((mut legend_vis, legend_children)) = legend_q.single_mut() {
        *legend_vis = *vis;
        if show {
            let confirm = action_label(Action::Confirm, &bindings, input.gamepad_connected);
            let loft = action_label(Action::Loft, &bindings, input.gamepad_connected);
            if let Some(ent) = legend_children.first()
                && let Ok(mut text) = legend_text_q.get_mut(*ent)
            {
                **text = format!("W/S FOOT  •  A/D AIM  •  {loft} LOFT  •  {confirm} SHOT");
            }
        }
    }
    if !show {
        return;
    }
    // Middle child is the arrow indicator (LEG, arrow, OFF)
    if let Some(arrow_ent) = children.iter().nth(1)
        && let Ok(mut node) = arrow_q.get_mut(arrow_ent)
    {
        let x = input.move_vec.x.clamp(-1.0, 1.0);
        node.margin = UiRect::horizontal(px(x * 28.0));
    }
}

fn update_shot_preview(
    phase: Res<Phase>,
    am: Option<Res<ActiveMatch>>,
    attempt: Res<ShotAttempt>,
    input: Res<PlayerInput>,
    mut root_q: Query<&mut Visibility, With<ShotPreviewRoot>>,
    mut text_q: Query<&mut Text, With<ShotPreviewText>>,
) {
    let Ok(mut vis) = root_q.single_mut() else {
        return;
    };
    let batting_live = matches!(phase.0, PhaseEnum::BallLive)
        && am.as_deref().map(|m| m.user_batting()).unwrap_or(false);
    let result_flash = matches!(phase.0, PhaseEnum::ResultPause { .. }) && attempt.pressed;
    *vis = if batting_live || result_flash {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    if !batting_live && !result_flash {
        return;
    }
    let Ok(mut text) = text_q.single_mut() else {
        return;
    };
    **text = if batting_live {
        let fw = footwork_from_move_y(input.move_vec.y);
        let loft = input.held(Action::Loft);
        let aim = input.move_vec.x.clamp(-1.0, 1.0);
        select_shot(fw, aim, loft).label().to_uppercase()
    } else {
        attempt.kind.label().to_uppercase()
    };
}

fn update_summary(
    phase: Res<Phase>,
    am: Option<Res<ActiveMatch>>,
    wd: Res<WorldData>,
    mut root_q: Query<&mut Visibility, With<SummaryRoot>>,
    mut text_q: Query<&mut Text, With<SummaryText>>,
) {
    let Ok(mut vis) = root_q.single_mut() else {
        return;
    };
    let Ok(mut txt) = text_q.single_mut() else {
        return;
    };
    if !matches!(phase.0, PhaseEnum::MatchOver) {
        *vis = Visibility::Hidden;
        return;
    }
    let Some(am) = am.as_deref() else {
        *vis = Visibility::Hidden;
        return;
    };
    *vis = Visibility::Visible;
    let inns = &am.state.innings;
    // Current innings team
    let bat_team = am.batting_team(&wd);
    let bowl_team = am.fielding_team(&wd);
    // Top scorer in current innings
    let top = inns
        .cards
        .iter()
        .max_by_key(|c| c.runs)
        .map(|c| {
            let name = &wd.teams[am.state.teams[0]].players[c.player].name;
            format!("{name} {}({})", c.runs, c.balls)
        })
        .unwrap_or_else(|| "-".into());
    let best_bowler = inns
        .bowlers
        .iter()
        .max_by_key(|b| b.wickets as i32 * 100 - b.runs as i32)
        .map(|b| {
            let name = &bowl_team.players[b.player].name;
            format!(
                "{name} {}/{} ({}.{})",
                b.wickets,
                b.runs,
                b.balls / 6,
                b.balls % 6
            )
        })
        .unwrap_or_else(|| "-".into());

    let result_line = match &am.state.result {
        Some(crate::core::rules::Result::Win { winner, margin }) => {
            let wname = wd.teams[*winner].name.clone();
            format!("{wname} {margin}")
        }
        Some(crate::core::rules::Result::Tie) => "Match Tied".into(),
        None => "Match Complete".into(),
    };

    let overs_str = format!("{}.{} ov", inns.legal_balls / 6, inns.legal_balls % 6);
    let first_line = if let Some(t) = am.state.first_innings_total {
        format!("First innings: {t}  |  Chase: {}  ->  {}", t + 1, inns.runs)
    } else {
        String::new()
    };

    **txt = format!(
        "{}\n{}  {}/{}  ({})\n{}\nTop: {}\nBest: {}\n{}",
        result_line,
        bat_team.short,
        inns.runs,
        inns.wickets,
        overs_str,
        first_line,
        top,
        best_bowler,
        "SPACE to continue"
    );
}

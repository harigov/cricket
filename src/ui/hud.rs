//! In-match HUD: scoreboard, batter/bowler stats, phase prompts, outcome
//! banner and the shot-timing meter.

use crate::game::*;
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
struct SummaryRoot;
#[derive(Component)]
struct SummaryText;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "../../assets/fonts/Lato-Black.ttf");
        bevy::asset::embedded_asset!(app, "../../assets/fonts/Lato-Bold.ttf");
        bevy::asset::embedded_asset!(app, "../../assets/fonts/Lato-Regular.ttf");
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
                )
                    .run_if(in_state(AppState::InMatch)),
            );
    }
}

// ---------------------------------------------------------------------------
// Build / teardown
// ---------------------------------------------------------------------------

fn panel_bg() -> BackgroundColor {
    BackgroundColor(Color::srgba(0.05, 0.05, 0.08, 0.72))
}

fn text_shadow() -> TextShadow {
    TextShadow {
        offset: Vec2::new(0.0, 2.0),
        color: Color::srgba(0.0, 0.0, 0.0, 0.7),
    }
}

fn spawn_hud(mut commands: Commands, assets: Res<AssetServer>) {
    let display = bevy::asset::load_embedded_asset!(
        assets.as_ref(),
        "../../assets/fonts/Lato-Black.ttf"
    );
    let bold = bevy::asset::load_embedded_asset!(
        assets.as_ref(),
        "../../assets/fonts/Lato-Bold.ttf"
    );
    let regular = bevy::asset::load_embedded_asset!(
        assets.as_ref(),
        "../../assets/fonts/Lato-Regular.ttf"
    );
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
            // ---- television scorebug, top-left ----
            p.spawn((
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
                BackgroundColor(Color::srgba(0.015, 0.025, 0.04, 0.96)),
                BorderColor::all(Color::srgba(0.72, 0.82, 0.90, 0.26)),
            ))
            .with_children(|c| {
                c.spawn((
                    ScoreAccent,
                    Node { width: percent(100), height: px(4), ..default() },
                    BackgroundColor(Color::srgb(0.08, 0.38, 0.82)),
                ));
                c.spawn((Node {
                    width: percent(100), height: px(25), align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                    padding: UiRect::horizontal(px(12)), ..default()
                }, BackgroundColor(Color::srgba(0.08, 0.10, 0.14, 0.98))))
                .with_children(|header| {
                    header.spawn((ScoreField::Innings, Text::new("1ST INNINGS"), TextFont {
                        font: bold.clone(), font_size: 10.0, ..default()
                    }, TextColor(Color::srgb(0.76, 0.81, 0.86))));
                    header.spawn((Text::new("WILLOW  •  LIVE"), TextFont {
                        font: bold.clone(), font_size: 10.0, ..default()
                    }, TextColor(Color::srgb(0.98, 0.72, 0.25))));
                });
                c.spawn(Node {
                    width: percent(100), height: px(75), align_items: AlignItems::Center,
                    padding: UiRect::horizontal(px(12)), column_gap: px(11), ..default()
                }).with_children(|main| {
                    main.spawn((ScoreCrest, ImageNode::new(default_crest.clone()), Node {
                        width: px(48), height: px(48), ..default()
                    }));
                    main.spawn((ScoreField::Team, Text::new("IND"), TextFont {
                        font: display.clone(), font_size: 22.0, ..default()
                    }, TextColor(Color::srgb(0.93, 0.95, 0.97)), text_shadow(), Node {
                        width: px(52), ..default()
                    }));
                    main.spawn((ScoreField::Runs, Text::new("0/0"), TextFont {
                        font: display.clone(), font_size: 39.0, ..default()
                    }, TextColor(Color::WHITE), text_shadow(), Node {
                        flex_grow: 1.0, ..default()
                    }));
                    main.spawn((Node {
                        height: px(31), padding: UiRect::horizontal(px(11)),
                        align_items: AlignItems::Center, border_radius: BorderRadius::all(px(3)),
                        ..default()
                    }, BackgroundColor(Color::srgba(0.20, 0.23, 0.29, 0.92))))
                    .with_children(|overs| {
                        overs.spawn((ScoreField::Overs, Text::new("0.0 OV"), TextFont {
                            font: bold.clone(), font_size: 14.0, ..default()
                        }, TextColor(Color::srgb(0.94, 0.95, 0.97))));
                    });
                });
                c.spawn((ScoreAccent, Node {
                    width: percent(100), height: px(27), align_items: AlignItems::Center,
                    padding: UiRect::horizontal(px(12)), ..default()
                }, BackgroundColor(Color::srgb(0.08, 0.38, 0.82))))
                .with_children(|ribbon| {
                    ribbon.spawn((ScoreField::Equation, Text::new("CURRENT RUN RATE  0.00"), TextFont {
                        font: bold.clone(), font_size: 11.0, ..default()
                    }, TextColor(Color::WHITE), text_shadow()));
                });
                c.spawn((Node {
                    width: percent(100), height: px(31), align_items: AlignItems::Center,
                    padding: UiRect::horizontal(px(12)), ..default()
                }, BackgroundColor(Color::srgba(0.075, 0.09, 0.12, 0.98))))
                .with_children(|batters| {
                    batters.spawn((ScoreField::Batters, Text::new(""), TextFont {
                        font: bold.clone(), font_size: 13.0, ..default()
                    }, TextColor(Color::srgb(0.92, 0.94, 0.96))));
                });
                c.spawn((Node {
                    width: percent(100), height: px(29), align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                    padding: UiRect::horizontal(px(12)), ..default()
                }, BackgroundColor(Color::srgba(0.035, 0.045, 0.065, 0.98))))
                .with_children(|delivery| {
                    delivery.spawn((ScoreField::Bowler, Text::new(""), TextFont {
                        font: regular.clone(), font_size: 12.0, ..default()
                    }, TextColor(Color::srgb(0.77, 0.81, 0.85))));
                    delivery.spawn((ScoreField::Delivery, Text::new(""), TextFont {
                        font: bold.clone(), font_size: 11.0, ..default()
                    }, TextColor(Color::srgb(0.98, 0.72, 0.25))));
                });
            });

            // ---- result sting, matching a replay/boundary television graphic ----
            p.spawn((
                OutcomeRoot,
                Node {
                    position_type: PositionType::Absolute,
                    top: percent(27),
                    width: percent(100),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                Visibility::Hidden,
            )).with_children(|root| {
                root.spawn((OutcomePanel, Node {
                    min_width: px(390), height: px(76), align_items: AlignItems::Stretch,
                    border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(5)),
                    overflow: Overflow::clip(), ..default()
                }, BackgroundColor(Color::srgba(0.08, 0.055, 0.015, 0.96)),
                BorderColor::all(Color::srgba(1.0, 0.78, 0.30, 0.55))))
                .with_children(|panel| {
                    panel.spawn((OutcomeAccent, Node { width: px(7), height: percent(100), ..default() },
                        BackgroundColor(Color::srgb(0.98, 0.76, 0.24))));
                    panel.spawn((Node {
                        width: px(96), height: percent(100), align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center, ..default()
                    }, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.22))))
                    .with_children(|kind| {
                        kind.spawn((OutcomeField::Kind, Text::new("BOUNDARY"), TextFont {
                            font: bold.clone(), font_size: 11.0, ..default()
                        }, TextColor(Color::srgb(0.98, 0.76, 0.24))));
                    });
                    panel.spawn(Node {
                        flex_grow: 1.0, height: percent(100), align_items: AlignItems::Center,
                        padding: UiRect::horizontal(px(22)), ..default()
                    }).with_children(|message| {
                        message.spawn((OutcomeField::Message, Text::new("FOUR"), TextFont {
                            font: display.clone(), font_size: 31.0, ..default()
                        }, TextColor(Color::WHITE), text_shadow()));
                    });
                });
            });

            // ---- lower-third control / phase prompt ----
            p.spawn((
                PromptRoot,
                Node {
                    position_type: PositionType::Absolute,
                    bottom: px(24),
                    width: percent(100),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                Visibility::Hidden,
            )).with_children(|root| {
                root.spawn((Node {
                    min_width: px(520), height: px(39), align_items: AlignItems::Stretch,
                    border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(4)),
                    overflow: Overflow::clip(), ..default()
                }, BackgroundColor(Color::srgba(0.02, 0.03, 0.045, 0.94)),
                BorderColor::all(Color::srgba(0.78, 0.84, 0.90, 0.30))))
                .with_children(|panel| {
                    panel.spawn((Node {
                        width: px(112), align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center, ..default()
                    }, BackgroundColor(Color::srgb(0.76, 0.55, 0.16))))
                    .with_children(|kind| {
                        kind.spawn((PromptField::Kind, Text::new("BATTING"), TextFont {
                            font: bold.clone(), font_size: 11.0, ..default()
                        }, TextColor(Color::srgb(0.04, 0.05, 0.06))));
                    });
                    panel.spawn(Node {
                        flex_grow: 1.0, align_items: AlignItems::Center,
                        padding: UiRect::horizontal(px(17)), ..default()
                    }).with_children(|message| {
                        message.spawn((PromptField::Message, Text::new(""), TextFont {
                            font: bold.clone(), font_size: 13.0, ..default()
                        }, TextColor(Color::srgb(0.94, 0.96, 0.98))));
                    });
                });
            });

            // ---- match summary panel (only on MatchOver) ----
            p.spawn((
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
                    Text::new(""),
                    TextFont { font: bold.clone(), font_size: 20.0, ..default() },
                    TextColor(Color::WHITE),
                    TextLayout::new_with_justify(Justify::Center),
                ));
            });

            // ---- timing meter ----
            p.spawn((
                MeterRoot,
                Node {
                    position_type: PositionType::Absolute,
                    bottom: percent(13),
                    left: percent(35),
                    width: percent(30),
                    height: px(26),
                    ..default()
                },
                panel_bg(),
                Visibility::Hidden,
            ))
            .with_children(|m| {
                // sweet-spot band around centre
                m.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: percent(43),
                        width: percent(14),
                        height: percent(100),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.2, 0.8, 0.3, 0.45)),
                ));
                m.spawn((
                    MeterMarker,
                    Node {
                        position_type: PositionType::Absolute,
                        top: px(0),
                        left: px(0),
                        width: px(6),
                        height: percent(100),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(1.0, 0.95, 0.4)),
                ));
            });
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
    assets: Res<AssetServer>,
    mut text_q: Query<(&ScoreField, &mut Text)>,
    mut accent_q: Query<&mut BackgroundColor, With<ScoreAccent>>,
    mut crest_q: Query<&mut ImageNode, With<ScoreCrest>>,
) {
    let Some(am) = am else { return };
    let inns = &am.state.innings;
    let bat = am.batting_team(&wd);
    if bat.players.get(inns.striker).is_none()
        || bat.players.get(inns.non_striker).is_none()
    {
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
            format!(
                "TARGET {tg}   •   NEED {need} FROM {balls_left}   •   RRR {required:.2}"
            )
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
        s.name.to_uppercase(), sc.runs, sc.balls,
        ns.name.to_uppercase(), nc.runs, nc.balls,
    );
    let delivery = del
        .0
        .as_ref()
        .map(|plan| plan.label.to_uppercase())
        .unwrap_or_default();

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
        };
    }
    for mut background in &mut accent_q {
        background.0 = bat.primary_color;
    }
    if let Ok(mut crest) = crest_q.single_mut() {
        *crest = ImageNode::new(crate::render::load_team_crest(
            &assets,
            &bat.crest_asset(),
        ));
    }
}

fn update_prompt(
    phase: Res<Phase>,
    am: Option<Res<ActiveMatch>>,
    mut q: Query<&mut Text, (With<PromptText>, Without<ScoreText>)>,
) {
    let Ok(mut t) = q.single_mut() else { return };
    **t = match &phase.0 {
        PhaseEnum::ReadyToBall { .. } => match am.as_deref().map(|m| m.user_bowling()) {
            Some(true) => "SPACE / A: start your run-up".into(),
            _ => String::new(),
        },
        PhaseEnum::AimLength { lock, .. } => match lock {
            None => "SPACE: lock LENGTH  (marker sweeps down the pitch)".into(),
            Some(_) => "SPACE: lock LINE  (marker sweeps across)".into(),
        },
        PhaseEnum::RunUp { p } => {
            if *p < 1.0 { "Bowler running in...".into() } else { String::new() }
        }
        PhaseEnum::BallLive => {
            let user_batting =
                am.as_deref().map(|m| m.user_batting()).unwrap_or(false);
            if user_batting {
                "SPACE: play shot | SHIFT+SPACE: loft | A/D: aim".into()
            } else {
                String::new()
            }
        }
        PhaseEnum::OverBreak { .. } => "Over complete. Next bowler coming on...".into(),
        PhaseEnum::InningsBreak => {
            "INNINGS BREAK — SPACE / A to begin the chase".into()
        }
        PhaseEnum::MatchOver => "Match over — SPACE / A: continue".into(),
        _ => String::new(),
    };
}

fn update_outcome(
    phase: Res<Phase>,
    mut q: Query<(&mut Text, &mut Visibility), With<OutcomeText>>,
) {
    let Ok((mut t, mut vis)) = q.single_mut() else { return };
    match &phase.0 {
        PhaseEnum::ResultPause { text, .. } => {
            **t = text.clone();
            *vis = Visibility::Visible;
        }
        _ => *vis = Visibility::Hidden,
    }
}

fn update_meter(
    phase: Res<Phase>,
    am: Option<Res<ActiveMatch>>,
    rel: Res<ReleaseInfo>,
    attempt: Res<ShotAttempt>,
    mut root_q: Query<(&mut Visibility, &mut Node), With<MeterRoot>>,
    mut marker_q: Query<&mut Node, (With<MeterMarker>, Without<MeterRoot>)>,
) {
    let Ok((mut vis, _)) = root_q.single_mut() else { return };
    let show = matches!(phase.0, PhaseEnum::BallLive)
        && am.as_deref().map(|m| m.user_batting()).unwrap_or(false)
        && rel.active;
    *vis = if show { Visibility::Visible } else { Visibility::Hidden };
    if !show {
        return;
    }
    let offset = attempt.offset.unwrap_or(rel.t - rel.t_arrive);
    let frac = ((offset / METER_WINDOW) + 0.5).clamp(0.0, 1.0);
    if let Ok(mut node) = marker_q.single_mut() {
        node.left = percent(frac * 100.0);
    }
}

fn update_summary(
    phase: Res<Phase>,
    am: Option<Res<ActiveMatch>>,
    wd: Res<WorldData>,
    mut root_q: Query<&mut Visibility, With<SummaryRoot>>,
    mut text_q: Query<&mut Text, With<SummaryText>>,
) {
    let Ok(mut vis) = root_q.single_mut() else { return };
    let Ok(mut txt) = text_q.single_mut() else { return };
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
            format!("{name} {}/{} ({}.{})", b.wickets, b.runs, b.balls / 6, b.balls % 6)
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

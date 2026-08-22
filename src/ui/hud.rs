//! In-match HUD: scoreboard, batter/bowler stats, phase prompts, outcome
//! banner and the shot-timing meter.

use crate::game::*;
use crate::state::AppState;
use bevy::prelude::*;

#[derive(Component)]
struct HudRoot;
#[derive(Component)]
struct ScoreText;
#[derive(Component)]
struct InfoText;
#[derive(Component)]
struct PromptText;
#[derive(Component)]
struct OutcomeText;
#[derive(Component)]
struct MeterRoot;
#[derive(Component)]
struct MeterMarker;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::InMatch), spawn_hud)
            .add_systems(OnExit(AppState::InMatch), despawn_hud)
            .add_systems(
                Update,
                (
                    update_scoreboard,
                    update_prompt,
                    update_outcome,
                    update_meter,
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

fn spawn_hud(mut commands: Commands) {
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
            // ---- scoreboard, top-left ----
            p.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    top: px(12),
                    left: px(12),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(px(10)),
                    row_gap: px(4),
                    ..default()
                },
                panel_bg(),
            ))
            .with_children(|c| {
                c.spawn((
                    ScoreText,
                    Text::new(""),
                    TextFont { font_size: 34.0, ..default() },
                    TextColor(Color::WHITE),
                ));
                c.spawn((
                    InfoText,
                    Text::new(""),
                    TextFont { font_size: 17.0, ..default() },
                    TextColor(Color::srgb(0.85, 0.85, 0.9)),
                ));
            });

            // ---- big centre outcome banner ----
            p.spawn((
                OutcomeText,
                Node {
                    position_type: PositionType::Absolute,
                    top: percent(28),
                    width: percent(100),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                Text::new(""),
                TextFont { font_size: 54.0, ..default() },
                TextColor(Color::srgb(1.0, 0.9, 0.3)),
                Visibility::Hidden,
            ));

            // ---- bottom prompt ----
            p.spawn((
                PromptText,
                Node {
                    position_type: PositionType::Absolute,
                    bottom: percent(7),
                    width: percent(100),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                Text::new(""),
                TextFont { font_size: 21.0, ..default() },
                TextColor(Color::WHITE),
            ));

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
    mut score_q: Query<&mut Text, With<ScoreText>>,
    mut info_q: Query<
        &mut Text,
        (With<InfoText>, Without<ScoreText>, Without<PromptText>),
    >,
) {
    let Some(am) = am else { return };
    let Ok(mut score) = score_q.single_mut() else { return };
    let inns = &am.state.innings;
    let bat = am.batting_team(&wd);
    **score = format!(
        "{}  {}/{}   ({}.{} ov)",
        bat.short,
        inns.runs,
        inns.wickets,
        inns.legal_balls / 6,
        inns.legal_balls % 6,
    );

    let Ok(mut info) = info_q.single_mut() else { return };
    let team = bat;
    if team.players.get(inns.striker).is_none()
        || team.players.get(inns.non_striker).is_none()
    {
        return;
    }
    let s = &team.players[inns.striker];
    let ns = &team.players[inns.non_striker];
    let sc = inns.card_of(inns.striker);
    let nc = inns.card_of(inns.non_striker);

    let bowler_line = match inns.current_bowler {
        Some(b) => {
            let bp = &am.fielding_team(&wd).players[b];
            let bc = inns.bowler_card_of(b);
            format!(
                "Bowling: {} [{}]  {}/{} ({}.{})",
                bp.name,
                bp.style.map_or("-", |st| st.label()),
                bc.wickets,
                bc.runs,
                bc.balls / 6,
                bc.balls % 6
            )
        }
        None => String::new(),
    };

    let target_line = match inns.target {
        Some(tg) => {
            let need = tg.saturating_sub(inns.runs) as f32;
            let overs_left = ((am.state.overs * 6).saturating_sub(inns.legal_balls) as f32 / 6.0).max(0.1);
            format!(
                "  |  Target {} · need {:.0} · RR req {:.2}",
                tg, tg.saturating_sub(inns.runs), need / overs_left
            )
        }
        None => format!("  |  CRR {:.2}", inns.run_rate()),
    };

    **info = format!(
        "*{} {}({})   {} {}({})\n{}\n{}{}",
        s.name,
        sc.runs,
        sc.balls,
        ns.name,
        nc.runs,
        nc.balls,
        bowler_line,
        del.0.as_ref().map_or(String::new(), |p| p.label.clone()),
        target_line,
    );
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

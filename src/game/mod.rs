pub mod audio;
pub mod ball;
pub mod fielding;
pub mod match_flow;

use crate::core::rules::{BallOutcome, MatchState, Progression};
use crate::core::stadiums::{PitchType, Stadium};
use crate::core::teams::{Player, Team, batting_order, pick_bowlers};
use bevy::prelude::*;

/// All static content (teams, stadiums).
#[derive(Resource)]
pub struct WorldData {
    pub teams: Vec<Team>,
    pub stadiums: Vec<Stadium>,
}

impl WorldData {
    pub fn new() -> Self {
        WorldData {
            teams: crate::core::teams::builtin_teams(),
            stadiums: crate::core::stadiums::builtin_stadiums(),
        }
    }
}

/// Configuration captured by menus before starting a match.
#[derive(Resource, Clone)]
pub struct MatchSetup {
    /// Indices into WorldData.teams: [user_team, opponent].
    pub teams: [usize; 2],
    pub stadium: usize,
    pub overs: u32,
    /// true = the local player's team bats first.
    pub user_bats_first: bool,
    /// true for a tournament fixture (auto opponent/stadium).
    pub from_tournament: bool,
}

/// Live match session.
#[derive(Resource)]
pub struct ActiveMatch {
    pub state: MatchState,
    pub stadium: usize,
    /// The local player's team (index into WorldData.teams), if any.
    pub user_team: Option<usize>,
    /// Player index (within the fielding team) currently bowling.
    pub bowler_player: usize,
}

impl ActiveMatch {
    pub fn batting_team<'w>(&self, wd: &'w WorldData) -> &'w Team {
        &wd.teams[self.state.teams[0]]
    }

    pub fn fielding_team<'w>(&self, wd: &'w WorldData) -> &'w Team {
        &wd.teams[self.state.teams[1]]
    }

    pub fn striker<'w>(&self, wd: &'w WorldData) -> &'w Player {
        let t = self.batting_team(wd);
        &t.players[self.state.innings.striker]
    }

    pub fn bowler<'w>(&self, wd: &'w WorldData) -> &'w Player {
        &self.fielding_team(wd).players[self.bowler_player]
    }

    pub fn pitch(&self, wd: &WorldData) -> PitchType {
        wd.stadiums[self.stadium].pitch
    }

    pub fn user_batting(&self) -> bool {
        self.user_team == Some(self.state.teams[0])
    }

    pub fn user_bowling(&self) -> bool {
        self.user_team == Some(self.state.teams[1])
    }
}

/// High-level phase of the current delivery cycle / match flow.
#[derive(Resource, Default)]
pub struct Phase(pub PhaseEnum);

#[derive(Default, Clone, Debug)]
pub enum PhaseEnum {
    #[default]
    Idle,
    /// Waiting for the next delivery.
    ReadyToBall {
        t: f32,
    },
    /// Human bowling: choosing length then line.
    AimLength {
        t: f32,
        lock: Option<f32>,
    },
    /// Bowler running in. `p` 0..1.
    RunUp {
        p: f32,
    },
    /// Ball has been released and is travelling.
    BallLive,
    /// Showing the outcome of the last ball.
    ResultPause {
        t: f32,
        text: String,
    },
    /// Between overs: pick next bowler.
    OverBreak {
        t: f32,
    },
    InningsBreak,
    MatchOver,
}

impl PhaseEnum {
    pub fn is_live(&self) -> bool {
        matches!(self, PhaseEnum::BallLive)
    }
}

/// Parameters of the delivery being bowled.
#[derive(Clone, Debug)]
pub struct DeliveryPlan {
    /// Release speed m/s (already playability-scaled).
    pub speed: f32,
    /// Lateral position (world z) the ball passes the striker's stumps.
    pub line_z: f32,
    /// Distance before the striker's stumps where the ball bounces.
    pub length_from_stumps: f32,
    /// Constant lateral acceleration pre-bounce (swing), m/s^2.
    pub swing: f32,
    /// Instantaneous lateral velocity change at the bounce (spin), m/s.
    pub turn: f32,
    pub label: String,
    /// True when this delivery is so far wide it should be called wide.
    pub wide: bool,
}

impl DeliveryPlan {
    pub fn quality_vs_batsman(&self) -> f32 {
        // 0 = terrible line/length, 1 = unplayable.
        let line_pen = (self.line_z.abs() - 0.25).max(0.0) * 1.5;
        let len_ideal = 7.5_f32;
        let len_pen = ((self.length_from_stumps - len_ideal).abs() / 9.0).min(1.0);
        (1.0 - line_pen.min(1.0) * 0.7 - len_pen * 0.6).clamp(0.05, 1.0)
    }
}

/// Boundary radius of the current stadium (updated on scene build).
#[derive(Resource)]
pub struct BoundaryRadius(pub f32);

/// Delivery currently in flight / last released.
#[derive(Resource, Default)]
pub struct CurrentDelivery(pub Option<DeliveryPlan>);

/// Marker for the aim marker mesh shown during human bowling.
#[derive(Component)]
pub struct AimMarker;

/// Scripted outcome of a struck/stopped ball, applied when its timer
/// expires or the physical ball reaches its destination sooner.
#[derive(Resource, Default)]
pub struct Pending(pub Option<PendingOutcome>);

#[derive(Clone)]
pub struct PendingOutcome {
    pub outcome: BallOutcome,
    pub text: String,
    pub apply_in: f32,
    /// Seconds since the ball was struck (drives runner animation).
    pub elapsed: f32,
    /// Total runs the batters will complete (for runner animation).
    pub runs_anim: u32,
    /// Fielder slot chasing the ball (if any).
    pub chaser_slot: Option<usize>,
    pub boundary: bool,
    pub aerial_catch: bool,
}

/// Timing reference for the live delivery.
#[derive(Resource, Default)]
pub struct ReleaseInfo {
    pub active: bool,
    /// Contact/miss already decided for this delivery.
    pub resolved: bool,
    /// Seconds since release.
    pub t: f32,
    /// Predicted arrival at bat plane (real seconds).
    pub t_arrive: f32,
}

/// A shot attempt registered during the current delivery.
#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct ShotAttempt {
    pub pressed: bool,
    /// Seconds-since-release at press time (negative = early swing).
    pub offset: Option<f32>,
    pub loft: bool,
    pub dir_x: f32,
    /// Set when an AI batter decides to swing.
    pub ai_scheduled: bool,
}

/// Events fired when innings/match progression changes.
#[derive(Message)]
pub struct ProgressionEvt(pub Progression);

/// Rolling last-six delivery symbols for the broadcast scorebug.
#[derive(Resource, Default)]
pub struct RecentBalls {
    pub entries: std::collections::VecDeque<String>,
}

impl RecentBalls {
    pub fn push_outcome(&mut self, outcome: &BallOutcome) {
        self.entries.push_back(outcome_symbol(outcome));
        while self.entries.len() > 6 {
            self.entries.pop_front();
        }
    }

    pub fn display(&self) -> String {
        if self.entries.is_empty() {
            return "—  —  —  —  —  —".into();
        }
        let mut slots = ["—"; 6];
        let start = 6usize.saturating_sub(self.entries.len());
        for (i, sym) in self.entries.iter().enumerate() {
            slots[start + i] = sym.as_str();
        }
        slots.join("  ")
    }
}

/// Compact ball-history glyph for the scorebug strip.
pub fn outcome_symbol(outcome: &BallOutcome) -> String {
    match outcome {
        BallOutcome::Runs(0) => "•".into(),
        BallOutcome::Runs(n) => n.to_string(),
        BallOutcome::Four => "4".into(),
        BallOutcome::Six => "6".into(),
        BallOutcome::Wide => "Wd".into(),
        BallOutcome::NoBall => "Nb".into(),
        BallOutcome::Wicket(_) | BallOutcome::WicketAndRuns(_, _) => "W".into(),
    }
}

pub struct GameplayPlugin;

impl Plugin for GameplayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Phase>()
            .init_resource::<Pending>()
            .init_resource::<ReleaseInfo>()
            .init_resource::<ShotAttempt>()
            .init_resource::<RecentBalls>()
            .init_resource::<crate::render::camera_rig::BallRecording>()
            .init_resource::<crate::render::camera_rig::ReplayState>()
            .init_resource::<crate::render::camera_rig::PresentationState>();
    }
}

/// Whether the local player's team bats first after the toss.
pub fn user_bats_first_from_toss(user_team: usize, toss_winner: usize, elects_bat: bool) -> bool {
    if toss_winner == user_team {
        elects_bat
    } else {
        !elects_bat
    }
}

/// Build a fresh ActiveMatch from setup info.
pub fn build_active_match(setup: &MatchSetup, wd: &WorldData) -> ActiveMatch {
    let team_a = &wd.teams[setup.teams[0]];
    let team_b = &wd.teams[setup.teams[1]];
    let (bat_first, bowl_first) = if setup.user_bats_first {
        (team_a, team_b)
    } else {
        (team_b, team_a)
    };
    let order = batting_order(bat_first);
    let bowlers = pick_bowlers(bowl_first, 5);
    let mut state = MatchState::new(
        setup.overs,
        if setup.user_bats_first {
            [setup.teams[0], setup.teams[1]]
        } else {
            [setup.teams[1], setup.teams[0]]
        },
        order,
        &bowlers,
    );
    state.innings.current_bowler = bowlers.first().copied();
    ActiveMatch {
        state,
        stadium: setup.stadium,
        user_team: Some(setup.teams[0]),
        bowler_player: bowlers[0],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_bats_first_when_user_wins_and_elects_bat() {
        assert!(user_bats_first_from_toss(2, 2, true));
        assert!(!user_bats_first_from_toss(2, 2, false));
    }

    #[test]
    fn user_bats_first_when_opposition_wins_and_elects_bowl() {
        assert!(user_bats_first_from_toss(2, 5, false));
        assert!(!user_bats_first_from_toss(2, 5, true));
    }
}

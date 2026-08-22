/// Tournament structures: a multi-team knockout cup played across
/// different stadiums, with quick-sim for AI vs AI matches.
use super::rules::{MatchState, Result as MatchResult};
use super::teams::{batting_order, pick_bowlers, team_rating, Team};
use super::{stadiums::Stadium, teams};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Semifinal1,
    Semifinal2,
    Final,
}

impl Stage {
    pub fn label(&self) -> &'static str {
        match self {
            Stage::Semifinal1 => "Semi-Final 1",
            Stage::Semifinal2 => "Semi-Final 2",
            Stage::Final => "GRAND FINAL",
        }
    }
    pub fn next(&self) -> Option<Stage> {
        match self {
            Stage::Semifinal1 | Stage::Semifinal2 => Some(Stage::Final),
            Stage::Final => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Fixture {
    pub stage: Stage,
    /// Indices into `teams`.
    pub home: usize,
    pub away: usize,
    pub stadium: usize,
    pub overs: u32,
    pub result: Option<MatchResult>,
    /// True when the local player took part (played, not simulated).
    pub played_by_user: bool,
}

impl Fixture {
    pub fn winner(&self) -> Option<usize> {
        match self.result.clone()? {
            MatchResult::Win { winner, .. } => Some(winner),
            MatchResult::Tie => Some(self.home), // tie: home advances
        }
    }
}

#[derive(Clone, Debug)]
pub struct Tournament {
    pub name: String,
    pub teams: Vec<Team>,
    pub stadiums: Vec<Stadium>,
    pub fixtures: Vec<Fixture>,
    pub user_team: Option<usize>,
}

impl Tournament {
    /// 4-team knockout. `user_team` selects the player-controlled side.
    pub fn knockout(
        mut entrants: Vec<Team>,
        stadiums: Vec<Stadium>,
        user_team: Option<String>,
    ) -> Tournament {
        // Shuffle deterministically-ish by rating spread for variety.
        entrants.sort_by_key(|t| (team_rating(t) * 10.0) as i64);
        let n = entrants.len();
        let user_idx = user_team.as_ref().and_then(|name| {
            entrants.iter().position(|t| &t.name == name)
        });
        let mk = |stage, a, b, stad| Fixture {
            stage,
            home: a,
            away: b,
            stadium: stad,
            overs: 20,
            result: None,
            played_by_user: false,
        };
        Tournament {
            name: format!("{}-Team Championship", n),
            fixtures: vec![
                mk(Stage::Semifinal1, 0, 3, 0),
                mk(Stage::Semifinal2, 1, 2, 1),
                mk(Stage::Final, 0, 1, 2), // placeholders; filled on advance
            ],
            teams: entrants,
            stadiums,
            user_team: user_idx,
        }
    }

    /// The fixture the user should play next, if any remain.
    pub fn next_user_fixture(&self) -> Option<(usize, Fixture)> {
        if self.user_team.is_none() {
            return None;
        }
        self.fixtures.iter().position(|f| {
            f.result.is_none()
                && (Some(f.home) == self.user_team
                    || Some(f.away) == self.user_team)
        }).map(|i| (i, self.fixtures[i].clone()))
    }

    /// Next fixture needing a result at all (for sim-to-next-user-match).
    pub fn pending_fixtures(&self) -> Vec<(usize, Fixture)> {
        self.fixtures
            .iter()
            .enumerate()
            .filter(|(_, f)| f.result.is_none())
            .map(|(i, f)| (i, f.clone()))
            .collect()
    }

    /// Record a finished match and propagate winners into later rounds.
    pub fn record_result(&mut self, idx: usize, state: &MatchState,
                         played_by_user: bool) {
        let f = &mut self.fixtures[idx];
        f.result = state.result.clone();
        f.played_by_user = played_by_user;
        self.propagate();
    }

    fn propagate(&mut self) {
        let s1 = self.fixtures.iter().find(|f| f.stage == Stage::Semifinal1)
            .and_then(|f| f.winner());
        let s2 = self.fixtures.iter().find(|f| f.stage == Stage::Semifinal2)
            .and_then(|f| f.winner());
        if let (Some(a), Some(b)) = (s1, s2) {
            let fin = self.fixtures.iter_mut()
                .find(|f| f.stage == Stage::Final).unwrap();
            fin.home = a;
            fin.away = b;
            // Pick a stadium different from semis when possible.
            fin.stadium = 2 % self.stadiums.len();
        }
    }

    pub fn champion(&self) -> Option<usize> {
        let final_f = self.fixtures.iter().find(|f| f.stage == Stage::Final)?;
        final_f.winner().filter(|_| final_f.result.is_some())
    }

    /// Fast statistical simulation of an innings for AI-vs-AI matches.
    /// Returns total runs for the batting team.
    pub fn quick_sim_innings(&self, bat_team: &Team, bowl_team: &Team,
                             seed: u64, overs: u32) -> (u32, u32) {
        use teams::hash_f32;
        let order = batting_order(bat_team);
        let bowlers = pick_bowlers(bowl_team, 5);
        let mut runs = 0u32;
        let mut wickets = 0u32;
        let mut striker = 0usize; // slot in order
        let mut non_striker = 1usize;
        let mut next_slot = 2usize;
        let mut balls = 0u32;
        while balls < overs * 6 && wickets < 10 && next_slot <= 10 {
            let b = &bowlers[(balls / 6) as usize % bowlers.len()];
            let bowler = &bowl_team.players[*b];
            let batter = &bat_team.players[order[striker]];
            let r = hash_f32(seed.wrapping_add((balls as u64) << 8));
            // Probability model driven by ratings.
            let bat_skill = batter.batting as f32 / 100.0;
            let bowl_skill = bowler.bowling as f32 / 100.0;
            let edge = 0.06 + 0.10 * (bowl_skill - bat_skill + 0.5);
            if r < edge.max(0.02) {
                wickets += 1;
                if next_slot <= 10 {
                    striker = next_slot;
                    next_slot += 1;
                }
            } else {
                let r2 = hash_f32(seed ^ ((balls as u64).wrapping_mul(7919)));
                let aggressive = bat_skill.clamp(0.4, 0.95);
                let runs_this_ball = if r2 < 0.08 * aggressive { 6 }
                    else if r2 < 0.22 * aggressive { 4 }
                    else if r2 < 0.55 { 1 + (r2 * 10.0) as u32 % 2 }
                    else { 0 };
                runs += runs_this_ball;
                if runs_this_ball % 2 == 1 {
                    std::mem::swap(&mut striker, &mut non_striker);
                }
            }
            balls += 1;
            if balls % 6 == 0 {
                std::mem::swap(&mut striker, &mut non_striker);
            }
        }
        (runs, wickets)
    }

    /// Simulate an entire fixture without playing it.
    pub fn sim_fixture(&mut self, idx: usize, seed: u64) {
        let f = self.fixtures[idx].clone();
        let overs = f.overs;
        let t1 = self.teams[f.home].clone();
        let t2 = self.teams[f.away].clone();
        let (r1, _w1) = self.quick_sim_innings(&t1, &t2, seed, overs);
        let (r2, _w2) = self.quick_sim_innings(&t2, &t1, seed ^ 0xBEEF, overs);
        let result = if r1 > r2 {
            MatchResult::Win { winner: f.home, margin: format!("won by {} runs", r1 - r2) }
        } else if r2 > r1 {
            MatchResult::Win { winner: f.away, margin: format!("won by {} wickets", 10) }
        } else {
            MatchResult::Tie
        };
        self.fixtures[idx].result = Some(result);
        self.propagate();
    }
}

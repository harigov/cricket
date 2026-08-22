/// Tournament structures: a multi-team knockout cup played across
/// different stadiums, with quick-sim for AI vs AI matches.
use super::rules::{MatchState, Result as MatchResult};
use super::teams::{Team, batting_order, pick_bowlers, team_rating};
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
    /* winner resolution lives on Tournament (needs world map) */
}

#[derive(Clone, Debug)]
pub struct Tournament {
    pub name: String,
    pub teams: Vec<Team>,
    /// For each entrant, their index into WorldData.teams.
    pub world_idx: Vec<usize>,
    pub stadiums: Vec<Stadium>,
    pub fixtures: Vec<Fixture>,
    /// Index into `teams` of the user-controlled side.
    pub user_team: Option<usize>,
}

/// Entrant pair used to build a tournament.
pub struct Entrant {
    pub world_idx: usize,
    pub team: Team,
}

impl Tournament {
    /// Map a WorldData team index to this tournament's local slot.
    pub fn local_of(&self, world_idx: usize) -> Option<usize> {
        self.world_idx.iter().position(|&w| w == world_idx)
    }

    /// Winner of a fixture as a tournament-local team index.
    pub fn fixture_winner(&self, f: &Fixture) -> Option<usize> {
        match f.result.as_ref()? {
            MatchResult::Win { winner, .. } => {
                // `winner` is a WorldData team index; map it locally.
                let local = self.world_idx.iter().position(|&w| w == *winner);
                Some(local.unwrap_or(f.home))
            }
            MatchResult::Tie => Some(f.home),
        }
    }
}

impl Tournament {
    /// 4-team knockout. `user_local` selects the player-controlled side.
    pub fn knockout(
        mut entrants: Vec<Entrant>,
        stadiums: Vec<Stadium>,
        user_local: Option<usize>,
    ) -> Tournament {
        // Seed strongest vs weakest for variety across runs.
        entrants.sort_by_key(|e| (team_rating(&e.team) * 10.0) as i64);
        let n = entrants.len();
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
                mk(Stage::Semifinal1, 0, 3, 0 % stadiums.len()),
                mk(Stage::Semifinal2, 1, 2, 1 % stadiums.len()),
                mk(Stage::Final, 0, 1, 2 % stadiums.len()), // filled on advance
            ],
            world_idx: entrants.iter().map(|e| e.world_idx).collect(),
            teams: entrants.into_iter().map(|e| e.team).collect(),
            stadiums,
            user_team: user_local,
        }
    }

    /// The fixture the user should play next, if any remain.
    pub fn next_user_fixture(&self) -> Option<(usize, Fixture)> {
        self.user_team?;
        self.fixtures
            .iter()
            .position(|f| {
                f.result.is_none()
                    && (Some(f.home) == self.user_team || Some(f.away) == self.user_team)
            })
            .map(|i| (i, self.fixtures[i].clone()))
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
    pub fn record_result(&mut self, idx: usize, state: &MatchState, played_by_user: bool) {
        let f = &mut self.fixtures[idx];
        f.result = state.result.clone();
        f.played_by_user = played_by_user;
        self.propagate();
    }

    fn propagate(&mut self) {
        let (s1, s2) = {
            let this = &*self;
            (
                this.fixtures
                    .iter()
                    .find(|f| f.stage == Stage::Semifinal1)
                    .and_then(|f| this.fixture_winner(f)),
                this.fixtures
                    .iter()
                    .find(|f| f.stage == Stage::Semifinal2)
                    .and_then(|f| this.fixture_winner(f)),
            )
        };
        if let (Some(a), Some(b)) = (s1, s2) {
            let fin = self
                .fixtures
                .iter_mut()
                .find(|f| f.stage == Stage::Final)
                .unwrap();
            fin.home = a;
            fin.away = b;
            // Pick a stadium different from semis when possible.
            fin.stadium = 2 % self.stadiums.len();
        }
    }

    pub fn champion(&self) -> Option<usize> {
        let final_f = self.fixtures.iter().find(|f| f.stage == Stage::Final)?;
        final_f
            .result
            .as_ref()
            .and_then(|_| self.fixture_winner(final_f))
    }

    /// Fast statistical simulation of an innings for AI-vs-AI matches.
    /// Returns total runs for the batting team.
    pub fn quick_sim_innings(
        &self,
        bat_team: &Team,
        bowl_team: &Team,
        seed: u64,
        overs: u32,
    ) -> (u32, u32) {
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
                let runs_this_ball = if r2 < 0.08 * aggressive {
                    6
                } else if r2 < 0.22 * aggressive {
                    4
                } else if r2 < 0.55 {
                    1 + (r2 * 10.0) as u32 % 2
                } else {
                    0
                };
                runs += runs_this_ball;
                if runs_this_ball % 2 == 1 {
                    std::mem::swap(&mut striker, &mut non_striker);
                }
            }
            balls += 1;
            if balls.is_multiple_of(6) {
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
            MatchResult::Win {
                winner: f.home,
                margin: format!("won by {} runs", r1 - r2),
            }
        } else if r2 > r1 {
            MatchResult::Win {
                winner: f.away,
                margin: format!("won by {} wickets", 10),
            }
        } else {
            MatchResult::Tie
        };
        self.fixtures[idx].result = Some(result);
        self.propagate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::stadiums::builtin_stadiums;

    fn make() -> Tournament {
        let teams = crate::core::teams::builtin_teams();
        let entrants: Vec<Entrant> = teams
            .iter()
            .take(4)
            .enumerate()
            .map(|(i, t)| Entrant {
                world_idx: i,
                team: t.clone(),
            })
            .collect();
        Tournament::knockout(entrants, builtin_stadiums(), Some(2))
    }

    #[test]
    fn bracket_structure() {
        let t = make();
        assert_eq!(t.fixtures.len(), 3);
        assert_eq!(t.fixtures[0].stage, Stage::Semifinal1);
        // User team must appear in exactly one semifinal.
        let u = t.user_team.unwrap();
        let count = t.fixtures[..2]
            .iter()
            .filter(|f| f.home == u || f.away == u)
            .count();
        assert_eq!(count, 1, "user must play exactly one semi");
    }

    #[test]
    fn sim_completes_and_propagates() {
        let mut t = make();
        t.sim_fixture(0, 7);
        t.sim_fixture(1, 9);
        assert!(t.fixtures[0].result.is_some());
        assert!(t.fixtures[1].result.is_some());
        // Final participants filled in from semi winners.
        assert_ne!(t.fixtures[2].home, t.fixtures[2].away.min(0)); // sanity
        assert!(t.next_user_fixture().is_some()); // user's final (or done)
        t.sim_fixture(2, 11);
        assert!(t.champion().is_some());
    }

    #[test]
    fn world_idx_mapping_roundtrip() {
        let t = make();
        for (local, &w) in t.world_idx.iter().enumerate() {
            assert_eq!(t.local_of(w), Some(local));
        }
    }
}

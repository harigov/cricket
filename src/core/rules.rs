/// Match rules & bookkeeping: scorecards, strike rotation, over/inning
/// progression and results. Pure domain logic, no engine dependencies.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dismissal {
    Bowled,
    Caught { fielder: usize },
    CaughtBehind { keeper: bool },
    Lbw,
    RunOut,
    Stumped,
    HitWicket,
}

impl Dismissal {
    pub fn label(&self) -> String {
        match self {
            Dismissal::Bowled => "b".into(),
            Dismissal::Lbw => "lbw b".into(),
            Dismissal::Caught { .. } => "c".into(),
            Dismissal::CaughtBehind { .. } => "c".into(),
            Dismissal::RunOut => "run out".into(),
            Dismissal::Stumped => "st".into(),
            Dismissal::HitWicket => "hw".into(),
        }
    }
}

/// What happened on one delivery (as reported by the gameplay layer).
#[derive(Clone, Debug)]
pub enum BallOutcome {
    /// 0..=6 running between wickets.
    Runs(u8),
    /// Clean boundary along the ground.
    Four,
    /// Cleared the rope.
    Six,
    Wide,
    NoBall,
    Wicket(Dismissal),
    /// Wicket AND runs scored (e.g. caught after running).
    WicketAndRuns(Dismissal, u8),
}

#[derive(Clone, Debug)]
pub struct BatterCard {
    pub player: usize,
    pub runs: u32,
    pub balls: u32,
    pub fours: u32,
    pub sixes: u32,
    pub out: Option<Dismissal>,
}

#[derive(Clone, Debug)]
pub struct BowlerCard {
    pub player: usize,
    pub balls: u32,
    pub runs: u32,
    pub wickets: u32,
}

#[derive(Clone, Debug)]
pub struct Innings {
    pub batting_team: usize,
    pub runs: u32,
    pub wickets: u32,
    pub legal_balls: u32,
    pub extras: u32,
    /// Player indices in batting order.
    pub order: Vec<usize>,
    pub striker: usize,
    pub non_striker: usize,
    /// Index into `order` of the next batter to walk in.
    pub next_batter_slot: usize,
    pub cards: Vec<BatterCard>,
    pub bowlers: Vec<BowlerCard>,
    pub current_bowler: Option<usize>,
    pub previous_bowler: Option<usize>,
    /// Balls in current over (legal ones), for over-end detection.
    balls_this_over: u32,
    /// Set for the chasing innings.
    pub target: Option<u32>,
}

impl Innings {
    pub fn new(
        batting_team: usize,
        order: Vec<usize>,
        target: Option<u32>,
        bowling_players: &[usize],
    ) -> Self {
        let cards = order
            .iter()
            .map(|&p| BatterCard { player: p, runs: 0, balls: 0, fours: 0, sixes: 0, out: None })
            .collect();
        let bowlers = bowling_players
            .iter()
            .map(|&p| BowlerCard { player: p, balls: 0, runs: 0, wickets: 0 })
            .collect();
        let striker = order[0];
        let non_striker = order[1];
        Innings {
            batting_team,
            runs: 0,
            wickets: 0,
            legal_balls: 0,
            extras: 0,
            order,
            striker,
            non_striker,
            next_batter_slot: 2,
            cards,
            bowlers,
            current_bowler: None,
            previous_bowler: None,
            balls_this_over: 0,
            target,
        }
    }

    pub fn overs_faced(&self) -> f32 {
        (self.legal_balls as f32) / 6.0
    }

    pub fn run_rate(&self) -> f32 {
        if self.legal_balls == 0 { 0.0 } else {
            self.runs as f32 * 6.0 / self.legal_balls as f32
        }
    }

    pub fn card_of(&self, player: usize) -> &BatterCard {
        self.cards.iter().find(|c| c.player == player).unwrap()
    }

    pub fn bowler_card_of(&self, player: usize) -> &BowlerCard {
        self.bowlers.iter().find(|c| c.player == player).unwrap()
    }

    fn card_mut(&mut self, player: usize) -> &mut BatterCard {
        self.cards.iter_mut().find(|c| c.player == player).unwrap()
    }

    fn bowler_card_mut(&mut self, player: usize) -> &mut BowlerCard {
        self.bowlers.iter_mut().find(|c| c.player == player).unwrap()
    }

    /// Apply a delivery. Returns a summary line for commentary/HUD.
    pub fn apply_ball(&mut self, outcome: &BallOutcome) -> String {
        match outcome {
            BallOutcome::Wide | BallOutcome::NoBall => {
                self.runs += 1;
                self.extras += 1;
                if let Some(b) = self.current_bowler {
                    self.bowler_card_mut(b).runs += 1;
                }
                match outcome {
                    BallOutcome::Wide => "Wide!".to_string(),
                    _ => "No ball!".to_string(),
                }
            }
            _ => {
                self.legal_balls += 1;
                self.balls_this_over += 1;
                if let Some(b) = self.current_bowler {
                    let bc = self.bowler_card_mut(b);
                    bc.balls += 1;
                }
                let mut note = String::new();
                match outcome {
                    BallOutcome::Wicket(d) => {
                        self.wicket(d, 0, &mut note);
                    }
                    BallOutcome::WicketAndRuns(d, r) => {
                        self.wicket(d, *r, &mut note);
                    }
                    BallOutcome::Four => {
                        self.runs += 4;
                        let c = self.card_mut(self.striker);
                        c.runs += 4; c.balls += 1; c.fours += 1;
                        note = "FOUR!".into();
                    }
                    BallOutcome::Six => {
                        self.runs += 6;
                        let c = self.card_mut(self.striker);
                        c.runs += 6; c.balls += 1; c.sixes += 1;
                        note = "SIX!".into();
                    }
                    BallOutcome::Runs(r) => {
                        self.runs += *r as u32;
                        let c = self.card_mut(self.striker);
                        c.runs += *r as u32; c.balls += 1;
                        note = format!("{r} run{}",
                            if *r == 1 { "" } else { "s" });
                    }
                    _ => unreachable!(),
                }
                // Rotate strike for odd runs.
                let ran = match outcome {
                    BallOutcome::Runs(r) => *r % 2 == 1,
                    BallOutcome::WicketAndRuns(_, r) => *r % 2 == 1,
                    _ => false,
                };
                if ran { self.swap_strike(); }
                // End of over rotates strike too.
                if self.balls_this_over == 6 {
                    self.balls_this_over = 0;
                    self.swap_strike();
                    self.previous_bowler = self.current_bowler;
                    if let Some(b) = self.current_bowler {
                        self.bowler_card_mut(b); // keep card
                    }
                }
                note
            }
        }
    }

    fn wicket(&mut self, d: &Dismissal, runs: u8, note: &mut String) {
        self.wickets += 1;
        self.runs += runs as u32;
        {
            let c = self.card_mut(self.striker);
            c.runs += runs as u32;
            c.balls += 1;
            c.out = Some(*d);
        }
        if let Some(b) = self.current_bowler {
            if matches!(d, Dismissal::Bowled | Dismissal::Lbw
                        | Dismissal::Caught { .. } | Dismissal::CaughtBehind { .. }
                        | Dismissal::Stumped | Dismissal::HitWicket) {
                self.bowler_card_mut(b).wickets += 1;
            }
        }
        *note = "OUT!".into();
        // Bring in the next batter.
        if self.next_batter_slot < self.order.len() {
            let next = self.order[self.next_batter_slot];
            self.next_batter_slot += 1;
            self.striker = next;
        }
    }

    fn swap_strike(&mut self) {
        std::mem::swap(&mut self.striker, &mut self.non_striker);
    }

    pub fn over_complete(&self) -> bool {
        self.legal_balls > 0 && self.legal_balls % 6 == 0 && self.balls_this_over == 0
    }

    pub fn all_out(&self, wickets_limit: u32) -> bool {
        self.wickets >= wickets_limit || self.next_batter_slot >= self.order.len()
    }

    /// True when the allotted overs have been completed.
    pub fn overs_done(&self, overs: u32) -> bool {
        self.legal_balls >= overs * 6
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Result {
    /// Winner index (0/1), margin description.
    Win { winner: usize, margin: String },
    Tie,
}

#[derive(Clone, Debug)]
pub struct MatchState {
    /// Overs per innings.
    pub overs: u32,
    pub wickets_limit: u32,
    /// Team indices into the tournament roster [batting_first_second...].
    pub teams: [usize; 2],
    pub innings_num: u8,
    pub innings: Innings,
    pub first_innings_total: Option<u32>,
    pub result: Option<Result>,
}

impl MatchState {
    pub fn new(overs: u32, teams: [usize; 2], first_order: Vec<usize>,
               bowling_players: &[usize]) -> Self {
        MatchState {
            overs,
            wickets_limit: 10,
            teams,
            innings_num: 1,
            innings: Innings::new(teams[0], first_order, None, bowling_players),
            first_innings_total: None,
            result: None,
        }
    }

    /// Call at natural points (after each ball resolution) to detect
    /// innings/match end. Returns an event for the UI.
    pub fn check_progression(&mut self) -> Option<Progression> {
        if self.result.is_some() {
            return None;
        }
        let inns_over = self.innings.all_out(self.wickets_limit)
            || self.innings.overs_done(self.overs)
            || self.innings.target.map_or(false, |t| self.innings.runs >= t);
        if !inns_over {
            return None;
        }
        if self.innings_num == 1 {
            self.first_innings_total = Some(self.innings.runs);
            Some(Progression::InningsBreak)
        } else {
            let t = self.innings.target.unwrap();
            let winner = if self.innings.runs >= t {
                Some(1)
            } else if self.innings.runs == t - 1 {
                None
            } else {
                Some(0)
            };
            self.result = Some(match winner {
                Some(w) if w == 1 => Result::Win {
                    winner: self.teams[1],
                    margin: format!("won by {} wickets", 10 - self.innings.wickets),
                },
                Some(_) => Result::Win {
                    winner: self.teams[0],
                    margin: format!("won by {} runs", t - 1 - self.innings.runs),
                },
                None => Result::Tie,
            });
            Some(Progression::MatchOver)
        }
    }

    /// Prepare the second innings. `chasing_order` is the new batting order.
    pub fn start_chase(&mut self, chasing_order: Vec<usize>,
                       bowling_players: &[usize]) {
        let target = self.first_innings_total.unwrap() + 1;
        // Swap so teams[0] is now the chasing side.
        self.teams = [self.teams[1], self.teams[0]];
        self.innings_num = 2;
        self.innings = Innings::new(
            0, chasing_order, Some(target), bowling_players);
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Progression {
    InningsBreak,
    MatchOver,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order() -> Vec<usize> { (0..11).collect() }

    #[test]
    fn scoring_and_rotation() {
        let mut i = Innings::new(0, order(), None, &[11]);
        i.current_bowler = Some(11);
        i.apply_ball(&BallOutcome::Runs(3)); // odd -> rotate
        assert_eq!(i.runs, 3);
        assert_eq!(i.striker, 1);
        i.apply_ball(&BallOutcome::Four);
        assert_eq!(i.striker, 1);
        assert_eq!(i.card_of(1).fours, 1);
        assert_eq!(i.legal_balls, 2);
    }

    #[test]
    fn wide_no_count() {
        let mut i = Innings::new(0, order(), None, &[11]);
        i.apply_ball(&BallOutcome::Wide);
        assert_eq!(i.legal_balls, 0);
        assert_eq!(i.extras, 1);
        assert_eq!(i.runs, 1);
    }

    #[test]
    fn over_end_swaps_strike() {
        let mut i = Innings::new(0, order(), None, &[11]);
        i.current_bowler = Some(11);
        for _ in 0..5 { i.apply_ball(&BallOutcome::Runs(0)); }
        assert_eq!(i.striker, 0);
        i.apply_ball(&BallOutcome::Runs(0));
        assert!(i.over_complete());
        assert_eq!(i.striker, 1); // rotated at over end
    }

    #[test]
    fn wicket_brings_next_batter() {
        let mut i = Innings::new(0, order(), None, &[11]);
        i.apply_ball(&BallOutcome::Wicket(Dismissal::Bowled));
        assert_eq!(i.wickets, 1);
        assert_eq!(i.striker, 2);
        assert_eq!(i.next_batter_slot, 3);
        assert!(i.card_of(0).out.is_some());
    }

    #[test]
    fn chase_win_detection() {
        let mut m = MatchState::new(20, [0, 1], order(), &[11]);
        let mut i = &mut m.innings;
        for _ in 0..120 { i.apply_ball(&BallOutcome::Runs(1)); }
        assert_eq!(m.innings.runs, 120);
        assert!(m.innings.overs_done(20));
        assert!(m.check_progression().is_some()); // innings break
        m.start_chase(order(), &[0]);
        assert_eq!(m.innings.target, Some(121));
        for _ in 0..110 { m.innings.apply_ball(&BallOutcome::Runs(1)); }
        assert!(m.check_progression().is_none()); // 110 < 121
        for _ in 0..11 { m.innings.apply_ball(&BallOutcome::Six); } // way past
        match m.check_progression() {
            Some(Progression::MatchOver) => {}
            other => panic!("expected match over, got {:?}", other),
        }
    }
}

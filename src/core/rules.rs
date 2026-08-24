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
            .map(|&p| BatterCard {
                player: p,
                runs: 0,
                balls: 0,
                fours: 0,
                sixes: 0,
                out: None,
            })
            .collect();
        let bowlers = bowling_players
            .iter()
            .map(|&p| BowlerCard {
                player: p,
                balls: 0,
                runs: 0,
                wickets: 0,
            })
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
        if self.legal_balls == 0 {
            0.0
        } else {
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
        self.bowlers
            .iter_mut()
            .find(|c| c.player == player)
            .unwrap()
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
                        c.runs += 4;
                        c.balls += 1;
                        c.fours += 1;
                        note = "FOUR!".into();
                    }
                    BallOutcome::Six => {
                        self.runs += 6;
                        let c = self.card_mut(self.striker);
                        c.runs += 6;
                        c.balls += 1;
                        c.sixes += 1;
                        note = "SIX!".into();
                    }
                    BallOutcome::Runs(r) => {
                        self.runs += *r as u32;
                        let c = self.card_mut(self.striker);
                        c.runs += *r as u32;
                        c.balls += 1;
                        note = format!("{r} run{}", if *r == 1 { "" } else { "s" });
                    }
                    _ => unreachable!(),
                }
                // Rotate strike for odd runs.
                let ran = match outcome {
                    BallOutcome::Runs(r) => *r % 2 == 1,
                    BallOutcome::WicketAndRuns(_, r) => *r % 2 == 1,
                    _ => false,
                };
                if ran {
                    self.swap_strike();
                }
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
        if let Some(b) = self.current_bowler
            && matches!(
                d,
                Dismissal::Bowled
                    | Dismissal::Lbw
                    | Dismissal::Caught { .. }
                    | Dismissal::CaughtBehind { .. }
                    | Dismissal::Stumped
                    | Dismissal::HitWicket
            )
        {
            self.bowler_card_mut(b).wickets += 1;
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
        self.legal_balls > 0 && self.legal_balls.is_multiple_of(6) && self.balls_this_over == 0
    }

    /// True when the batting side can no longer continue (wickets down or
    /// no more batters in a short custom order).
    pub fn all_out(&self, wickets_limit: u32) -> bool {
        let max_wickets = wickets_limit.min(self.order.len().saturating_sub(1) as u32);
        self.wickets >= max_wickets
    }

    /// True when the allotted overs have been completed.
    pub fn overs_done(&self, overs: u32) -> bool {
        self.legal_balls >= overs * 6
    }

    /// The one-fifth-of-the-innings cap on a single bowler, rounded up
    /// (e.g. 20 overs -> 4 per bowler, 50 overs -> 10). Real limited-overs
    /// laws phrase it this way so a short match still gets at least one over
    /// per bowler out of a full attack.
    pub fn max_overs_per_bowler(total_overs: u32) -> u32 {
        total_overs.div_ceil(5).max(1)
    }

    /// Legal balls `player` has already sent down this innings.
    pub fn balls_bowled_by(&self, player: usize) -> u32 {
        self.bowlers
            .iter()
            .find(|b| b.player == player)
            .map(|b| b.balls)
            .unwrap_or(0)
    }

    /// Whether `player` may legally open the next over: not the bowler who
    /// just finished (no two overs in a row) and still under the
    /// one-fifth-of-overs cap.
    pub fn bowler_eligible(&self, player: usize, total_overs: u32) -> bool {
        if Some(player) == self.previous_bowler {
            return false;
        }
        self.balls_bowled_by(player) < Self::max_overs_per_bowler(total_overs) * 6
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Result {
    /// Winner index (0/1), margin description.
    Win {
        winner: usize,
        margin: String,
    },
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
    pub fn new(
        overs: u32,
        teams: [usize; 2],
        first_order: Vec<usize>,
        bowling_players: &[usize],
    ) -> Self {
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
            || self.innings.target.is_some_and(|t| self.innings.runs >= t);
        if !inns_over {
            return None;
        }
        if self.innings_num == 1 {
            self.first_innings_total = Some(self.innings.runs);
            Some(Progression::InningsBreak)
        } else {
            let target = self.innings.target.unwrap();
            let chase_runs = self.innings.runs;
            // After `start_chase`, teams[0] is chasing and teams[1] defended.
            self.result = Some(if chase_runs >= target {
                Result::Win {
                    winner: self.teams[0],
                    margin: format!(
                        "won by {} wickets",
                        self.wickets_limit.saturating_sub(self.innings.wickets)
                    ),
                }
            } else if chase_runs == target - 1 {
                Result::Tie
            } else {
                Result::Win {
                    winner: self.teams[1],
                    margin: format!("won by {} runs", target - 1 - chase_runs),
                }
            });
            Some(Progression::MatchOver)
        }
    }

    /// Prepare the second innings. `chasing_order` is the new batting order.
    pub fn start_chase(&mut self, chasing_order: Vec<usize>, bowling_players: &[usize]) {
        let target = self.first_innings_total.unwrap() + 1;
        // Swap so teams[0] is now the chasing side.
        self.teams = [self.teams[1], self.teams[0]];
        self.innings_num = 2;
        self.innings = Innings::new(self.teams[0], chasing_order, Some(target), bowling_players);
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Progression {
    InningsBreak,
    MatchOver,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order() -> Vec<usize> {
        (0..11).collect()
    }

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
        for _ in 0..5 {
            i.apply_ball(&BallOutcome::Runs(0));
        }
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
    fn all_out_at_wickets_limit_not_ninth_wicket() {
        let mut i = Innings::new(0, order(), None, &[11]);
        for _ in 0..9 {
            i.apply_ball(&BallOutcome::Wicket(Dismissal::Bowled));
        }
        assert_eq!(i.wickets, 9);
        assert!(!i.all_out(10));
        i.apply_ball(&BallOutcome::Wicket(Dismissal::Bowled));
        assert_eq!(i.wickets, 10);
        assert!(i.all_out(10));
    }

    #[test]
    fn all_out_short_order() {
        let short = vec![0, 1, 2, 3, 4];
        let mut i = Innings::new(0, short, None, &[5]);
        for _ in 0..3 {
            i.apply_ball(&BallOutcome::Wicket(Dismissal::Bowled));
        }
        assert!(!i.all_out(10));
        i.apply_ball(&BallOutcome::Wicket(Dismissal::Bowled));
        assert!(i.all_out(10));
    }

    #[test]
    fn max_overs_per_bowler_rounds_up_one_fifth() {
        assert_eq!(Innings::max_overs_per_bowler(20), 4); // 20/5 exact
        assert_eq!(Innings::max_overs_per_bowler(50), 10); // 50/5 exact
        assert_eq!(Innings::max_overs_per_bowler(10), 2); // 10/5 exact
        assert_eq!(Innings::max_overs_per_bowler(7), 2); // ceil(7/5) = 2
        assert_eq!(Innings::max_overs_per_bowler(1), 1); // never zero
    }

    #[test]
    fn bowler_ineligible_for_back_to_back_overs() {
        let mut i = Innings::new(0, order(), None, &[11, 10]);
        i.previous_bowler = Some(11);
        assert!(!i.bowler_eligible(11, 20), "can't bowl two overs in a row");
        assert!(i.bowler_eligible(10, 20), "a different bowler is fine");
    }

    #[test]
    fn bowler_ineligible_past_one_fifth_of_overs() {
        let mut i = Innings::new(0, order(), None, &[11]);
        i.current_bowler = Some(11);
        // 20-over match: cap is 4 overs (24 balls) per bowler.
        for _ in 0..24 {
            i.apply_ball(&BallOutcome::Runs(0));
        }
        // previous_bowler only updates at the *end* of an over; bowling 24
        // dot balls straight through crosses 4 over-boundaries, each of
        // which stamps previous_bowler = 11, so both rules trip together —
        // exactly what should happen for a bowler who has used up the cap.
        assert!(!i.bowler_eligible(11, 20), "quota used up");
    }

    #[test]
    fn bowler_eligible_under_quota_and_not_previous() {
        let mut i = Innings::new(0, order(), None, &[11]);
        i.current_bowler = Some(11);
        for _ in 0..18 {
            i.apply_ball(&BallOutcome::Runs(0));
        }
        // 3 overs bowled, cap is 4 for a 20-over match, and the *previous*
        // over was bowled by someone else's over boundary logic — but here
        // 11 bowled every over, so 11 remains ineligible (rule 1). Confirm
        // a bowler under quota with a *different* previous over is fine.
        i.previous_bowler = Some(99); // simulate someone else just finished
        assert!(i.bowler_eligible(11, 20), "3 overs bowled, cap is 4");
    }

    #[test]
    fn chase_win_detection() {
        let mut m = MatchState::new(20, [0, 1], order(), &[11]);
        let i = &mut m.innings;
        for _ in 0..120 {
            i.apply_ball(&BallOutcome::Runs(1));
        }
        assert_eq!(m.innings.runs, 120);
        assert!(m.innings.overs_done(20));
        assert!(m.check_progression().is_some()); // innings break
        m.start_chase(order(), &[0]);
        assert_eq!(m.innings.batting_team, 1);
        assert_eq!(m.innings.target, Some(121));
        for _ in 0..110 {
            m.innings.apply_ball(&BallOutcome::Runs(1));
        }
        assert!(m.check_progression().is_none()); // 110 < 121
        for _ in 0..11 {
            m.innings.apply_ball(&BallOutcome::Six);
        } // way past
        match m.check_progression() {
            Some(Progression::MatchOver) => {}
            other => panic!("expected match over, got {:?}", other),
        }
        assert_eq!(
            m.result,
            Some(Result::Win {
                winner: 1,
                margin: "won by 10 wickets".into(),
            })
        );
    }

    #[test]
    fn chase_defending_win_by_runs() {
        let mut m = MatchState::new(5, [0, 1], order(), &[11]);
        for _ in 0..30 {
            m.innings.apply_ball(&BallOutcome::Runs(1));
        }
        assert_eq!(m.check_progression(), Some(Progression::InningsBreak));
        m.start_chase(order(), &[0]);
        for _ in 0..10 {
            m.innings.apply_ball(&BallOutcome::Runs(0));
        }
        for _ in 0..20 {
            m.innings.apply_ball(&BallOutcome::Runs(1));
        }
        assert_eq!(m.innings.runs, 20);
        assert!(m.innings.overs_done(5));
        assert_eq!(m.check_progression(), Some(Progression::MatchOver));
        assert_eq!(
            m.result,
            Some(Result::Win {
                winner: 0,
                margin: "won by 10 runs".into(),
            })
        );
    }

    #[test]
    fn chase_tie() {
        let mut m = MatchState::new(5, [0, 1], order(), &[11]);
        for _ in 0..30 {
            m.innings.apply_ball(&BallOutcome::Runs(1));
        }
        assert_eq!(m.check_progression(), Some(Progression::InningsBreak));
        m.start_chase(order(), &[0]);
        for _ in 0..30 {
            m.innings.apply_ball(&BallOutcome::Runs(1));
        }
        assert_eq!(m.innings.runs, 30);
        assert!(m.innings.overs_done(5));
        assert_eq!(m.check_progression(), Some(Progression::MatchOver));
        assert_eq!(m.result, Some(Result::Tie));
    }

    #[test]
    fn chase_win_wickets_margin_uses_limit() {
        let mut m = MatchState::new(5, [0, 1], order(), &[11]);
        for _ in 0..30 {
            m.innings.apply_ball(&BallOutcome::Runs(1));
        }
        assert_eq!(m.check_progression(), Some(Progression::InningsBreak));
        m.start_chase(order(), &[0]);
        m.wickets_limit = 6;
        for _ in 0..3 {
            m.innings
                .apply_ball(&BallOutcome::Wicket(Dismissal::Bowled));
        }
        for _ in 0..31 {
            m.innings.apply_ball(&BallOutcome::Runs(1));
        }
        assert_eq!(m.check_progression(), Some(Progression::MatchOver));
        assert_eq!(
            m.result,
            Some(Result::Win {
                winner: 1,
                margin: "won by 3 wickets".into(),
            })
        );
    }
}

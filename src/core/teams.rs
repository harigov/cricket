use std::f32::consts::TAU;

/// Bowling style of a player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BowlStyle {
    Fast,
    FastMedium,
    Medium,
    LegSpin,
    OffSpin,
}

impl BowlStyle {
    pub fn is_spin(&self) -> bool {
        matches!(self, BowlStyle::LegSpin | BowlStyle::OffSpin)
    }

    /// Base delivery speed in m/s (~ kph / 3.6).
    pub fn base_speed(&self) -> f32 {
        match self {
            BowlStyle::Fast => 38.0,      // ~137 kph
            BowlStyle::FastMedium => 34.0, // ~122 kph
            BowlStyle::Medium => 29.0,     // ~104 kph
            BowlStyle::LegSpin | BowlStyle::OffSpin => 24.0, // ~86 kph
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            BowlStyle::Fast => "Fast",
            BowlStyle::FastMedium => "Fast-medium",
            BowlStyle::Medium => "Medium",
            BowlStyle::LegSpin => "Leg spin",
            BowlStyle::OffSpin => "Off spin",
        }
    }
}

/// Player role for ordering & quicksim weighting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Batter,
    Keeper,
    AllRounder,
    Bowler,
}

/// A cricketer.
#[derive(Clone, Debug)]
pub struct Player {
    pub name: String,
    pub role: Role,
    /// 40..=100. Higher = better batter.
    pub batting: u8,
    /// 0..=100. 0 = doesn't bowl.
    pub bowling: u8,
    pub style: Option<BowlStyle>,
}

impl Player {
    fn new(
        name: &str,
        role: Role,
        batting: u8,
        bowling: u8,
        style: Option<BowlStyle>,
    ) -> Self {
        Player { name: name.into(), role, batting, bowling, style }
    }

    pub fn can_bowl(&self) -> bool {
        self.bowling > 20 && self.style.is_some()
    }
}

/// A team of 11 players with a sensible batting order
/// (keepers/bowlers last).
#[derive(Clone, Debug)]
pub struct Team {
    pub name: String,
    pub short: String,
    pub primary_color: bevy::color::Color,
    pub secondary_color: bevy::color::Color,
    pub players: Vec<Player>,
}

impl Team {
    pub fn batters(&self) -> impl Iterator<Item = usize> + '_ {
        self.players.iter().enumerate().map(|(i, _)| i)
    }

    pub fn bowlers(&self) -> Vec<usize> {
        self.players
            .iter()
            .enumerate()
            .filter(|(_, p)| p.can_bowl())
            .map(|(i, _)| i)
            .collect()
    }
}

fn team(name: &str, short: &str, c1: u32, c2: u32, roster: Vec<Player>) -> Team {
    Team {
        name: name.into(),
        short: short.into(),
        primary_color: bevy::color::Color::srgb_u8(
            (c1 >> 16) as u8,
            (c1 >> 8) as u8,
            c1 as u8,
        ),
        secondary_color: bevy::color::Color::srgb_u8(
            (c2 >> 16) as u8,
            (c2 >> 8) as u8,
            c2 as u8,
        ),
        players: roster,
    }
}

macro_rules! p {
    ($n:expr, $r:expr, $bat:expr, $bowl:expr, $style:expr) => {
        Player::new($n, $r, $bat, $bowl, $style)
    };
}

use Role::*;
use BowlStyle::*;

/// Built-in teams (fictional players).
pub fn builtin_teams() -> Vec<Team> {
    vec![
        team("India", "IND", 0x004B_A0, 0xFF99_00, vec![
            p!("R. Shanker", Batter, 88, 10, None),
            p!("A. Deshmukh", Batter, 85, 15, Some(OffSpin)),
            p!("V. Kolli", Batter, 92, 20, Some(Medium)),
            p!("S. Iyerkar", Batter, 80, 30, Some(OffSpin)),
            p!("K. Pandiyan", AllRounder, 76, 68, Some(FastMedium)),
            p!("D. Karthee", Keeper, 78, 0, None),
            p!("H. Pandiya", AllRounder, 70, 74, Some(FastMedium)),
            p!("R. Jadhav", Bowler, 45, 88, Some(LegSpin)),
            p!("J. Bumroh", Bowler, 35, 95, Some(Fast)),
            p!("M. Siraaj", Bowler, 32, 90, Some(Fast)),
            p!("Y. Chandel", Bowler, 30, 84, Some(Medium)),
        ]),
        team("Australia", "AUS", 0x1D4E_89, 0xF2C5_11, vec![
            p!("D. Warnick", Batter, 89, 12, Some(OffSpin)),
            p!("T. Headland", Batter, 86, 25, Some(OffSpin)),
            p!("M. Marshel", AllRounder, 82, 66, Some(FastMedium)),
            p!("S. Smithers", Batter, 91, 18, Some(LegSpin)),
            p!("G. Maxfield", AllRounder, 78, 72, Some(Medium)),
            p!("J. Inglish", Keeper, 77, 0, None),
            p!("T. Stoinberg", AllRounder, 69, 64, Some(Medium)),
            p!("P. Cumming", Bowler, 48, 93, Some(Fast)),
            p!("M. Starke", Bowler, 38, 94, Some(Fast)),
            p!("A. Zampaio", Bowler, 30, 86, Some(LegSpin)),
            p!("J. Hazlewick", Bowler, 28, 91, Some(Fast)),
        ]),
        team("England", "ENG", 0x1B3C_8C, 0xDC26_26, vec![
            p!("J. Roystone", Batter, 87, 10, None),
            p!("F. Saltburn", Keeper, 81, 0, None),
            p!("H. Brookeside", Batter, 85, 14, Some(Medium)),
            p!("L. Livingston", Batter, 88, 22, Some(OffSpin)),
            p!("B. Stoker", AllRounder, 79, 78, Some(Fast)),
            p!("M. Alikhan", AllRounder, 72, 70, Some(OffSpin)),
            p!("S. Curram", AllRounder, 68, 71, Some(FastMedium)),
            p!("C. Jordanson", Bowler, 42, 84, Some(Medium)),
            p!("A. Rashido", Bowler, 36, 90, Some(LegSpin)),
            p!("M. Woodson", Bowler, 33, 92, Some(Fast)),
            p!("J. Arkle", Bowler, 30, 88, Some(Fast)),
        ]),
        team("Pakistan", "PAK", 0x0A6B_3D, 0xDDDD_DD, vec![
            p!("B. Azzamat", Batter, 89, 12, Some(LegSpin)),
            p!("M. Rizzwan", Keeper, 84, 0, None),
            p!("F. Zamani", Batter, 82, 16, Some(OffSpin)),
            p!("S. Maqsood", Batter, 78, 24, Some(OffSpin)),
            p!("I. Khanzada", AllRounder, 75, 69, Some(FastMedium)),
            p!("S. Aslam", AllRounder, 71, 73, Some(Fast)),
            p!("M. Nawazi", AllRounder, 67, 72, Some(OffSpin)),
            p!("S. Afridul", Bowler, 44, 94, Some(Fast)),
            p!("H. Raoof", Bowler, 34, 89, Some(Fast)),
            p!("N. Shahzad", Bowler, 31, 87, Some(Fast)),
            p!("A. Qadeer", Bowler, 28, 83, Some(LegSpin)),
        ]),
        team("South Africa", "RSA", 0x0069_3E, 0xFFB6_12, vec![
            p!("Q. de Klerk", Keeper, 86, 0, None),
            p!("R. Hendriks", Batter, 83, 10, None),
            p!("A. Markman", Batter, 85, 21, Some(OffSpin)),
            p!("H. Klaasenburg", Batter, 80, 12, None),
            p!("D. Millard", Batter, 88, 14, None),
            p!("T. Stubbings", AllRounder, 72, 65, Some(OffSpin)),
            p!("M. Jansohn", AllRounder, 68, 76, Some(Fast)),
            p!("K. Maharajh", Bowler, 40, 85, Some(OffSpin)),
            p!("K. Rabaad", Bowler, 36, 96, Some(Fast)),
            p!("A. Nortier", Bowler, 32, 90, Some(Fast)),
            p!("L. Ngidhi", Bowler, 28, 86, Some(Fast)),
        ]),
        team("New Zealand", "NZL", 0x111417, 0xC8C8C8, vec![
            p!("F. Allanby", Batter, 84, 10, None),
            p!("D. Conwell", Batter, 86, 15, Some(Medium)),
            p!("K. Willison", Batter, 90, 20, Some(Medium)),
            p!("G. Phillipson", Batter, 82, 12, None),
            p!("D. Mitchelson", Batter, 79, 25, Some(OffSpin)),
            p!("T. Lathmore", Keeper, 76, 0, None),
            p!("M. Bracewood", AllRounder, 71, 70, Some(OffSpin)),
            p!("M. Santino", AllRounder, 66, 74, Some(LegSpin)),
            p!("T. Boultby", Bowler, 38, 91, Some(FastMedium)),
            p!("L. Fergusson", Bowler, 32, 93, Some(Fast)),
            p!("T. Southey", Bowler, 34, 89, Some(Fast)),
        ]),
    ]
}

/// Pick a balanced batting order: sort so that specialists bat before
/// bowlers (input rosters are already ordered; kept for custom teams).
pub fn batting_order(team: &Team) -> Vec<usize> {
    let mut order: Vec<usize> = (0..team.players.len()).collect();
    order.sort_by_key(|&i| {
        let pl = &team.players[i];
        match pl.role {
            Role::Batter | Role::Keeper => 0,
            Role::AllRounder => 1,
            Role::Bowler => 2,
        }
    });
    order
}

/// Choose the best `count` bowlers by rating.
pub fn pick_bowlers(team: &Team, count: usize) -> Vec<usize> {
    let mut b = team.bowlers();
    b.sort_by(|&a, &b| team.players[b].bowling.cmp(&team.players[a].bowling));
    b.truncate(count);
    b
}

/// Total team rating used by the quick simulator.
pub fn team_rating(team: &Team) -> f32 {
    let bat: f32 =
        team.players.iter().map(|p| p.batting as f32).sum::<f32>() / 11.0;
    let bowl: f32 = {
        let mut bs = team.bowlers();
        if bs.is_empty() { 50.0 } else {
            bs.sort_by(|&a, &b| team.players[b].bowling.cmp(&team.players[a].bowling));
            bs.truncate(5);
            bs.iter().map(|&i| team.players[i].bowling as f32).sum::<f32>() / 5.0
        }
    };
    bat * 0.55 + bowl * 0.45
}

/// Deterministic-ish hash helper for quicksim variety without RNG state.
pub fn hash_f32(seed: u64) -> f32 {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^= x >> 30; x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27; x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    ((x % 10_000) as f32) / 10_000.0
}

pub fn angle_spread(seed: u64) -> f32 {
    hash_f32(seed) * TAU
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn teams_have_eleven_players() {
        for t in builtin_teams() {
            assert_eq!(t.players.len(), 11, "{} has wrong size", t.name);
            assert!(t.bowlers().len() >= 4, "{} lacks bowlers", t.name);
        }
    }

    #[test]
    fn bowlers_sorted_by_rating() {
        let t = &builtin_teams()[0];
        let b = pick_bowlers(t, 5);
        assert_eq!(b.len(), 5);
        assert_eq!(t.players[b[0]].name, "J. Bumroh");
    }

    #[test]
    fn order_puts_bowlers_last() {
        let t = &builtin_teams()[1];
        let o = batting_order(t);
        let last = &t.players[*o.last().unwrap()];
        assert_eq!(last.role, Bowler);
    }
}

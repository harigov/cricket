pub mod geometry;
pub mod rules;
pub mod stadiums;
pub mod teams;
pub mod tournament;

/// Coordinate conventions used across the whole game:
///
/// * `Y` is up.
/// * The pitch runs along the `X` axis. The bowling end stumps are at
///   `x = -PITCH_HALF_LEN`, the striker's stumps at `x = +PITCH_HALF_LEN`.
/// * A delivery travels in the `+X` direction.
/// * `Z` spans the width of the pitch. For a right-handed batter facing the
///   bowler (`-X`), the **off side** is `+Z` and the leg side is `-Z`.
/// * Shot directions / field positions use an angle measured in degrees
///   clockwise from "straight down the ground" (`-X`). Positive angles sweep
///   toward the off side (`+Z`), negative toward the leg side.
pub fn angle_dir(degrees: f32) -> bevy::math::Vec2 {
    // Returns a unit vector in the XZ plane: (x, z) pointing where the
    // ball should travel for the given shot angle.
    let rad = degrees.to_radians();
    bevy::math::Vec2::new(-rad.cos(), rad.sin())
}

/// Footwork the batter commits to before contact. Front foot reaches forward
/// to the pitch of the ball, back foot rocks back to give room and time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Footwork {
    /// Neither foot committed — a stand-and-deliver push off the stance.
    #[default]
    Planted,
    Front,
    Back,
}

/// A named cricket stroke. The batter's control inputs (footwork, aim and the
/// loft modifier) are classified into one of these, and it in turn drives both
/// the animation and the shot's exit-velocity profile.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ShotKind {
    /// Dead-bat block: no run-scoring intent, very forgiving on timing.
    #[default]
    Defend,
    /// Back-foot defensive push.
    Backfoot,
    StraightDrive,
    CoverDrive,
    OnDrive,
    /// Wristy clip off the pads through midwicket.
    Flick,
    SquareCut,
    LateCut,
    Pull,
    Hook,
    Sweep,
    SlogSweep,
    /// Front-foot drive hit over the infield.
    LoftedDrive,
    /// Cross-batted heave over the leg side.
    Slog,
}

/// Classify stick Y into committed footwork. Threshold matches batting input.
pub fn footwork_from_move_y(y: f32) -> Footwork {
    if y > 0.4 {
        Footwork::Front
    } else if y < -0.4 {
        Footwork::Back
    } else {
        Footwork::Planted
    }
}

/// Map footwork + aim + loft onto a named stroke. Aim is -1 (leg side) .. +1 (off side).
///
/// Keyboard aim is discrete: `poll_input` builds `move_vec` from digital key holds
/// (±1 per axis) and `clamp_length_max(1.0)`, so only nine `(aim_x, move_y)` pairs
/// exist. Bands use three aim levels — leg (`aim <= -0.5`), straight (`|aim| < 0.5`),
/// off (`aim >= 0.5`) — crossed with three footwork states and the loft modifier:
///
/// ```text
/// keys      aim_x    move_y   footwork   flat              loft
/// none       0.000   0.000    Planted    Defend            LoftedDrive
/// A         -1.000   0.000    Planted    OnDrive           Slog
/// D         +1.000   0.000    Planted    CoverDrive        LoftedDrive
/// W          0.000  +1.000    Front      StraightDrive     LoftedDrive
/// W+A       -0.707  +0.707    Front      Sweep             SlogSweep
/// W+D       +0.707  +0.707    Front      CoverDrive        LoftedDrive
/// S          0.000  -1.000    Back       Backfoot          Flick
/// S+A       -0.707  -0.707    Back       Pull              Hook
/// S+D       +0.707  -0.707    Back       SquareCut         LateCut
/// ```
///
/// Analog sticks still traverse the continuous range between those levels.
pub fn select_shot(footwork: Footwork, aim_x: f32, loft: bool) -> ShotKind {
    let aim = aim_x.clamp(-1.0, 1.0);
    let leg = aim <= -0.5;
    let off = aim >= 0.5;
    match footwork {
        Footwork::Planted => {
            if leg {
                if loft {
                    ShotKind::Slog
                } else {
                    ShotKind::OnDrive
                }
            } else if off {
                if loft {
                    ShotKind::LoftedDrive
                } else {
                    ShotKind::CoverDrive
                }
            } else if loft {
                ShotKind::LoftedDrive
            } else {
                ShotKind::Defend
            }
        }
        Footwork::Front => {
            if leg {
                if loft {
                    ShotKind::SlogSweep
                } else {
                    ShotKind::Sweep
                }
            } else if off {
                if loft {
                    ShotKind::LoftedDrive
                } else {
                    ShotKind::CoverDrive
                }
            } else if loft {
                ShotKind::LoftedDrive
            } else {
                ShotKind::StraightDrive
            }
        }
        Footwork::Back => {
            if leg {
                if loft {
                    ShotKind::Hook
                } else {
                    ShotKind::Pull
                }
            } else if off {
                if loft {
                    ShotKind::LateCut
                } else {
                    ShotKind::SquareCut
                }
            } else if loft {
                ShotKind::Flick
            } else {
                ShotKind::Backfoot
            }
        }
    }
}

/// Exit-velocity profile for a stroke: base speed (m/s), launch elevation (deg),
/// the preferred scoring angle (deg, per [`angle_dir`] conventions) and a
/// timing-forgiveness multiplier (>1 = easier to time).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShotProfile {
    pub speed: f32,
    pub elev: f32,
    pub angle: f32,
    pub forgiveness: f32,
}

/// Per-stroke contact characteristics blended with timing tier at the bat.
pub fn shot_profile(kind: ShotKind) -> ShotProfile {
    match kind {
        ShotKind::Defend => ShotProfile {
            speed: 9.0,
            elev: 4.0,
            angle: 0.0,
            forgiveness: 2.35,
        },
        ShotKind::Backfoot => ShotProfile {
            speed: 11.0,
            elev: 5.0,
            angle: 8.0,
            forgiveness: 2.05,
        },
        ShotKind::StraightDrive => ShotProfile {
            speed: 30.0,
            elev: 8.0,
            angle: 0.0,
            forgiveness: 1.05,
        },
        ShotKind::CoverDrive => ShotProfile {
            speed: 28.0,
            elev: 10.0,
            angle: 42.0,
            forgiveness: 1.0,
        },
        ShotKind::OnDrive => ShotProfile {
            speed: 26.0,
            elev: 9.0,
            angle: -28.0,
            forgiveness: 1.05,
        },
        ShotKind::Flick => ShotProfile {
            speed: 24.0,
            elev: 11.0,
            angle: -48.0,
            forgiveness: 0.98,
        },
        ShotKind::SquareCut => ShotProfile {
            speed: 27.0,
            elev: 12.0,
            angle: 58.0,
            forgiveness: 0.95,
        },
        ShotKind::LateCut => ShotProfile {
            speed: 22.0,
            elev: 14.0,
            angle: 78.0,
            forgiveness: 0.88,
        },
        ShotKind::Pull => ShotProfile {
            speed: 29.0,
            elev: 14.0,
            angle: -62.0,
            forgiveness: 0.92,
        },
        ShotKind::Hook => ShotProfile {
            speed: 26.0,
            elev: 38.0,
            angle: -72.0,
            forgiveness: 0.72,
        },
        ShotKind::Sweep => ShotProfile {
            speed: 23.0,
            elev: 6.0,
            angle: -55.0,
            forgiveness: 0.9,
        },
        ShotKind::SlogSweep => ShotProfile {
            speed: 31.0,
            elev: 32.0,
            angle: -68.0,
            forgiveness: 0.68,
        },
        ShotKind::LoftedDrive => ShotProfile {
            speed: 32.0,
            elev: 34.0,
            angle: 12.0,
            forgiveness: 0.78,
        },
        ShotKind::Slog => ShotProfile {
            speed: 34.0,
            elev: 42.0,
            angle: -35.0,
            forgiveness: 0.55,
        },
    }
}

/// Timing degradation when a stroke is played to the wrong length. Returns a
/// multiplier >= 1.0 applied to the effective timing offset (higher = worse).
pub fn shot_length_penalty(kind: ShotKind, length_from_stumps: f32) -> f32 {
    if matches!(kind, ShotKind::Defend | ShotKind::Backfoot) {
        return 1.0;
    }
    let dist = (length_from_stumps - kind.ideal_length()).abs();
    match kind {
        ShotKind::StraightDrive | ShotKind::CoverDrive | ShotKind::OnDrive | ShotKind::Flick => {
            1.0 + (dist / 4.2).powi(2) * 0.85
        }
        ShotKind::LoftedDrive => 1.0 + (dist / 3.8).powi(2) * 0.95,
        ShotKind::Sweep | ShotKind::SlogSweep => 1.0 + (dist / 2.8).powi(2) * 1.05,
        ShotKind::SquareCut | ShotKind::LateCut => 1.0 + (dist / 3.5).powi(2) * 0.9,
        ShotKind::Pull | ShotKind::Hook => 1.0 + (dist / 3.2).powi(2) * 1.1,
        ShotKind::Slog => 1.0 + (dist / 5.0).powi(2) * 0.55,
        ShotKind::Defend | ShotKind::Backfoot => unreachable!(),
    }
}

impl ShotKind {
    /// Display name for the HUD shot indicator.
    pub fn label(self) -> &'static str {
        match self {
            ShotKind::Defend => "Defend",
            ShotKind::Backfoot => "Back-foot Defence",
            ShotKind::StraightDrive => "Straight Drive",
            ShotKind::CoverDrive => "Cover Drive",
            ShotKind::OnDrive => "On Drive",
            ShotKind::Flick => "Flick",
            ShotKind::SquareCut => "Cut",
            ShotKind::LateCut => "Late Cut",
            ShotKind::Pull => "Pull",
            ShotKind::Hook => "Hook",
            ShotKind::Sweep => "Sweep",
            ShotKind::SlogSweep => "Slog Sweep",
            ShotKind::LoftedDrive => "Lofted Drive",
            ShotKind::Slog => "Slog",
        }
    }

    /// True for strokes played with a horizontal bat — they swing through a
    /// flatter arc and are animated differently from the vertical-bat drives.
    pub fn cross_batted(self) -> bool {
        matches!(
            self,
            ShotKind::SquareCut
                | ShotKind::LateCut
                | ShotKind::Pull
                | ShotKind::Hook
                | ShotKind::Sweep
                | ShotKind::SlogSweep
                | ShotKind::Slog
        )
    }

    /// True when the stroke is deliberately hit into the air.
    pub fn aerial(self) -> bool {
        matches!(
            self,
            ShotKind::LoftedDrive | ShotKind::SlogSweep | ShotKind::Slog | ShotKind::Hook
        )
    }

    /// Ideal pitch length (metres before the stumps) for this stroke.
    pub fn ideal_length(self) -> f32 {
        match self {
            ShotKind::Defend | ShotKind::Backfoot => 7.0,
            ShotKind::StraightDrive
            | ShotKind::CoverDrive
            | ShotKind::OnDrive
            | ShotKind::Flick
            | ShotKind::LoftedDrive => 5.8,
            ShotKind::Sweep | ShotKind::SlogSweep => 4.6,
            ShotKind::SquareCut | ShotKind::LateCut => 10.5,
            ShotKind::Pull | ShotKind::Hook => 11.8,
            ShotKind::Slog => 7.5,
        }
    }
}

#[cfg(test)]
mod shot_tests {
    use super::*;
    use bevy::math::Vec2;

    /// Mirror `poll_input`'s keyboard branch: integer axis deltas, then length clamp.
    fn keyboard_move_vec(left: bool, right: bool, prev: bool, next: bool) -> Vec2 {
        let mut mv = Vec2::ZERO;
        if left {
            mv.x -= 1.0;
        }
        if right {
            mv.x += 1.0;
        }
        if prev {
            mv.y += 1.0;
        }
        if next {
            mv.y -= 1.0;
        }
        mv.clamp_length_max(1.0)
    }

    #[test]
    fn every_shot_kind_is_reachable() {
        let key_combos = [
            (false, false, false, false), // none
            (true, false, false, false),  // A
            (false, true, false, false),  // D
            (false, false, true, false),  // W
            (true, false, true, false),   // W+A
            (false, true, true, false),   // W+D
            (false, false, false, true),  // S
            (true, false, false, true),   // S+A
            (false, true, false, true),   // S+D
        ];
        let mut reached = std::collections::HashSet::new();
        for &(left, right, prev, next) in &key_combos {
            let mv = keyboard_move_vec(left, right, prev, next);
            let fw = footwork_from_move_y(mv.y);
            let aim = mv.x;
            for loft in [false, true] {
                reached.insert(select_shot(fw, aim, loft));
            }
        }
        for kind in [
            ShotKind::Defend,
            ShotKind::Backfoot,
            ShotKind::StraightDrive,
            ShotKind::CoverDrive,
            ShotKind::OnDrive,
            ShotKind::Flick,
            ShotKind::SquareCut,
            ShotKind::LateCut,
            ShotKind::Pull,
            ShotKind::Hook,
            ShotKind::Sweep,
            ShotKind::SlogSweep,
            ShotKind::LoftedDrive,
            ShotKind::Slog,
        ] {
            assert!(reached.contains(&kind), "keyboard cannot reach {kind:?}");
        }
    }

    #[test]
    fn defensive_shots_are_forgiving_and_not_aerial() {
        for kind in [ShotKind::Defend, ShotKind::Backfoot] {
            let p = shot_profile(kind);
            assert!(p.forgiveness > 1.8);
            assert!(p.speed < 14.0);
            assert!(!kind.aerial());
        }
    }

    #[test]
    fn aerial_shots_trade_risk_for_reward() {
        let slog = shot_profile(ShotKind::Slog);
        let defend = shot_profile(ShotKind::Defend);
        assert!(slog.speed > defend.speed * 2.5);
        assert!(slog.elev > defend.elev * 5.0);
        assert!(slog.forgiveness < defend.forgiveness * 0.35);
        assert!(ShotKind::Slog.aerial());
    }

    #[test]
    fn length_penalty_hits_mismatched_strokes() {
        let pull_full = shot_length_penalty(ShotKind::Pull, 4.0);
        let pull_short = shot_length_penalty(ShotKind::Pull, 12.0);
        assert!(pull_full > pull_short * 1.4);

        let drive_short = shot_length_penalty(ShotKind::CoverDrive, 12.5);
        let drive_good = shot_length_penalty(ShotKind::CoverDrive, 5.8);
        assert!(drive_short > drive_good * 1.2);
    }

    #[test]
    fn length_penalty_minimised_at_ideal_length() {
        let kinds = [
            ShotKind::Defend,
            ShotKind::Backfoot,
            ShotKind::StraightDrive,
            ShotKind::CoverDrive,
            ShotKind::OnDrive,
            ShotKind::Flick,
            ShotKind::SquareCut,
            ShotKind::LateCut,
            ShotKind::Pull,
            ShotKind::Hook,
            ShotKind::Sweep,
            ShotKind::SlogSweep,
            ShotKind::LoftedDrive,
            ShotKind::Slog,
        ];
        for kind in kinds {
            if matches!(kind, ShotKind::Defend | ShotKind::Backfoot) {
                assert_eq!(shot_length_penalty(kind, 3.0), 1.0);
                assert_eq!(shot_length_penalty(kind, kind.ideal_length()), 1.0);
                continue;
            }
            let ideal = kind.ideal_length();
            let at_ideal = shot_length_penalty(kind, ideal);
            assert_eq!(at_ideal, 1.0, "{kind:?} at ideal length");
            for offset in [0.5, 1.0, 2.0, 4.0] {
                assert!(
                    shot_length_penalty(kind, ideal - offset) > at_ideal,
                    "{kind:?} too short by {offset}"
                );
                assert!(
                    shot_length_penalty(kind, ideal + offset) > at_ideal,
                    "{kind:?} too long by {offset}"
                );
            }
        }
    }

    #[test]
    fn loft_never_flattens_stroke() {
        for fw in [Footwork::Planted, Footwork::Front, Footwork::Back] {
            for aim_step in -10..=10 {
                let aim = aim_step as f32 / 10.0;
                let flat = select_shot(fw, aim, false);
                let lofted = select_shot(fw, aim, true);
                let flat_elev = shot_profile(flat).elev;
                let loft_elev = shot_profile(lofted).elev;
                assert!(
                    loft_elev >= flat_elev,
                    "fw={fw:?} aim={aim}: {flat:?} elev={flat_elev} vs {lofted:?} elev={loft_elev}"
                );
            }
        }
    }
}

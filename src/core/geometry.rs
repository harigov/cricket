/// Pitch and field geometry constants (metres / degrees), matching real
/// cricket dimensions so distances feel right.

/// Stump-to-stump length of the pitch.
pub const PITCH_LENGTH: f32 = 20.12;
pub const PITCH_HALF_LEN: f32 = PITCH_LENGTH / 2.0;
/// Width of the playing strip.
pub const PITCH_WIDTH: f32 = 3.05;
/// Distance from stumps to the popping crease.
pub const CREASE_DEPTH: f32 = 1.22;
/// Height of the stumps.
pub const STUMP_HEIGHT: f32 = 0.71;

/// Where the striker stands, slightly in front of the stumps and on the
/// leg side of the stump line.
pub const BATSMAN_POS: bevy::math::Vec2 =
    bevy::math::Vec2::new(PITCH_HALF_LEN - 0.9, -0.15);
/// Keeper crouches just behind the stumps (distance from the batter).
pub const KEEPER_OFFSET: f32 = 2.6;
/// Bowler releases the ball roughly here (front foot on the crease).
pub const RELEASE_POINT: bevy::math::Vec3 = bevy::math::Vec3::new(
    -PITCH_HALF_LEN + 1.22,
    2.05,
    0.0,
);

/// Default boundary radius; stadiums override this.
pub const DEFAULT_BOUNDARY_RADIUS: f32 = 65.0;

/// A named fielding position: polar coordinates around the striker's
/// ground position. `angle` is degrees from straight (see module docs),
/// `dist` is metres from the striker.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FieldPos {
    pub name: &'static str,
    pub angle: f32,
    pub dist: f32,
}

impl FieldPos {
    /// World XZ position of this fielder for the given striker position.
    pub fn world_pos(&self, striker: bevy::math::Vec2) -> bevy::math::Vec2 {
        let d = crate::core::angle_dir(self.angle);
        striker + d * self.dist
    }
}

use std::f32::consts::PI;

const fn fp(name: &'static str, angle: f32, dist: f32) -> FieldPos {
    FieldPos { name, angle, dist }
}

/// Standard positions used to lay out fields. Angles: positive = off side.
pub mod positions {
    use super::*;

    pub const KEEPER: FieldPos = fp("Keeper", 180.0, super::super::geometry::KEEPER_OFFSET);    pub const SLIP: FieldPos = fp("Slip", 163.0, 11.0);
    pub const THIRD_MAN: FieldPos = fp("Third Man", 140.0, 55.0);
    pub const POINT: FieldPos = fp("Point", 95.0, 30.0);
    pub const COVER: FieldPos = fp("Cover", 60.0, 30.0);
    pub const MID_OFF: FieldPos = fp("Mid-off", 25.0, 28.0);
    pub const MID_ON: FieldPos = fp("Mid-on", -25.0, 28.0);
    pub const MIDWICKET: FieldPos = fp("Midwicket", -60.0, 30.0);
    pub const SQUARE_LEG: FieldPos = fp("Square Leg", -95.0, 28.0);
    pub const FINE_LEG: FieldPos = fp("Fine Leg", -150.0, 52.0);
    pub const LONG_OFF: FieldPos = fp("Long Off", 15.0, 58.0);
    pub const LONG_ON: FieldPos = fp("Long On", -18.0, 58.0);
    pub const DEEP_MIDWICKET: FieldPos = fp("Deep Midwicket", -50.0, 58.0);
    pub const DEEP_POINT: FieldPos = fp("Deep Point", 85.0, 55.0);
    pub const DEEP_COVER: FieldPos = fp("Deep Cover", 50.0, 56.0);
    pub const SHORT_FINE_LEG: FieldPos = fp("Short Fine Leg", -165.0, 16.0);
}

/// A field layout: keeper + 10 fielders (bowler excluded, they follow through).
#[derive(Clone, Debug)]
pub struct FieldLayout {
    pub positions: Vec<FieldPos>,
}

impl FieldLayout {
    /// Balanced T20-style spread field with a slip.
    pub fn standard() -> Self {
        use positions::*;
        FieldLayout {
            positions: vec![
                KEEPER,
                SLIP,
                POINT,
                COVER,
                MID_OFF,
                MID_ON,
                MIDWICKET,
                SQUARE_LEG,
                DEEP_MIDWICKET,
                LONG_ON,
                FINE_LEG,
            ],
        }
    }

    /// Attacking field with catchers in the ring for the new batter.
    pub fn attacking() -> Self {
        use positions::*;
        FieldLayout {
            positions: vec![
                KEEPER,
                SLIP,
                SHORT_FINE_LEG,
                POINT,
                COVER,
                MID_OFF,
                MID_ON,
                SQUARE_LEG,
                LONG_OFF,
                DEEP_COVER,
                DEEP_MIDWICKET,
            ],
        }
    }

    /// Defensive boundary-riding field when batters are set.
    pub fn defensive() -> Self {
        use positions::*;
        FieldLayout {
            positions: vec![
                KEEPER,
                THIRD_MAN,
                DEEP_POINT,
                DEEP_COVER,
                LONG_OFF,
                LONG_ON,
                DEEP_MIDWICKET,
                SQUARE_LEG,
                FINE_LEG,
                MID_OFF,
                MID_ON,
            ],
        }
    }

    pub fn keeper(&self) -> &FieldPos {
        &self.positions[0]
    }
}

/// Convert an impact direction (unit vector in XZ) back into a shot angle
/// in degrees, following our convention (0 = straight, +off side).
pub fn dir_to_angle(dir: bevy::math::Vec2) -> f32 {
    // angle_dir(t) = (-cos t, sin t); so given d: t = atan2(d.z, -d.x)
    let a = dir.y.atan2(-dir.x);
    a.to_degrees().rem_euclid(360.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::Vec2;

    #[test]
    fn angle_dir_straight_is_minus_x() {
        let d = crate::core::angle_dir(0.0);
        assert!(d.x < -0.99 && d.y.abs() < 0.01);
    }

    #[test]
    fn angle_dir_off_side_positive_z() {
        let d = crate::core::angle_dir(90.0);
        assert!(d.x.abs() < 0.01 && d.y > 0.99);
    }

    #[test]
    fn roundtrip_angle() {
        for deg in [-170.0_f32, -45.0, 0.0, 33.0, 120.0, 179.0] {
            let d = crate::core::angle_dir(deg);
            let back = dir_to_angle(d).rem_euclid(360.0);
            let diff = (back - deg.rem_euclid(360.0)).abs();
            assert!(diff < 0.5, "deg {} got {}", deg, back);
        }
    }

    #[test]
    fn keeper_behind_stumps() {
        let p = positions::KEEPER.world_pos(BATSMAN_POS);
        assert!(p.x > BATSMAN_POS.x + 1.8);
        assert!(p.y.abs() < 1.5); // Vec2.y here maps to world z
    }
}

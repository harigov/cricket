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

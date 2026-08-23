//! Helpers for placing geometry on circular stadium rings.
//! Local +X follows the tangent; local +Z faces outward along the radius.

use std::f32::consts::{FRAC_PI_2, PI};

use bevy::prelude::*;

/// Unit tangent on the seating ring at `angle` radians (XZ plane).
pub fn ring_tangent(angle: f32) -> Vec3 {
    Vec3::new(-angle.sin(), 0.0, angle.cos())
}

/// Outward radial unit vector at `angle`.
pub fn ring_radial(angle: f32) -> Vec3 {
    Vec3::new(angle.cos(), 0.0, angle.sin())
}

/// World position on a ring at the given radius and height.
pub fn ring_position(angle: f32, radius: f32, height: f32) -> Vec3 {
    Vec3::new(angle.cos() * radius, height, angle.sin() * radius)
}

/// Transform for ring-aligned box geometry: width along tangent, depth along radius.
pub fn ring_segment_transform(angle: f32, radius: f32, height: f32) -> Transform {
    Transform::from_translation(ring_position(angle, radius, height))
        .with_rotation(Quat::from_rotation_y(-angle - FRAC_PI_2))
}

/// Rotation for spectators seated on the ring, facing the pitch centre.
pub fn ring_face_center_rotation(angle: f32) -> Quat {
    let pos = ring_position(angle, 1.0, 0.0);
    Transform::from_translation(pos)
        .looking_at(Vec3::ZERO, Vec3::Y)
        .rotation
}

/// Floodlight tower placement: just outside the seating bowl at each quadrant.
pub fn floodlight_angles() -> [f32; 4] {
    [PI * 0.25, PI * 0.75, PI * 1.25, PI * 1.75]
}

/// Radius for towers given the outer edge of the seating bowl.
pub fn floodlight_radius(bowl_outer_radius: f32) -> f32 {
    bowl_outer_radius + 9.5
}

/// Half-width of the square stadium apron (centred on the pitch).
/// Keeps surrounds ground in sync with bowl scale — extends past floodlight towers.
pub fn stadium_ground_half_extent(bowl_outer_radius: f32) -> f32 {
    floodlight_radius(bowl_outer_radius) + 8.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tangent_perpendicular_to_radial() {
        for i in 0..16 {
            let a = i as f32 / 16.0 * PI * 2.0;
            let t = ring_tangent(a);
            let r = ring_radial(a);
            assert!((t.dot(r)).abs() < 1e-5, "angle {a}");
            assert!((t.length() - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn tangent_at_zero_points_along_positive_z() {
        let t = ring_tangent(0.0);
        assert!((t.x).abs() < 1e-5);
        assert!((t.z - 1.0).abs() < 1e-5);
    }

    #[test]
    fn ring_segment_maps_local_x_to_tangent() {
        let angle = 0.7;
        let xf = ring_segment_transform(angle, 50.0, 2.0);
        let local_x = xf.rotation * Vec3::X;
        let tangent = ring_tangent(angle);
        assert!((local_x - tangent).length() < 1e-4);
    }

    #[test]
    fn ring_segment_maps_local_z_inward() {
        let angle = 1.1;
        let xf = ring_segment_transform(angle, 50.0, 2.0);
        let local_z = xf.rotation * Vec3::Z;
        let radial = ring_radial(angle);
        // +Z faces toward the pitch (inward); -Z is outward radial.
        assert!((local_z + radial).length() < 1e-4);
    }

    #[test]
    fn face_center_rotation_looks_inward() {
        let angle = 0.0;
        let rot = ring_face_center_rotation(angle);
        let forward = rot * Vec3::NEG_Z;
        assert!(forward.x < -0.9, "expected inward -X, got {forward:?}");
    }

    #[test]
    fn floodlight_radius_outside_bowl() {
        let bowl = 80.0;
        assert!(floodlight_radius(bowl) > bowl);
    }

    #[test]
    fn ground_extent_beyond_bowl_outer() {
        let bowl_outer = 101.0;
        let half = stadium_ground_half_extent(bowl_outer);
        assert!(
            half > bowl_outer,
            "apron must extend past bowl outer ({bowl_outer}), got half {half}"
        );
        assert!(
            half > floodlight_radius(bowl_outer),
            "apron must extend past floodlights"
        );
    }
}

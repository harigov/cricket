//! Helpers for placing geometry on circular stadium rings.
//! Local +X follows the tangent; local +Z faces outward along the radius.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
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

/// Radius of the circular stadium apron (centred on the pitch).
/// Large enough that the establishing broadcast camera never frames a hard edge
/// against the sky, while staying inside the sky dome.
pub fn stadium_ground_radius(bowl_outer_radius: f32) -> f32 {
    // ~578 m for a standard bowl — past floodlights, inside the 600 m sky shell.
    (floodlight_radius(bowl_outer_radius) + 470.0).min(578.0)
}

/// Half-extent alias used by apron extent tests (disc radius for a circular apron).
pub fn stadium_ground_half_extent(bowl_outer_radius: f32) -> f32 {
    stadium_ground_radius(bowl_outer_radius)
}

/// Horizontal disc with radial colour/alpha fade so the outer rim blends into sky.
pub fn stadium_ground_disc_mesh(radius: f32, segments: usize) -> Mesh {
    const BASE_RGB: [f32; 3] = [0.431, 0.424, 0.392]; // apron beige
    const SKY_RGB: [f32; 3] = [0.42, 0.62, 0.82]; // day horizon tint
    const RINGS: usize = 12;
    const FADE_START: f32 = 0.86;

    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();

    let mut ring_start = 0u32;
    for ring in 0..=RINGS {
        let frac = ring as f32 / RINGS as f32;
        let r = frac * radius;
        let fade = ((frac - FADE_START) / (1.0 - FADE_START)).clamp(0.0, 1.0);
        let rgb = [
            BASE_RGB[0] + (SKY_RGB[0] - BASE_RGB[0]) * fade,
            BASE_RGB[1] + (SKY_RGB[1] - BASE_RGB[1]) * fade,
            BASE_RGB[2] + (SKY_RGB[2] - BASE_RGB[2]) * fade,
        ];

        let base = positions.len() as u32;
        if ring == 0 {
            positions.push([0.0, 0.0, 0.0]);
            normals.push([0.0, 1.0, 0.0]);
            uvs.push([0.5, 0.5]);
            colors.push([rgb[0], rgb[1], rgb[2], 1.0]);
        } else {
            for i in 0..segments {
                let t = i as f32 / segments as f32 * TAU;
                positions.push([t.cos() * r, 0.0, t.sin() * r]);
                normals.push([0.0, 1.0, 0.0]);
                uvs.push([t.cos() * 0.5 * frac + 0.5, t.sin() * 0.5 * frac + 0.5]);
                colors.push([rgb[0], rgb[1], rgb[2], 1.0]);
            }
        }

        if ring > 0 {
            let prev_base = ring_start;
            let curr_base = base;
            for i in 0..segments {
                let i0 = i as u32;
                let i1 = ((i + 1) % segments) as u32;
                if ring == 1 {
                    indices.extend_from_slice(&[prev_base, curr_base + i1, curr_base + i0]);
                } else {
                    indices.extend_from_slice(&[prev_base + i0, curr_base + i1, curr_base + i0]);
                    indices.extend_from_slice(&[prev_base + i0, prev_base + i1, curr_base + i1]);
                }
            }
        }
        ring_start = base;
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Ring-oriented box dimensions for one stadium segment.
pub struct RingBoxSpec {
    pub angle: f32,
    pub radius: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub depth: f32,
}

/// Merged mesh of ring-oriented boxes.
pub fn ring_boxes_mesh(specs: &[RingBoxSpec]) -> Mesh {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();

    for spec in specs {
        let xf = ring_segment_transform(spec.angle, spec.radius, spec.y);
        append_oriented_box(
            &mut positions,
            &mut normals,
            &mut uvs,
            &mut indices,
            xf,
            Vec3::new(spec.width * 0.5, spec.height * 0.5, spec.depth * 0.5),
        );
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Build ring box specs for a uniform annular band (tiers, concourse, canopy, facades).
pub fn ring_band_specs(
    segments: usize,
    skip_every: usize,
    radius: f32,
    y: f32,
    width: f32,
    height: f32,
    depth: f32,
) -> Vec<RingBoxSpec> {
    let mut specs = Vec::with_capacity(segments);
    for seg in 0..segments {
        if skip_every > 0 && seg % skip_every == 0 {
            continue;
        }
        let mid = (seg as f32 + 0.5) / segments as f32 * TAU;
        specs.push(RingBoxSpec {
            angle: mid,
            radius,
            y,
            width,
            height,
            depth,
        });
    }
    specs
}

/// Closed horizontal ring tube (boundary rope) at `radius` and `y`.
pub fn ring_tube_mesh(radius: f32, y: f32, segments: usize, tube_radius: f32) -> Mesh {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();

    for seg in 0..segments {
        let a0 = seg as f32 / segments as f32 * TAU;
        let a1 = (seg + 1) as f32 / segments as f32 * TAU;
        let mid = (a0 + a1) * 0.5;
        let len = 2.0 * radius * (PI / segments as f32);
        let xf = ring_segment_transform(mid, radius, y);
        append_oriented_box(
            &mut positions,
            &mut normals,
            &mut uvs,
            &mut indices,
            xf,
            Vec3::new(len * 0.5, tube_radius, tube_radius),
        );
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

fn append_oriented_box(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    uvs: &mut Vec<[f32; 2]>,
    indices: &mut Vec<u32>,
    transform: Transform,
    half: Vec3,
) {
    const FACE_CORNERS: [[(f32, f32, f32); 4]; 6] = [
        [(1., 1., -1.), (-1., 1., -1.), (-1., 1., 1.), (1., 1., 1.)],
        [
            (1., -1., 1.),
            (-1., -1., 1.),
            (-1., -1., -1.),
            (1., -1., -1.),
        ],
        [(1., 1., 1.), (-1., 1., 1.), (-1., -1., 1.), (1., -1., 1.)],
        [
            (-1., 1., -1.),
            (1., 1., -1.),
            (1., -1., -1.),
            (-1., -1., -1.),
        ],
        [
            (-1., 1., 1.),
            (-1., 1., -1.),
            (-1., -1., -1.),
            (-1., -1., 1.),
        ],
        [(1., 1., -1.), (1., 1., 1.), (1., -1., 1.), (1., -1., -1.)],
    ];
    const FACE_NORMALS: [Vec3; 6] = [
        Vec3::Y,
        Vec3::NEG_Y,
        Vec3::Z,
        Vec3::NEG_Z,
        Vec3::NEG_X,
        Vec3::X,
    ];

    let base = positions.len() as u32;
    for (face, corners) in FACE_CORNERS.iter().enumerate() {
        let n = FACE_NORMALS[face].to_array();
        for (i, &(lx, ly, lz)) in corners.iter().enumerate() {
            let local = Vec3::new(lx * half.x, ly * half.y, lz * half.z);
            positions.push(transform.transform_point(local).to_array());
            normals.push(n);
            uvs.push([(i as f32 % 2.0), (i as f32 * 0.5).fract()]);
        }
        let f = base + face as u32 * 4;
        indices.extend_from_slice(&[f, f + 1, f + 2, f, f + 2, f + 3]);
    }
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

    #[test]
    fn ground_disc_within_sky_dome() {
        let bowl_outer = 101.0;
        let r = stadium_ground_radius(bowl_outer);
        assert!(r < 580.0, "disc radius {r} must stay inside 600 m sky dome");
    }

    #[test]
    fn ring_boxes_mesh_has_triangles() {
        let specs = ring_band_specs(8, 0, 50.0, 1.0, 2.0, 0.5, 1.0);
        let mesh = ring_boxes_mesh(&specs);
        let positions = mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap();
        assert!(positions.len() > 0);
    }
}

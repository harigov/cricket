//! Fine-detail mesh builders for the seating bowl: seats, vomitories, aisle
//! stairs, the cantilever roof and the outer facade.
//!
//! Everything here returns a *single merged mesh* carrying
//! [`Mesh::ATTRIBUTE_COLOR`]. A real bowl holds tens of thousands of seats and
//! several hundred roof members; one entity per part would dominate frame time
//! and Bevy exposes no instancing API here, so each subsystem is baked into one
//! large vertex-coloured mesh sharing a single material handle. Colour variation
//! that would otherwise need a material per part rides on the vertices instead.
//!
//! Local frames follow [`crate::render::ring_geometry`]: `+X` is the ring
//! tangent, `+Z` points inward toward the pitch, `+Y` is up.

use std::f32::consts::TAU;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::render::ring_geometry::{ring_position, ring_segment_transform, ring_tangent};

/// Deterministic 32-bit integer hash. Same mixing idiom as
/// [`crate::render::sky::sky_hash`] so the stadium is identical run to run;
/// spawn code must never reach for `rand`.
pub(crate) fn stand_hash(a: u32, b: u32, seed: u32) -> u32 {
    let mut n = a
        .wrapping_mul(374_761_393)
        .wrapping_add(b.wrapping_mul(668_265_263))
        .wrapping_add(seed.wrapping_mul(982_451_653));
    n = (n ^ (n >> 13)).wrapping_mul(1_274_126_177);
    n ^ (n >> 16)
}

/// [`stand_hash`] mapped into `[0, 1)`.
pub(crate) fn stand_unit(a: u32, b: u32, seed: u32) -> f32 {
    (stand_hash(a, b, seed) & 0x00FF_FFFF) as f32 / 16_777_216.0
}

/// True when a merged mesh came back with no geometry, so the caller can skip
/// the entity entirely rather than spawn an invisible draw.
pub(crate) fn mesh_is_empty(mesh: &Mesh) -> bool {
    mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        .is_none_or(|p| p.is_empty())
}

// ---------------------------------------------------------------------------
// Merged mesh builder
// ---------------------------------------------------------------------------

/// Unit-box face corners, wound counter-clockwise seen from outside.
const BOX_FACE_CORNERS: [[(f32, f32, f32); 4]; 6] = [
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
const BOX_FACE_NORMALS: [Vec3; 6] = [
    Vec3::Y,
    Vec3::NEG_Y,
    Vec3::Z,
    Vec3::NEG_Z,
    Vec3::NEG_X,
    Vec3::X,
];
/// Index of the `-Y` face in [`BOX_FACE_CORNERS`].
const FACE_BOTTOM: usize = 1;

const QUAD_UVS: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];

/// Accumulator for merged, vertex-coloured stadium geometry.
#[derive(Default)]
pub(crate) struct StandMesh {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl StandMesh {
    /// For small fittings where pre-sizing buys nothing.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Pre-size for an expected vertex count. Seat bands push hundreds of
    /// thousands of vertices and the reallocation churn shows up in
    /// `build_stadium`'s profile.
    pub(crate) fn with_capacity(vertices: usize) -> Self {
        Self {
            positions: Vec::with_capacity(vertices),
            normals: Vec::with_capacity(vertices),
            uvs: Vec::with_capacity(vertices),
            colors: Vec::with_capacity(vertices),
            indices: Vec::with_capacity(vertices / 2 * 3),
        }
    }

    /// Quad wound counter-clockwise when viewed from its front face.
    pub(crate) fn push_quad(&mut self, corners: [Vec3; 4], color: [f32; 4]) {
        let normal = (corners[1] - corners[0])
            .cross(corners[2] - corners[0])
            .normalize_or_zero()
            .to_array();
        let base = self.positions.len() as u32;
        for (i, c) in corners.iter().enumerate() {
            self.positions.push(c.to_array());
            self.normals.push(normal);
            self.uvs.push(QUAD_UVS[i]);
            self.colors.push(color);
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Both windings of a quad, for panels read from above and below.
    pub(crate) fn push_quad_double_sided(&mut self, corners: [Vec3; 4], color: [f32; 4]) {
        self.push_quad(corners, color);
        self.push_quad([corners[3], corners[2], corners[1], corners[0]], color);
    }

    /// Oriented box. `half` is the half-extent in the box's own local frame.
    pub(crate) fn push_box(&mut self, xf: Transform, half: Vec3, color: [f32; 4]) {
        self.push_box_faces(xf, half, color, false);
    }

    /// Oriented box with the downward face omitted. Seats are never seen from
    /// directly underneath, and dropping the face trims a sixth of the bowl's
    /// vertex budget.
    pub(crate) fn push_box_open_bottom(&mut self, xf: Transform, half: Vec3, color: [f32; 4]) {
        self.push_box_faces(xf, half, color, true);
    }

    fn push_box_faces(&mut self, xf: Transform, half: Vec3, color: [f32; 4], skip_bottom: bool) {
        for (face, corners) in BOX_FACE_CORNERS.iter().enumerate() {
            if skip_bottom && face == FACE_BOTTOM {
                continue;
            }
            let n = (xf.rotation * BOX_FACE_NORMALS[face]).to_array();
            let base = self.positions.len() as u32;
            for (i, &(lx, ly, lz)) in corners.iter().enumerate() {
                let local = Vec3::new(lx * half.x, ly * half.y, lz * half.z);
                self.positions.push(xf.transform_point(local).to_array());
                self.normals.push(n);
                self.uvs.push(QUAD_UVS[i]);
                self.colors.push(color);
            }
            self.indices
                .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }

    /// Ring-aligned box: `size.x` along the tangent, `size.z` along the radius.
    pub(crate) fn push_ring_box(
        &mut self,
        angle: f32,
        radius: f32,
        y: f32,
        size: Vec3,
        color: [f32; 4],
    ) {
        self.push_box(ring_segment_transform(angle, radius, y), size * 0.5, color);
    }

    /// Square-section strut spanning `a`..`b` — the workhorse for roof trusses,
    /// screen supports and camera gantries.
    pub(crate) fn push_strut(&mut self, a: Vec3, b: Vec3, thickness: f32, color: [f32; 4]) {
        let delta = b - a;
        let len = delta.length();
        if len < 1e-4 {
            return;
        }
        let xf = Transform::from_translation((a + b) * 0.5)
            .with_rotation(Quat::from_rotation_arc(Vec3::Y, delta / len));
        self.push_box(xf, Vec3::new(thickness, len * 0.5, thickness), color);
    }

    pub(crate) fn build(self) -> Mesh {
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.colors);
        mesh.insert_indices(Indices::U32(self.indices));
        mesh
    }
}

// ---------------------------------------------------------------------------
// Seats
// ---------------------------------------------------------------------------

/// Ring segments in a seat row. The treads, risers, aisles, stairs and seats all
/// share this segmentation (`stadium::TIER_SEGMENTS`), and so must the crowd —
/// two independent grids is exactly how spectators ended up sitting on the floor
/// between the seats.
pub(crate) const SEAT_SEGMENTS: usize = 96;
/// Every `n`th segment is left open as a stair aisle (`stadium::AISLE_EVERY`).
pub(crate) const SEAT_AISLE_EVERY: usize = 8;

/// Seat pitch along a row. Real stadium seats sit at 0.50–0.58 m centres.
const SEAT_PITCH: f32 = 0.56;
/// Fraction of the pitch filled by the seat shell; the remainder is the gap
/// between neighbours, which is what makes a row read as seats at distance.
const SEAT_SHELL_FRAC: f32 = 0.78;
const SEAT_PAN_DEPTH: f32 = 0.46;
const SEAT_PAN_THICKNESS: f32 = 0.07;
/// Pan height above the tread — standard seat height.
const SEAT_PAN_HEIGHT: f32 = 0.42;
const SEAT_BACK_THICKNESS: f32 = 0.09;
/// Backrest crown height above the tread.
const SEAT_BACK_TOP: f32 = 0.88;
/// Inward radial offset of the seat pan from the tread mid radius, leaving the
/// rest of the tread as the walkway in front of the row.
const SEAT_PAN_INWARD: f32 = 0.15;
/// Top face of the pan: the height a seated pelvis rests at.
const SEAT_HIP_HEIGHT: f32 = SEAT_PAN_HEIGHT + SEAT_PAN_THICKNESS * 0.5;
/// How far back from the pan centre that pelvis sits. Nobody perches on the
/// middle of a stadium seat; they sit back until their spine meets the backrest.
const SEAT_HIP_SETBACK: f32 = 0.08;

/// Base seat tones cycled row to row.
pub(crate) const SEAT_TONE_COUNT: usize = 3;
/// Segments spanned by one upper-deck mosaic block.
const MOSAIC_BLOCK_SEGMENTS: usize = 6;
/// Rows spanned by one upper-deck mosaic block.
const MOSAIC_BLOCK_ROWS: usize = 2;

/// Seat colours for one stadium, derived from its stand tint.
pub(crate) struct SeatPalette {
    /// Base tones cycled row to row for subtle banding.
    pub(crate) tones: [[f32; 3]; SEAT_TONE_COUNT],
    /// Contrasting tone used by the upper-deck mosaic blocks.
    pub(crate) accent: [f32; 3],
}

/// One row of seats on a tier tread.
pub(crate) struct SeatBand {
    pub(crate) segments: usize,
    /// Segments where `seg % aisle_every == 0` stay empty for the aisle.
    pub(crate) aisle_every: usize,
    /// Tread mid radius (see `BowlLayout::tier_mid_radius`).
    pub(crate) radius: f32,
    /// World height of the tread's walking surface.
    pub(crate) tread_top: f32,
    /// Tier index — drives the row-to-row tone cycle.
    pub(crate) row: usize,
    /// Paint the upper-deck block mosaic into this row.
    pub(crate) mosaic: bool,
}

impl SeatBand {
    /// The layout this band's seats are cut from.
    pub(crate) fn grid(&self) -> SeatGrid {
        SeatGrid {
            segments: self.segments,
            aisle_every: self.aisle_every,
            radius: self.radius,
            tread_top: self.tread_top,
        }
    }
}

/// Where the seats in one row are, and where the people in them go.
///
/// This is the single source of truth for seat placement: [`seat_band_mesh`]
/// cuts the shells from it and [`crate::render::crowd`] seats its spectators
/// from the same call, so a spectator is *in* seat `(seg, k)` by construction and
/// cannot drift out of alignment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SeatGrid {
    pub(crate) segments: usize,
    /// Segments where `seg % aisle_every == 0` stay empty for the aisle.
    pub(crate) aisle_every: usize,
    /// Tread mid radius (see `BowlLayout::tier_mid_radius`).
    pub(crate) radius: f32,
    /// World height of the tread's walking surface.
    pub(crate) tread_top: f32,
}

impl SeatGrid {
    /// Grid for a tread at `radius`, on the bowl's canonical segmentation.
    pub(crate) fn on_tread(radius: f32, tread_top: f32) -> Self {
        Self {
            segments: SEAT_SEGMENTS,
            aisle_every: SEAT_AISLE_EVERY,
            radius,
            tread_top,
        }
    }

    pub(crate) fn seats_per_segment(self) -> usize {
        seats_per_segment(self.radius, self.segments)
    }

    pub(crate) fn is_aisle(self, seg: usize) -> bool {
        segment_is_aisle(seg, self.aisle_every)
    }

    /// Seats in the whole row — the reference count the mesh must reproduce.
    pub(crate) fn total_seats(self) -> usize {
        (0..self.segments).filter(|s| !self.is_aisle(*s)).count() * self.seats_per_segment()
    }

    /// Centre-to-centre spacing actually achieved. A whole number of seats has
    /// to fit each segment, so this lands near [`SEAT_PITCH`] rather than on it.
    pub(crate) fn seat_pitch(self) -> f32 {
        TAU * self.radius / (self.segments * self.seats_per_segment()) as f32
    }

    /// Ring angle of seat `k` in segment `seg` — also the way that seat faces.
    pub(crate) fn seat_angle(self, seg: usize, k: usize) -> f32 {
        let per = self.seats_per_segment() as f32;
        (seg as f32 + (k as f32 + 0.5) / per) / self.segments as f32 * TAU
    }

    /// Frame of one seat: origin on the tread under the seat's centre line, `+X`
    /// along the row, `+Z` toward the pitch, `+Y` up. Everything fitted to a
    /// seat — shell, occupant, occupant's knees — is a local offset in here.
    pub(crate) fn seat_frame(self, seg: usize, k: usize) -> Transform {
        ring_segment_transform(self.seat_angle(seg, k), self.radius, self.tread_top)
    }

    /// Local offset of a seated pelvis inside [`SeatGrid::seat_frame`].
    pub(crate) fn hip_offset(self) -> Vec3 {
        Vec3::new(0.0, SEAT_HIP_HEIGHT, SEAT_PAN_INWARD - SEAT_HIP_SETBACK)
    }

    /// World height of a seated pelvis: the top face of the pan.
    pub(crate) fn hip_height(self) -> f32 {
        self.tread_top + SEAT_HIP_HEIGHT
    }

    /// World position of the pelvis of whoever occupies seat `(seg, k)`.
    pub(crate) fn seat_hip(self, seg: usize, k: usize) -> Vec3 {
        self.seat_frame(seg, k).transform_point(self.hip_offset())
    }

    /// Tread directly under that pelvis, where the occupant's feet land.
    pub(crate) fn seat_foot(self, seg: usize, k: usize) -> Vec3 {
        let hip = self.hip_offset();
        self.seat_frame(seg, k)
            .transform_point(Vec3::new(hip.x, 0.0, hip.z))
    }

    /// Backrest crown — the line an occupant's head has to clear to read.
    pub(crate) fn backrest_top(self) -> f32 {
        self.tread_top + SEAT_BACK_TOP
    }

    /// Every seat in the row, aisle segments skipped.
    pub(crate) fn seats(self) -> impl Iterator<Item = (usize, usize)> {
        let per = self.seats_per_segment();
        (0..self.segments)
            .filter(move |seg| !self.is_aisle(*seg))
            .flat_map(move |seg| (0..per).map(move |k| (seg, k)))
    }
}

/// Seats fitted into one ring segment at this radius.
pub(crate) fn seats_per_segment(radius: f32, segments: usize) -> usize {
    let arc = TAU * radius / segments as f32;
    ((arc / SEAT_PITCH).round() as usize).max(1)
}

/// Total seats in a band — the reference count the mesh must reproduce.
pub(crate) fn seat_band_count(band: &SeatBand) -> usize {
    band.grid().total_seats()
}

/// True when a ring segment is left open as a stair aisle. Mirrors the skip rule
/// in [`crate::render::ring_geometry::ring_band_specs`].
pub(crate) fn segment_is_aisle(seg: usize, aisle_every: usize) -> bool {
    aisle_every > 0 && seg.is_multiple_of(aisle_every)
}

/// Upper-deck mosaic: contiguous blocks of contrasting seats, the way real
/// grounds pick out patterns across a sparsely filled upper tier.
pub(crate) fn mosaic_block_is_accent(seg: usize, row: usize) -> bool {
    let bx = (seg / MOSAIC_BLOCK_SEGMENTS) as u32;
    let by = (row / MOSAIC_BLOCK_ROWS) as u32;
    stand_hash(bx, by, 0x5EA7).is_multiple_of(3)
}

fn seat_color(palette: &SeatPalette, band: &SeatBand, seg: usize, k: usize) -> [f32; 4] {
    let base = if band.mosaic && mosaic_block_is_accent(seg, band.row) {
        palette.accent
    } else {
        palette.tones[band.row % SEAT_TONE_COUNT]
    };
    // Per-seat brightness jitter breaks up the banding without a material per
    // seat: sun-faded plastic is never perfectly uniform.
    let jitter = 0.93 + stand_unit(seg as u32, (band.row * 97 + k) as u32, 0x53EA) * 0.12;
    [
        (base[0] * jitter).clamp(0.0, 1.0),
        (base[1] * jitter).clamp(0.0, 1.0),
        (base[2] * jitter).clamp(0.0, 1.0),
        1.0,
    ]
}

/// Merged mesh for one row of seats: pan, backrest and the gap between seats.
pub(crate) fn seat_band_mesh(band: &SeatBand, palette: &SeatPalette) -> Mesh {
    let grid = band.grid();
    // 2 boxes per seat, 5 faces each (no underside), 4 vertices per face.
    let mut m = StandMesh::with_capacity(grid.total_seats() * 40);
    let shell_w = grid.seat_pitch() * SEAT_SHELL_FRAC;

    // The backrest runs from just above the tread to its crown, so it doubles as
    // the visible support for the pan.
    const BACK_LOW: f32 = 0.06;
    let back_z = SEAT_PAN_INWARD - (SEAT_PAN_DEPTH + SEAT_BACK_THICKNESS) * 0.5;
    let back_h = SEAT_BACK_TOP - BACK_LOW;

    for (seg, k) in grid.seats() {
        let frame = grid.seat_frame(seg, k);
        let color = seat_color(palette, band, seg, k);
        m.push_box_open_bottom(
            frame * Transform::from_xyz(0.0, SEAT_PAN_HEIGHT, SEAT_PAN_INWARD),
            Vec3::new(
                shell_w * 0.5,
                SEAT_PAN_THICKNESS * 0.5,
                SEAT_PAN_DEPTH * 0.5,
            ),
            color,
        );
        m.push_box_open_bottom(
            frame * Transform::from_xyz(0.0, (BACK_LOW + SEAT_BACK_TOP) * 0.5, back_z),
            Vec3::new(shell_w * 0.5, back_h * 0.5, SEAT_BACK_THICKNESS * 0.5),
            color,
        );
    }
    m.build()
}

// ---------------------------------------------------------------------------
// Vomitories and aisle stairs
// ---------------------------------------------------------------------------

/// A tunnel mouth cut through the seating bowl.
pub(crate) struct Vomitory {
    /// Centre angle — always an aisle centre so it never eats into a seat row.
    pub(crate) angle: f32,
    /// Radius of the mouth opening (the face seen from the pitch).
    pub(crate) mouth_radius: f32,
    /// Radial depth of the tunnel bore, running outward from the mouth.
    pub(crate) depth: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
    /// Floor level at the mouth.
    pub(crate) floor_y: f32,
}

/// Aisle centre angles: the midpoint of every gap left by
/// [`crate::render::ring_geometry::ring_band_specs`].
pub(crate) fn aisle_angles(segments: usize, aisle_every: usize) -> Vec<f32> {
    (0..segments)
        .filter(|seg| segment_is_aisle(*seg, aisle_every))
        .map(|seg| (seg as f32 + 0.5) / segments as f32 * TAU)
        .collect()
}

/// Every `nth` aisle carries a tunnel mouth; the rest stay plain stair aisles.
pub(crate) fn vomitory_angles(segments: usize, aisle_every: usize, nth: usize) -> Vec<f32> {
    let nth = nth.max(1);
    aisle_angles(segments, aisle_every)
        .into_iter()
        .enumerate()
        .filter(|(i, _)| i.is_multiple_of(nth))
        .map(|(_, a)| a)
        .collect()
}

/// The dark bore behind each mouth: floor, soffit, side walls and a back wall,
/// wound to face inward so the camera looks *into* a tunnel.
pub(crate) fn vomitory_interior_mesh(voms: &[Vomitory]) -> Mesh {
    const FLOOR: [f32; 4] = [0.10, 0.10, 0.11, 1.0];
    const WALL: [f32; 4] = [0.055, 0.058, 0.065, 1.0];
    const SOFFIT: [f32; 4] = [0.03, 0.03, 0.035, 1.0];
    const BACK: [f32; 4] = [0.015, 0.016, 0.02, 1.0];

    let mut m = StandMesh::with_capacity(voms.len() * 32);
    for v in voms {
        let r0 = v.mouth_radius;
        let r1 = v.mouth_radius + v.depth;
        let hw = v.width * 0.5;
        let y0 = v.floor_y;
        let y1 = v.floor_y + v.height;
        let tangent = ring_tangent(v.angle);
        let corner = |r: f32, x: f32, y: f32| ring_position(v.angle, r, y) + tangent * x;

        m.push_quad(
            [
                corner(r0, -hw, y0),
                corner(r0, hw, y0),
                corner(r1, hw, y0),
                corner(r1, -hw, y0),
            ],
            FLOOR,
        );
        m.push_quad(
            [
                corner(r1, -hw, y1),
                corner(r1, hw, y1),
                corner(r0, hw, y1),
                corner(r0, -hw, y1),
            ],
            SOFFIT,
        );
        m.push_quad(
            [
                corner(r0, -hw, y0),
                corner(r1, -hw, y0),
                corner(r1, -hw, y1),
                corner(r0, -hw, y1),
            ],
            WALL,
        );
        m.push_quad(
            [
                corner(r1, hw, y0),
                corner(r0, hw, y0),
                corner(r0, hw, y1),
                corner(r1, hw, y1),
            ],
            WALL,
        );
        m.push_quad(
            [
                corner(r1, hw, y0),
                corner(r1, hw, y1),
                corner(r1, -hw, y1),
                corner(r1, -hw, y0),
            ],
            BACK,
        );
    }
    m.build()
}

/// Concrete surround for each tunnel mouth: lintel, jambs and the steps that
/// drop from the mouth onto the tread in front.
pub(crate) fn vomitory_frame_mesh(voms: &[Vomitory]) -> Mesh {
    const CONCRETE: [f32; 4] = [0.62, 0.61, 0.58, 1.0];
    const LINTEL: [f32; 4] = [0.70, 0.69, 0.66, 1.0];
    const STEP: [f32; 4] = [0.56, 0.55, 0.53, 1.0];
    const STEPS: usize = 3;
    const JAMB_W: f32 = 0.34;

    let mut m = StandMesh::with_capacity(voms.len() * 96);
    for v in voms {
        let r = v.mouth_radius + 0.18;
        for side in [-1.0_f32, 1.0] {
            m.push_box(
                ring_segment_transform(v.angle, r, v.floor_y + v.height * 0.5)
                    * Transform::from_xyz(side * (v.width * 0.5 + JAMB_W * 0.5), 0.0, 0.0),
                Vec3::new(JAMB_W * 0.5, v.height * 0.5, 0.42),
                CONCRETE,
            );
        }
        m.push_ring_box(
            v.angle,
            r,
            v.floor_y + v.height + 0.28,
            Vec3::new(v.width + JAMB_W * 2.0, 0.56, 0.96),
            LINTEL,
        );
        for s in 0..STEPS {
            let t = (s + 1) as f32;
            m.push_ring_box(
                v.angle,
                v.mouth_radius - t * 0.34,
                v.floor_y - t * 0.16,
                Vec3::new(v.width * 0.94, 0.20, 0.36),
                STEP,
            );
        }
    }
    m.build()
}

/// One flight of aisle steps climbing a single tier.
pub(crate) struct StairFlight {
    pub(crate) angle: f32,
    /// Radius of the bottom nosing.
    pub(crate) inner_radius: f32,
    /// Radial run of the flight.
    pub(crate) run: f32,
    /// Total rise of the flight.
    pub(crate) rise: f32,
    /// Level the flight starts from (the tread it springs off).
    pub(crate) base_y: f32,
    /// Level the steps are built down to. Aisles are gaps in every tread ring,
    /// so a flight has to be solid concrete rather than floating nosings.
    pub(crate) foot_y: f32,
    pub(crate) width: f32,
    pub(crate) steps: usize,
}

/// Total step boxes across all flights — the reference count for tests.
pub(crate) fn stair_step_count(flights: &[StairFlight]) -> usize {
    flights.iter().map(|f| f.steps).sum()
}

/// Merged mesh of every aisle stair step.
pub(crate) fn stair_flights_mesh(flights: &[StairFlight]) -> Mesh {
    let mut m = StandMesh::with_capacity(stair_step_count(flights) * 24);
    for (fi, f) in flights.iter().enumerate() {
        if f.steps == 0 {
            continue;
        }
        let going = f.run / f.steps as f32;
        let rise = f.rise / f.steps as f32;
        for s in 0..f.steps {
            let nosing = f.base_y + (s + 1) as f32 * rise;
            let height = (nosing - f.foot_y).max(0.10);
            // Nosing brightness alternates so treads read individually.
            let tone = 0.60 + ((s + fi) % 2) as f32 * 0.06;
            m.push_ring_box(
                f.angle,
                f.inner_radius + (s as f32 + 0.5) * going,
                nosing - height * 0.5,
                Vec3::new(f.width, height, going),
                [tone, tone * 0.99, tone * 0.95, 1.0],
            );
        }
    }
    m.build()
}

// ---------------------------------------------------------------------------
// Cantilever roof
// ---------------------------------------------------------------------------

/// Geometry of the cantilever roof over the upper deck.
pub(crate) struct RoofSpec {
    /// Radial trusses springing from the rear columns.
    pub(crate) truss_count: usize,
    /// Radius of the free cantilever tip / tension ring.
    pub(crate) inner_radius: f32,
    /// Radius of the rear support line.
    pub(crate) outer_radius: f32,
    pub(crate) inner_y: f32,
    pub(crate) outer_y: f32,
    /// Structural depth of the truss at the rear support.
    pub(crate) depth: f32,
    /// Member half-thickness.
    pub(crate) member: f32,
    /// Upward crown at mid span (0 = straight slope).
    pub(crate) camber: f32,
    /// Web panels per truss.
    pub(crate) web_panels: usize,
}

/// Evenly spaced truss angles.
pub(crate) fn roof_truss_angles(count: usize) -> Vec<f32> {
    (0..count).map(|i| i as f32 / count as f32 * TAU).collect()
}

impl RoofSpec {
    /// Top-chord point at normalised span `t` (0 = cantilever tip, 1 = rear).
    fn top_chord(&self, angle: f32, t: f32) -> Vec3 {
        let r = self.inner_radius + (self.outer_radius - self.inner_radius) * t;
        let y = self.inner_y
            + (self.outer_y - self.inner_y) * t
            + self.camber * (t * std::f32::consts::PI).sin();
        ring_position(angle, r, y)
    }

    /// Bottom-chord point at normalised span `t`. The truss tapers to nothing at
    /// the tip, which is what gives a cantilever its wedge profile.
    fn bottom_chord(&self, angle: f32, t: f32) -> Vec3 {
        self.top_chord(angle, t) - Vec3::Y * (self.depth * t)
    }

    /// Roof bays around the ring (one between each adjacent pair of trusses).
    pub(crate) fn panel_count(&self) -> usize {
        self.truss_count
    }
}

/// Every fourth roof bay is glazed so daylight reaches the back rows.
pub(crate) fn roof_panel_is_translucent(bay: usize) -> bool {
    bay % 4 == 2
}

/// Chords plus web bracing for every radial truss, as one mesh.
pub(crate) fn roof_truss_mesh(spec: &RoofSpec) -> Mesh {
    const STEEL: [f32; 4] = [0.74, 0.75, 0.78, 1.0];
    const WEB: [f32; 4] = [0.62, 0.63, 0.66, 1.0];

    let panels = spec.web_panels.max(1);
    let mut m = StandMesh::with_capacity(spec.truss_count * (panels * 4 + 1) * 24);
    for &angle in &roof_truss_angles(spec.truss_count) {
        for i in 0..panels {
            let t0 = i as f32 / panels as f32;
            let t1 = (i + 1) as f32 / panels as f32;
            let top0 = spec.top_chord(angle, t0);
            let top1 = spec.top_chord(angle, t1);
            let bot0 = spec.bottom_chord(angle, t0);
            let bot1 = spec.bottom_chord(angle, t1);
            m.push_strut(top0, top1, spec.member, STEEL);
            m.push_strut(bot0, bot1, spec.member * 0.9, STEEL);
            // Alternating diagonals plus a post at each panel point.
            if i.is_multiple_of(2) {
                m.push_strut(bot0, top1, spec.member * 0.62, WEB);
            } else {
                m.push_strut(top0, bot1, spec.member * 0.62, WEB);
            }
            m.push_strut(top1, bot1, spec.member * 0.62, WEB);
        }
        // Rear leg dropping onto the columns behind the back row.
        let rear = spec.top_chord(angle, 1.0);
        m.push_strut(
            rear,
            rear - Vec3::Y * (spec.depth + 2.4),
            spec.member * 1.25,
            STEEL,
        );
    }
    m.build()
}

/// Roof deck panels for the requested opacity class, one quad strip per bay.
pub(crate) fn roof_panel_mesh(spec: &RoofSpec, translucent: bool) -> Mesh {
    const RADIAL_STEPS: usize = 4;
    let color = if translucent {
        [0.82, 0.86, 0.92, 1.0]
    } else {
        [0.70, 0.71, 0.74, 1.0]
    };

    let angles = roof_truss_angles(spec.truss_count);
    let mut m = StandMesh::with_capacity(spec.panel_count() * RADIAL_STEPS * 8);
    for bay in 0..spec.panel_count() {
        if roof_panel_is_translucent(bay) != translucent {
            continue;
        }
        let a0 = angles[bay];
        // The last bay closes back onto truss 0, so unwrap the angle.
        let next = angles[(bay + 1) % angles.len()];
        let a1 = if next <= a0 { next + TAU } else { next };
        for s in 0..RADIAL_STEPS {
            let t0 = s as f32 / RADIAL_STEPS as f32;
            let t1 = (s + 1) as f32 / RADIAL_STEPS as f32;
            let lift = Vec3::Y * spec.member;
            m.push_quad_double_sided(
                [
                    spec.top_chord(a0, t0) + lift,
                    spec.top_chord(a1, t0) + lift,
                    spec.top_chord(a1, t1) + lift,
                    spec.top_chord(a0, t1) + lift,
                ],
                color,
            );
        }
    }
    m.build()
}

/// Tension ring tying the cantilever tips together, plus the fascia band that
/// gives the roof edge a visible thickness from the pitch.
pub(crate) fn roof_edge_mesh(spec: &RoofSpec) -> Mesh {
    const RING: [f32; 4] = [0.80, 0.81, 0.84, 1.0];
    const FASCIA: [f32; 4] = [0.30, 0.31, 0.34, 1.0];

    let segs = (spec.truss_count * 3).max(24);
    let arc = TAU * spec.inner_radius / segs as f32 * 1.04;
    let mut m = StandMesh::with_capacity(segs * 48);
    for s in 0..segs {
        let a = (s as f32 + 0.5) / segs as f32 * TAU;
        m.push_ring_box(
            a,
            spec.inner_radius,
            spec.inner_y - spec.member * 2.0,
            Vec3::new(arc, spec.member * 3.0, spec.member * 3.0),
            RING,
        );
        m.push_ring_box(
            a,
            spec.inner_radius - spec.member * 2.2,
            spec.inner_y - 0.85,
            Vec3::new(arc, 1.10, 0.22),
            FASCIA,
        );
    }
    m.build()
}

/// Roof underside: a continuous soffit so the bowl never sees open sky through
/// the structure, with occlusion shading baked into the vertex colours.
pub(crate) fn roof_soffit_mesh(spec: &RoofSpec) -> Mesh {
    const RADIAL_STEPS: usize = 3;
    let segs = spec.truss_count * 2;
    let mut m = StandMesh::with_capacity(segs * RADIAL_STEPS * 4);
    for s in 0..segs {
        let a0 = s as f32 / segs as f32 * TAU;
        let a1 = (s + 1) as f32 / segs as f32 * TAU;
        for i in 0..RADIAL_STEPS {
            let t0 = i as f32 / RADIAL_STEPS as f32;
            let t1 = (i + 1) as f32 / RADIAL_STEPS as f32;
            let shade = 0.46 - t0 * 0.16;
            let color = [shade, shade * 1.01, shade * 1.04, 1.0];
            let drop = Vec3::Y * -0.12;
            m.push_quad(
                [
                    spec.bottom_chord(a0, t0) + drop,
                    spec.bottom_chord(a0, t1) + drop,
                    spec.bottom_chord(a1, t1) + drop,
                    spec.bottom_chord(a1, t0) + drop,
                ],
                color,
            );
        }
    }
    m.build()
}

/// Light and speaker clusters slung from the roof soffit.
pub(crate) fn roof_cluster_mesh(spec: &RoofSpec, count: usize) -> Mesh {
    const HANGER: [f32; 4] = [0.22, 0.22, 0.24, 1.0];
    const HOUSING: [f32; 4] = [0.14, 0.145, 0.16, 1.0];

    let mut m = StandMesh::with_capacity(count * 96);
    for i in 0..count {
        let a = i as f32 / count as f32 * TAU;
        let t = 0.24 + stand_unit(i as u32, 3, 0xC1C1) * 0.16;
        let anchor = spec.bottom_chord(a, t) - Vec3::Y * 0.12;
        let drop = 1.5 + stand_unit(i as u32, 7, 0xC1C2) * 0.9;
        let hub = anchor - Vec3::Y * drop;
        let hub_r = Vec2::new(hub.x, hub.z).length();
        m.push_strut(anchor, hub, 0.06, HANGER);
        // Speaker box up against the soffit, floodlight bar slung below it.
        m.push_ring_box(a, hub_r, hub.y - 0.30, Vec3::new(1.9, 0.62, 0.70), HOUSING);
        m.push_ring_box(
            a,
            hub_r - 0.55,
            hub.y - 0.72,
            Vec3::new(2.4, 0.26, 0.34),
            HANGER,
        );
    }
    m.build()
}

/// Flags and banners ranged along the roof crown.
pub(crate) fn roof_flag_mesh(spec: &RoofSpec, count: usize, tones: &[[f32; 3]]) -> Mesh {
    const POLE: [f32; 4] = [0.86, 0.87, 0.89, 1.0];

    let mut m = StandMesh::with_capacity(count * 64);
    if tones.is_empty() {
        return m.build();
    }
    for i in 0..count {
        let a = i as f32 / count as f32 * TAU;
        let base = spec.top_chord(a, 0.78) + Vec3::Y * spec.member;
        let pole_h = 3.4 + stand_unit(i as u32, 11, 0xF1A6) * 1.2;
        m.push_strut(base, base + Vec3::Y * pole_h, 0.055, POLE);

        let tone = tones[stand_hash(i as u32, 5, 0xF1A7) as usize % tones.len()];
        let color = [tone[0], tone[1], tone[2], 1.0];
        let flag_h = pole_h * 0.42;
        // A kink at the free edge so the banner reads as cloth, not a decal.
        let sway = 0.22 + stand_unit(i as u32, 13, 0xF1A8) * 0.30;
        let top_l = base + Vec3::Y * (pole_h - 0.15);
        let top_r = top_l + ring_tangent(a) * 1.9 - Vec3::Y * sway;
        m.push_quad_double_sided(
            [
                top_l,
                top_r,
                top_r - Vec3::Y * flag_h,
                top_l - Vec3::Y * flag_h,
            ],
            color,
        );
    }
    m.build()
}

// ---------------------------------------------------------------------------
// Facade, concourse and gates
// ---------------------------------------------------------------------------

/// Outer wall of the stadium: ribs, glazing and crown.
pub(crate) struct FacadeSpec {
    pub(crate) segments: usize,
    pub(crate) radius: f32,
    pub(crate) base_y: f32,
    pub(crate) height: f32,
    pub(crate) rib_width: f32,
    pub(crate) rib_depth: f32,
    /// Horizontal glazing bands between the ribs.
    pub(crate) glazing_bands: usize,
}

/// Vertical pilaster ribs, alternating in projection so raking light carves the
/// wall into bays instead of one flat band.
pub(crate) fn facade_rib_mesh(spec: &FacadeSpec) -> Mesh {
    let mut m = StandMesh::with_capacity(spec.segments * 48);
    for s in 0..spec.segments {
        let a = s as f32 / spec.segments as f32 * TAU;
        let depth = if s.is_multiple_of(3) {
            spec.rib_depth * 1.6
        } else {
            spec.rib_depth
        };
        // Weathered concrete: a little colour noise rib to rib.
        let shade = 0.60 + stand_unit(s as u32, 1, 0xFACE) * 0.10;
        m.push_ring_box(
            a,
            spec.radius + depth * 0.4,
            spec.base_y + spec.height * 0.5,
            Vec3::new(spec.rib_width, spec.height, depth),
            [shade, shade * 0.985, shade * 0.95, 1.0],
        );
        // Capital block where the rib meets the crown.
        m.push_ring_box(
            a,
            spec.radius + depth * 0.4,
            spec.base_y + spec.height + 0.35,
            Vec3::new(spec.rib_width * 1.45, 0.70, depth * 1.3),
            [shade * 1.08, shade * 1.06, shade * 1.02, 1.0],
        );
    }
    m.build()
}

/// Glazing set back between the ribs, in horizontal bands with spandrels.
pub(crate) fn facade_glazing_mesh(spec: &FacadeSpec) -> Mesh {
    let bands = spec.glazing_bands.max(1);
    let arc = TAU * spec.radius / spec.segments as f32;
    let band_h = spec.height / bands as f32;
    let mut m = StandMesh::with_capacity(spec.segments * bands * 24);
    for s in 0..spec.segments {
        let a = (s as f32 + 0.5) / spec.segments as f32 * TAU;
        for b in 0..bands {
            // A spandrel course every third band, so it is not all glass.
            if b % 3 == 2 {
                continue;
            }
            // Per-panel tint variation reads as differing sky reflections.
            let t = 0.55 + stand_unit(s as u32, b as u32, 0x61A5) * 0.35;
            m.push_ring_box(
                a,
                spec.radius - spec.rib_depth * 0.2,
                spec.base_y + (b as f32 + 0.5) * band_h,
                Vec3::new(arc * 0.92, band_h * 0.66, 0.14),
                [t * 0.72, t * 0.86, t, 0.62],
            );
        }
    }
    m.build()
}

/// Parapet crown capping the facade, with a repeating pier pattern.
pub(crate) fn facade_parapet_mesh(spec: &FacadeSpec) -> Mesh {
    let segs = spec.segments * 2;
    let arc = TAU * spec.radius / segs as f32 * 1.04;
    let top = spec.base_y + spec.height + 0.7;
    let mut m = StandMesh::with_capacity(segs * 48);
    for s in 0..segs {
        let a = (s as f32 + 0.5) / segs as f32 * TAU;
        m.push_ring_box(
            a,
            spec.radius + spec.rib_depth * 0.2,
            top + 0.55,
            Vec3::new(arc, 1.10, spec.rib_depth * 2.1),
            [0.66, 0.655, 0.63, 1.0],
        );
        if s.is_multiple_of(4) {
            m.push_ring_box(
                a,
                spec.radius + spec.rib_depth * 0.2,
                top + 1.55,
                Vec3::new(arc * 0.7, 1.0, spec.rib_depth * 2.4),
                [0.70, 0.695, 0.67, 1.0],
            );
        }
    }
    m.build()
}

/// Lit concourse deck read through the facade openings: a bright soffit strip
/// plus the slab edge, so the gap between the decks is not a black void.
pub(crate) fn concourse_reveal_mesh(
    segments: usize,
    radius: f32,
    y: f32,
    depth: f32,
    height: f32,
) -> Mesh {
    let arc = TAU * radius / segments as f32 * 1.02;
    let mut m = StandMesh::with_capacity(segments * 48);
    for s in 0..segments {
        let a = (s as f32 + 0.5) / segments as f32 * TAU;
        // Interior glow varies bay to bay as if lit by separate fittings.
        let glow = 0.52 + stand_unit(s as u32, 2, 0xC0C0) * 0.28;
        m.push_ring_box(
            a,
            radius,
            y + height,
            Vec3::new(arc, 0.16, depth),
            [glow, glow * 0.97, glow * 0.88, 1.0],
        );
        m.push_ring_box(
            a,
            radius - depth * 0.5,
            y - 0.22,
            Vec3::new(arc, 0.44, 0.5),
            [0.48, 0.475, 0.46, 1.0],
        );
    }
    m.build()
}

/// Entry gate angles, evenly spaced around the outer wall.
pub(crate) fn gate_angles(count: usize) -> Vec<f32> {
    (0..count)
        .map(|i| (i as f32 + 0.5) / count as f32 * TAU)
        .collect()
}

/// Ground-level entry gates: portal frame, dark opening and turnstile piers.
pub(crate) fn gate_portal_mesh(angles: &[f32], radius: f32, width: f32, height: f32) -> Mesh {
    const FRAME: [f32; 4] = [0.72, 0.71, 0.68, 1.0];
    const OPENING: [f32; 4] = [0.045, 0.048, 0.055, 1.0];
    const PIER: [f32; 4] = [0.58, 0.575, 0.56, 1.0];

    let mut m = StandMesh::with_capacity(angles.len() * 192);
    for &a in angles {
        m.push_ring_box(
            a,
            radius - 0.5,
            height * 0.5,
            Vec3::new(width, height, 1.6),
            OPENING,
        );
        m.push_ring_box(
            a,
            radius + 0.6,
            height + 0.55,
            Vec3::new(width + 2.2, 1.10, 1.8),
            FRAME,
        );
        for side in [-1.0_f32, 1.0] {
            m.push_box(
                ring_segment_transform(a, radius + 0.6, height * 0.5)
                    * Transform::from_xyz(side * (width * 0.5 + 0.55), 0.0, 0.0),
                Vec3::new(0.55, height * 0.5, 0.9),
                FRAME,
            );
        }
        for k in 0..3 {
            m.push_box(
                ring_segment_transform(a, radius - 1.2, 0.55)
                    * Transform::from_xyz((k as f32 - 1.0) * (width / 3.0), 0.0, 0.0),
                Vec3::new(0.16, 0.55, 0.36),
                PIER,
            );
        }
    }
    m.build()
}

/// Advertising hoarding ring with real UVs, so a sponsor texture maps across
/// each board instead of smearing.
pub(crate) fn hoarding_ring_mesh(
    segments: usize,
    radius: f32,
    y: f32,
    height: f32,
    every: usize,
) -> Mesh {
    let every = every.max(1);
    let arc = TAU * radius / segments as f32;
    let hw = arc * every as f32 * 0.47;
    let hh = height * 0.5;
    let mut m = StandMesh::with_capacity(segments / every * 4);
    for s in (0..segments).step_by(every) {
        let a = (s as f32 + 0.5) / segments as f32 * TAU;
        let xf = ring_segment_transform(a, radius, y);
        // Wound to face the pitch (+Z local) with a 0..1 UV span per board.
        let face = |x: f32, h: f32| xf.transform_point(Vec3::new(x, h, 0.06));
        m.push_quad(
            [face(-hw, -hh), face(hw, -hh), face(hw, hh), face(-hw, hh)],
            [1.0, 1.0, 1.0, 1.0],
        );
    }
    m.build()
}

/// Dark backing box behind a hoarding ring (separate mesh: separate material).
pub(crate) fn hoarding_backing_mesh(
    segments: usize,
    radius: f32,
    y: f32,
    height: f32,
    every: usize,
) -> Mesh {
    let every = every.max(1);
    let arc = TAU * radius / segments as f32;
    let mut m = StandMesh::with_capacity(segments / every * 24);
    for s in (0..segments).step_by(every) {
        let a = (s as f32 + 0.5) / segments as f32 * TAU;
        m.push_ring_box(
            a,
            radius - 0.06,
            y,
            Vec3::new(arc * every as f32 * 0.98, height + 0.16, 0.22),
            [0.06, 0.07, 0.09, 1.0],
        );
    }
    m.build()
}

/// Broadcast camera gantry: a platform cantilevered off the bowl with a
/// pedestal-mounted camera body on it.
pub(crate) fn camera_gantry_mesh(angles: &[f32], radius: f32, y: f32) -> Mesh {
    const DECK: [f32; 4] = [0.30, 0.31, 0.33, 1.0];
    const RAIL: [f32; 4] = [0.78, 0.79, 0.81, 1.0];
    const BODY: [f32; 4] = [0.09, 0.09, 0.10, 1.0];

    let mut m = StandMesh::with_capacity(angles.len() * 216);
    for &a in angles {
        m.push_ring_box(a, radius, y, Vec3::new(3.4, 0.22, 2.4), DECK);
        m.push_ring_box(a, radius + 1.1, y + 0.60, Vec3::new(3.4, 0.10, 0.10), RAIL);
        for side in [-1.0_f32, 1.0] {
            m.push_box(
                ring_segment_transform(a, radius, y + 0.60)
                    * Transform::from_xyz(side * 1.7, 0.0, 0.0),
                Vec3::new(0.05, 0.05, 1.2),
                RAIL,
            );
        }
        // Bracket picking the deck up off the bowl behind it.
        m.push_strut(
            ring_position(a, radius, y - 0.11),
            ring_position(a, radius + 2.2, y - 2.0),
            0.09,
            RAIL,
        );
        m.push_ring_box(a, radius - 0.4, y + 0.55, Vec3::new(0.18, 0.78, 0.18), BODY);
        m.push_ring_box(a, radius - 0.4, y + 1.10, Vec3::new(0.44, 0.34, 0.96), BODY);
        m.push_ring_box(
            a,
            radius - 0.95,
            y + 1.10,
            Vec3::new(0.24, 0.24, 0.30),
            BODY,
        );
    }
    m.build()
}

/// Support truss holding the big screen up off the ground.
pub(crate) fn screen_support_mesh(center: Vec3, half_width: f32, base_y: f32) -> Mesh {
    const STEEL: [f32; 4] = [0.26, 0.27, 0.30, 1.0];

    let mut m = StandMesh::new();
    for side in [-1.0_f32, 1.0] {
        let foot = Vec3::new(center.x + 0.9, base_y, center.z + side * half_width * 0.7);
        let head = Vec3::new(center.x, center.y, center.z + side * half_width * 0.8);
        m.push_strut(foot, head, 0.16, STEEL);
        m.push_strut(
            foot,
            Vec3::new(center.x, center.y * 0.55, center.z),
            0.10,
            STEEL,
        );
    }
    // Horizontal tie behind the panel.
    m.push_strut(
        Vec3::new(center.x + 0.6, center.y * 0.6, center.z - half_width),
        Vec3::new(center.x + 0.6, center.y * 0.6, center.z + half_width),
        0.11,
        STEEL,
    );
    m.build()
}

/// Covered player tunnel running from the pavilion out to the field.
pub(crate) fn player_tunnel_mesh(angle: f32, inner_radius: f32, length: f32) -> Mesh {
    const WALL: [f32; 4] = [0.60, 0.595, 0.58, 1.0];
    const ROOF: [f32; 4] = [0.20, 0.205, 0.22, 1.0];
    const DARK: [f32; 4] = [0.03, 0.032, 0.038, 1.0];
    const WIDTH: f32 = 3.2;
    const HEIGHT: f32 = 2.9;

    let mut m = StandMesh::new();
    let mid_r = inner_radius + length * 0.5;
    for side in [-1.0_f32, 1.0] {
        m.push_box(
            ring_segment_transform(angle, mid_r, HEIGHT * 0.5)
                * Transform::from_xyz(side * (WIDTH * 0.5 + 0.2), 0.0, 0.0),
            Vec3::new(0.2, HEIGHT * 0.5, length * 0.5),
            WALL,
        );
    }
    m.push_ring_box(
        angle,
        mid_r,
        HEIGHT + 0.18,
        Vec3::new(WIDTH + 0.9, 0.36, length),
        ROOF,
    );
    // Dark throat, so the tunnel reads as going somewhere.
    m.push_ring_box(
        angle,
        inner_radius + length * 0.75,
        HEIGHT * 0.5,
        Vec3::new(WIDTH, HEIGHT, length * 0.5),
        DARK,
    );
    m.build()
}

/// Dugout fit-out: bench seats and kit bags for one enclosure.
pub(crate) fn dugout_fitout_mesh(center: Vec3, tones: &[[f32; 3]]) -> Mesh {
    const BENCH: [f32; 4] = [0.30, 0.32, 0.36, 1.0];
    const SEATS: usize = 6;

    let mut m = StandMesh::new();
    for i in 0..SEATS {
        let z = center.z + (i as f32 - (SEATS as f32 - 1.0) * 0.5) * 0.78;
        let pos = Vec3::new(center.x, center.y, z);
        m.push_box(
            Transform::from_translation(pos + Vec3::Y * 0.45),
            Vec3::new(0.34, 0.05, 0.30),
            BENCH,
        );
        m.push_box(
            Transform::from_translation(pos + Vec3::new(-0.34, 0.66, 0.0)),
            Vec3::new(0.05, 0.26, 0.30),
            BENCH,
        );
    }
    if tones.is_empty() {
        return m.build();
    }
    for i in 0..4 {
        let tone = tones[stand_hash(i, 9, 0xBA61) as usize % tones.len()];
        m.push_box(
            Transform::from_translation(Vec3::new(
                center.x - 0.9,
                center.y + 0.22,
                center.z + (i as f32 - 1.5) * 1.05,
            ))
            .with_rotation(Quat::from_rotation_y(stand_unit(i, 4, 0xBA62) * 0.6)),
            Vec3::new(0.26, 0.22, 0.52),
            [tone[0], tone[1], tone[2], 1.0],
        );
    }
    m.build()
}

#[cfg(test)]
mod tests {
    use bevy::camera::primitives::MeshAabb;

    use super::*;

    fn vertex_count(mesh: &Mesh) -> usize {
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
            .map(|p| p.len())
            .unwrap_or(0)
    }

    /// Largest horizontal reach of a mesh, i.e. the radius it occupies.
    fn radial_reach(mesh: &Mesh) -> f32 {
        let aabb = mesh.compute_aabb().expect("mesh must have bounds");
        aabb.half_extents.x.max(aabb.half_extents.z)
    }

    fn palette() -> SeatPalette {
        SeatPalette {
            tones: [[0.24, 0.30, 0.42], [0.28, 0.34, 0.46], [0.20, 0.26, 0.38]],
            accent: [0.86, 0.84, 0.78],
        }
    }

    fn band(row: usize, radius: f32, mosaic: bool) -> SeatBand {
        SeatBand {
            segments: 96,
            aisle_every: 8,
            radius,
            tread_top: 1.0 + row as f32 * 1.12,
            row,
            mosaic,
        }
    }

    fn roof() -> RoofSpec {
        RoofSpec {
            truss_count: 24,
            inner_radius: 88.0,
            outer_radius: 104.0,
            inner_y: 22.0,
            outer_y: 25.0,
            depth: 2.6,
            member: 0.22,
            camber: 0.8,
            web_panels: 6,
        }
    }

    fn facade() -> FacadeSpec {
        FacadeSpec {
            segments: 48,
            radius: 106.0,
            base_y: 9.0,
            height: 14.0,
            rib_width: 1.1,
            rib_depth: 0.9,
            glazing_bands: 6,
        }
    }

    #[test]
    fn stand_hash_is_deterministic_and_spread() {
        for a in 0..8u32 {
            assert_eq!(stand_hash(a, 3, 7), stand_hash(a, 3, 7));
            let u = stand_unit(a, 3, 7);
            assert!((0.0..1.0).contains(&u), "unit hash out of range: {u}");
        }
        // Neighbouring inputs must not collapse onto the same value.
        assert_ne!(stand_hash(1, 1, 1), stand_hash(2, 1, 1));
        assert_ne!(stand_hash(1, 1, 1), stand_hash(1, 2, 1));
    }

    #[test]
    fn aisle_segments_match_ring_band_skip_rule() {
        assert!(segment_is_aisle(0, 8));
        assert!(segment_is_aisle(16, 8));
        assert!(!segment_is_aisle(7, 8));
        assert!(!segment_is_aisle(1, 0), "aisle_every 0 means no aisles");
    }

    #[test]
    fn seat_pitch_stays_within_realistic_range() {
        // 96 segments at a 70 m radius: ~4.6 m of arc per segment.
        let n = seats_per_segment(70.0, 96);
        let actual = TAU * 70.0 / 96.0 / n as f32;
        assert!(
            (0.45..0.65).contains(&actual),
            "seat pitch {actual} outside stadium norms"
        );
    }

    #[test]
    fn seat_grid_follows_the_bowl_segmentation() {
        // These mirror `stadium::TIER_SEGMENTS` / `AISLE_EVERY`. If the bowl ever
        // resegments, the crowd has to move with it, which is the whole point of
        // both reading the same grid.
        assert_eq!(SEAT_SEGMENTS, 96);
        assert_eq!(SEAT_AISLE_EVERY, 8);
        let b = band(3, 76.0, false);
        let grid = SeatGrid::on_tread(b.radius, b.tread_top);
        assert_eq!(grid.segments, SEAT_SEGMENTS);
        assert_eq!(grid.aisle_every, SEAT_AISLE_EVERY);
        // A band and a grid on the same tread must be the same layout, or the
        // seat shells and the people in them come from different maths again.
        assert_eq!(grid, b.grid());
        assert_eq!(grid.total_seats(), seat_band_count(&b));
    }

    #[test]
    fn seat_grid_visits_every_seat_exactly_once() {
        let grid = SeatGrid::on_tread(80.0, 5.0);
        let per = grid.seats_per_segment();
        let seats: Vec<(usize, usize)> = grid.seats().collect();
        assert_eq!(seats.len(), grid.total_seats());
        for &(seg, k) in &seats {
            assert!(!grid.is_aisle(seg), "seat in aisle segment {seg}");
            assert!(k < per, "seat index {k} past the {per} that fit");
        }
        let mut sorted = seats.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), seats.len(), "a seat was visited twice");
    }

    #[test]
    fn adjacent_seats_sit_one_pitch_apart_across_segment_joins() {
        let grid = SeatGrid::on_tread(82.0, 6.0);
        let per = grid.seats_per_segment();
        let pitch = grid.seat_pitch();
        // Within a segment, and over the join into the next one: the row must be
        // evenly spaced, or the seams show as a double-width seat.
        for (a, b) in [((1, 0), (1, 1)), ((1, per - 1), (2, 0))] {
            let d = grid.seat_hip(a.0, a.1).distance(grid.seat_hip(b.0, b.1));
            assert!(
                (d - pitch).abs() < 0.02,
                "seats {a:?}->{b:?} are {d} apart, not one {pitch} pitch"
            );
        }
        assert!(
            (0.45..0.65).contains(&pitch),
            "pitch {pitch} outside stadium norms"
        );
    }

    #[test]
    fn seated_hip_lands_on_the_pan_against_the_backrest() {
        let grid = SeatGrid::on_tread(78.0, 7.5);
        // Pan top, not the tread floor: the whole bug this grid exists to fix.
        assert!((grid.hip_height() - (7.5 + 0.455)).abs() < 1e-5);
        assert!(grid.hip_height() > grid.tread_top + SEAT_PAN_HEIGHT);
        assert!(grid.hip_height() < grid.backrest_top() - 0.3);

        let hip = grid.hip_offset();
        let pan_front = SEAT_PAN_INWARD + SEAT_PAN_DEPTH * 0.5;
        let pan_back = SEAT_PAN_INWARD - SEAT_PAN_DEPTH * 0.5;
        assert!(
            hip.z > pan_back && hip.z < pan_front,
            "pelvis at {} is off the pan ({pan_back}..{pan_front})",
            hip.z
        );
        assert!(hip.z < SEAT_PAN_INWARD, "pelvis must sit back, not forward");

        // Feet share the seat's ground plan and stand on the tread itself.
        let foot = grid.seat_foot(4, 2);
        let seated = grid.seat_hip(4, 2);
        assert!((foot.y - grid.tread_top).abs() < 1e-5);
        assert!(Vec2::new(foot.x - seated.x, foot.z - seated.z).length() < 1e-4);
        // And the seat is a seat's worth of radius inside the tread mid line.
        let hip_r = Vec2::new(seated.x, seated.z).length();
        assert!(
            hip_r < grid.radius && hip_r > grid.radius - 0.3,
            "hip radius {hip_r} against tread mid {}",
            grid.radius
        );
    }

    #[test]
    fn seat_count_scales_with_radius_and_skips_aisles() {
        let inner = seat_band_count(&band(0, 70.0, false));
        let outer = seat_band_count(&band(11, 102.0, false));
        assert!(outer > inner, "outer rows are longer: {inner} vs {outer}");
        // 96 segments, 12 of which are aisles.
        assert_eq!(inner, 84 * seats_per_segment(70.0, 96));
        assert!(
            (600..1400).contains(&inner),
            "one row should hold hundreds of seats, got {inner}"
        );
    }

    #[test]
    fn seat_band_mesh_is_non_empty_and_bounded() {
        let b = band(3, 76.0, false);
        let mesh = seat_band_mesh(&b, &palette());
        assert!(!mesh_is_empty(&mesh));
        // Pan + backrest, five faces each, four vertices per face.
        assert_eq!(vertex_count(&mesh), seat_band_count(&b) * 40);
        assert!(mesh.attribute(Mesh::ATTRIBUTE_COLOR).is_some());

        let aabb = mesh.compute_aabb().unwrap();
        // The band wraps the full ring, so its bounds are the seat radius.
        assert!(
            (aabb.half_extents.x - b.radius).abs() < 1.5,
            "x reach {}",
            aabb.half_extents.x
        );
        assert!(
            (aabb.half_extents.z - b.radius).abs() < 1.5,
            "z reach {}",
            aabb.half_extents.z
        );
        let top = aabb.center.y + aabb.half_extents.y;
        assert!(
            top > b.tread_top + 0.5 && top < b.tread_top + 1.2,
            "seat crown {top} is not a seat height above tread {}",
            b.tread_top
        );
    }

    #[test]
    fn seat_band_mesh_is_deterministic() {
        let b = band(9, 90.0, true);
        let a = seat_band_mesh(&b, &palette());
        let c = seat_band_mesh(&b, &palette());
        for attr in [Mesh::ATTRIBUTE_POSITION, Mesh::ATTRIBUTE_COLOR] {
            assert_eq!(
                a.attribute(attr.id).unwrap().get_bytes(),
                c.attribute(attr.id).unwrap().get_bytes()
            );
        }
    }

    #[test]
    fn mosaic_produces_blocks_not_noise() {
        // Neighbouring segments inside one block share a value.
        for seg in 0..MOSAIC_BLOCK_SEGMENTS - 1 {
            assert_eq!(
                mosaic_block_is_accent(seg, 8),
                mosaic_block_is_accent(seg + 1, 8)
            );
        }
        let accents = (0..96).filter(|&s| mosaic_block_is_accent(s, 8)).count();
        assert!(accents > 0 && accents < 96, "mosaic is uniform: {accents}");
    }

    #[test]
    fn vomitories_sit_on_aisle_centres_clear_of_seats() {
        let aisles = aisle_angles(96, 8);
        assert_eq!(aisles.len(), 12);
        let voms = vomitory_angles(96, 8, 2);
        assert_eq!(voms.len(), 6);
        for v in &voms {
            assert!(
                aisles.iter().any(|a| (a - v).abs() < 1e-5),
                "vomitory at {v} is not on an aisle centre"
            );
            // It must fall inside a skipped segment, never on a seat row.
            let seg = (v / TAU * 96.0).floor() as usize;
            assert!(
                segment_is_aisle(seg, 8),
                "vomitory at {v} lands on seated segment {seg}"
            );
        }
    }

    #[test]
    fn vomitory_meshes_are_non_empty_and_bore_outward() {
        let voms: Vec<Vomitory> = vomitory_angles(96, 8, 2)
            .into_iter()
            .map(|angle| Vomitory {
                angle,
                mouth_radius: 78.0,
                depth: 5.0,
                width: 4.0,
                height: 3.0,
                floor_y: 4.5,
            })
            .collect();
        let interior = vomitory_interior_mesh(&voms);
        let frame = vomitory_frame_mesh(&voms);
        assert!(!mesh_is_empty(&interior));
        assert!(!mesh_is_empty(&frame));
        let reach = radial_reach(&interior);
        assert!(
            reach > 78.0 && reach < 78.0 + 6.0,
            "tunnel bore should run outward from the mouth, got {reach}"
        );
    }

    #[test]
    fn stair_flights_fill_the_aisle_solidly() {
        let flights: Vec<StairFlight> = aisle_angles(96, 8)
            .into_iter()
            .map(|angle| StairFlight {
                angle,
                inner_radius: 70.0,
                run: 2.25,
                rise: 1.12,
                base_y: 1.05,
                foot_y: 0.5,
                width: 3.0,
                steps: 3,
            })
            .collect();
        assert_eq!(stair_step_count(&flights), 36);
        let mesh = stair_flights_mesh(&flights);
        // Six faces per step box, four vertices per face.
        assert_eq!(vertex_count(&mesh), 36 * 24);

        let aabb = mesh.compute_aabb().unwrap();
        let low = aabb.center.y - aabb.half_extents.y;
        let high = aabb.center.y + aabb.half_extents.y;
        assert!(
            (low - 0.5).abs() < 1e-3,
            "steps must reach the flight's foot, got {low}"
        );
        assert!(
            (high - (1.05 + 1.12)).abs() < 1e-3,
            "top nosing must land on the next tread, got {high}"
        );
        // Radial run stays inside one tier's tread depth.
        let reach = radial_reach(&mesh);
        assert!(
            reach > 70.0 && reach < 70.0 + 2.35,
            "flight run {reach} escaped the tread"
        );
    }

    #[test]
    fn roof_truss_angles_are_evenly_spaced_and_unique() {
        let angles = roof_truss_angles(24);
        assert_eq!(angles.len(), 24);
        let step = TAU / 24.0;
        for (i, a) in angles.iter().enumerate() {
            assert!((a - i as f32 * step).abs() < 1e-5);
            assert!((0.0..TAU).contains(a));
        }
    }

    #[test]
    fn roof_truss_mesh_spans_cantilever_and_tapers_to_the_tip() {
        let spec = roof();
        let mesh = roof_truss_mesh(&spec);
        assert!(!mesh_is_empty(&mesh));
        let reach = radial_reach(&mesh);
        assert!(
            reach > spec.outer_radius - 1.0 && reach < spec.outer_radius + 2.0,
            "truss reach {reach} should stop at the rear support {}",
            spec.outer_radius
        );
        // Cantilever profile: full structural depth at the rear, none at the tip.
        let tip = spec.top_chord(0.0, 0.0) - spec.bottom_chord(0.0, 0.0);
        let rear = spec.top_chord(0.0, 1.0) - spec.bottom_chord(0.0, 1.0);
        assert!(tip.y.abs() < 1e-4);
        assert!((rear.y - spec.depth).abs() < 1e-4);
    }

    #[test]
    fn roof_panels_split_into_opaque_and_translucent_sets() {
        let spec = roof();
        let opaque = vertex_count(&roof_panel_mesh(&spec, false));
        let glazed = vertex_count(&roof_panel_mesh(&spec, true));
        assert!(
            opaque > 0 && glazed > 0,
            "both sets exist: {opaque}/{glazed}"
        );
        assert!(
            opaque > glazed,
            "most of the roof stays opaque: {opaque} vs {glazed}"
        );
        let translucent = (0..spec.panel_count())
            .filter(|&b| roof_panel_is_translucent(b))
            .count();
        assert_eq!(translucent, spec.panel_count() / 4);
    }

    #[test]
    fn roof_parts_stay_inside_the_roof_footprint() {
        let spec = roof();
        for mesh in [
            roof_edge_mesh(&spec),
            roof_soffit_mesh(&spec),
            roof_cluster_mesh(&spec, 12),
            roof_panel_mesh(&spec, false),
        ] {
            let reach = radial_reach(&mesh);
            assert!(
                reach <= spec.outer_radius + 1.0,
                "roof part reaches {reach}, past the rear support"
            );
            let aabb = mesh.compute_aabb().unwrap();
            let low = aabb.center.y - aabb.half_extents.y;
            assert!(
                low > spec.inner_y - 6.0,
                "roof part hangs to {low}, below the bowl roofline"
            );
        }
    }

    #[test]
    fn roof_flags_rise_above_the_deck() {
        let spec = roof();
        let mesh = roof_flag_mesh(&spec, 16, &[[0.8, 0.2, 0.2], [0.2, 0.3, 0.8]]);
        let aabb = mesh.compute_aabb().unwrap();
        let top = aabb.center.y + aabb.half_extents.y;
        assert!(top > spec.outer_y, "flags should clear the roof: {top}");
        assert!(top < spec.outer_y + 8.0, "flags absurdly tall: {top}");
        assert!(mesh_is_empty(&roof_flag_mesh(&spec, 16, &[])));
    }

    #[test]
    fn facade_parts_are_non_empty_and_stacked_correctly() {
        let spec = facade();
        let ribs = facade_rib_mesh(&spec);
        let glazing = facade_glazing_mesh(&spec);
        let parapet = facade_parapet_mesh(&spec);
        for mesh in [&ribs, &glazing, &parapet] {
            assert!(!mesh_is_empty(mesh));
        }
        let rib_aabb = ribs.compute_aabb().unwrap();
        let parapet_aabb = parapet.compute_aabb().unwrap();
        assert!(
            parapet_aabb.center.y - parapet_aabb.half_extents.y
                > rib_aabb.center.y + rib_aabb.half_extents.y - 1.5,
            "parapet must crown the ribs"
        );
        // Glazing sits behind the rib faces, not in front of them.
        assert!(radial_reach(&glazing) < radial_reach(&ribs));
    }

    #[test]
    fn gates_are_evenly_spaced_and_sit_at_ground_level() {
        let angles = gate_angles(8);
        assert_eq!(angles.len(), 8);
        for w in angles.windows(2) {
            assert!((w[1] - w[0] - TAU / 8.0).abs() < 1e-5);
        }
        let mesh = gate_portal_mesh(&angles, 106.0, 5.0, 4.2);
        let aabb = mesh.compute_aabb().unwrap();
        let low = aabb.center.y - aabb.half_extents.y;
        assert!(low.abs() < 0.2, "gates must sit at ground level, got {low}");
    }

    #[test]
    fn hoarding_ring_has_uvs_for_sponsor_art() {
        let mesh = hoarding_ring_mesh(96, 66.0, 1.4, 1.2, 2);
        assert!(!mesh.attribute(Mesh::ATTRIBUTE_UV_0).unwrap().is_empty());
        // One quad per board, 48 boards.
        assert_eq!(vertex_count(&mesh), 48 * 4);
        assert!(!mesh_is_empty(&hoarding_backing_mesh(
            96, 66.0, 1.4, 1.2, 2
        )));
    }

    #[test]
    fn detail_meshes_are_non_empty() {
        assert!(!mesh_is_empty(&camera_gantry_mesh(&[0.0, 1.5], 80.0, 12.0)));
        assert!(!mesh_is_empty(&screen_support_mesh(
            Vec3::new(-60.0, 6.0, 0.0),
            8.0,
            0.0
        )));
        assert!(!mesh_is_empty(&player_tunnel_mesh(
            std::f32::consts::PI,
            62.0,
            9.0
        )));
        assert!(!mesh_is_empty(&dugout_fitout_mesh(
            Vec3::new(-50.0, 0.0, 55.0),
            &[[0.8, 0.2, 0.2]]
        )));
        assert!(!mesh_is_empty(&concourse_reveal_mesh(
            48, 100.0, 9.0, 3.4, 2.2
        )));
    }

    #[test]
    fn open_bottom_box_drops_exactly_one_face() {
        let mut full = StandMesh::new();
        full.push_box(Transform::IDENTITY, Vec3::ONE, [1.0; 4]);
        let mut open = StandMesh::new();
        open.push_box_open_bottom(Transform::IDENTITY, Vec3::ONE, [1.0; 4]);
        let full = full.build();
        let open = open.build();
        assert_eq!(vertex_count(&full) - vertex_count(&open), 4);
        // Nothing is left facing straight down.
        let normals = open.attribute(Mesh::ATTRIBUTE_NORMAL).unwrap().get_bytes();
        let downward = normals
            .chunks_exact(12)
            .filter(|n| f32::from_le_bytes([n[4], n[5], n[6], n[7]]) < -0.99)
            .count();
        assert_eq!(downward, 0);
    }

    #[test]
    fn struts_span_their_endpoints_and_reject_degenerate_spans() {
        let mut m = StandMesh::new();
        m.push_strut(Vec3::ZERO, Vec3::new(3.0, 4.0, 0.0), 0.1, [1.0; 4]);
        let mesh = m.build();
        let aabb = mesh.compute_aabb().unwrap();
        assert!(aabb.center.x > 1.3 && aabb.center.y > 1.8);
        assert!(aabb.half_extents.x > 1.3 && aabb.half_extents.y > 1.8);

        let mut degenerate = StandMesh::new();
        degenerate.push_strut(Vec3::ONE, Vec3::ONE, 0.1, [1.0; 4]);
        assert!(mesh_is_empty(&degenerate.build()));
    }
}

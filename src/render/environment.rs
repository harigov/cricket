//! Surroundings rendered outside the stadium bowl.
//!
//! Every ground sits in a themed world (see [`StadiumEnvironment`]): a downtown
//! skyline, an alpine valley, an island coast, and so on. [`spawn_environment`]
//! populates the annulus between the outer edge of the bowl and the ground disc
//! that fades into the sky.
//!
//! The camera lives inside the bowl and looks out over the stands, so only two
//! things matter: the silhouette above the roofline and the middle distance seen
//! through the gaps. Everything here is therefore built for the long view —
//! landforms are single merged, vertex-coloured meshes and kit props are placed
//! in a few hundred instances of a handful of shared scene handles, never one
//! entity per detail.
//!
//! Placement is driven entirely by [`sky_hash`], so a ground looks the same in
//! every match.
//!
//! Every colour in here is written as the value it should have **on screen**
//! and turned into an albedo by [`day_albedo`]. The camera is exposed a long
//! way off the light it is given, so the two are nowhere near each other: see
//! [`DAY_FLAT_RESPONSE`].

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::asset::RenderAssetUsages;
use bevy::gltf::GltfAssetLabel;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::core::stadiums::StadiumEnvironment;
use crate::render::ring_geometry::{
    GroundPalette, ring_position, ring_segment_transform, stadium_ground_disc_mesh_tinted,
    stadium_ground_radius,
};
use crate::render::sky::{sky_hash, sky_horizon_color};
use crate::render::stadium::{StadiumBuildCtx, track_spawn};

/// Marker for every entity belonging to the themed surroundings.
#[derive(Component)]
pub struct EnvironmentProp;

/// Clear ring kept between the bowl and the nearest prop. Floodlight towers sit
/// at `bowl + 9.5`, so this leaves the whole service apron and the camera's
/// downward sightlines free.
const PROP_INNER_MARGIN: f32 = 45.0;
/// Props stop short of the apron rim so nothing straddles the horizon fade.
const PROP_OUTER_MARGIN: f32 = 18.0;
/// Themed dome radius — just inside the shared 600 m sky sphere it hides by day.
/// Matches the shared sky sphere's offset so the two horizons coincide.
const SKY_DOME_DROP: f32 = -6.0;
/// Segment count of the apron disc, mirroring `build_stadium`.
const GROUND_DISC_SEGMENTS: usize = 96;

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

/// The annulus available to the surroundings, from the clear ring around the
/// bowl out to the rim of the ground disc.
pub(crate) struct EnvLayout {
    pub(crate) inner: f32,
    pub(crate) outer: f32,
}

impl EnvLayout {
    pub(crate) fn new(bowl_outer_radius: f32) -> Self {
        Self {
            inner: bowl_outer_radius + PROP_INNER_MARGIN,
            outer: stadium_ground_radius(bowl_outer_radius) - PROP_OUTER_MARGIN,
        }
    }

    /// Radius `t` of the way across the annulus (0 = clear ring, 1 = rim).
    pub(crate) fn at(&self, t: f32) -> f32 {
        self.inner + (self.outer - self.inner) * t
    }

    /// Edge of the ground disc itself. Only flat surfaces may run out this far;
    /// anything with height needs the margin [`at(1.0)`](Self::at) leaves.
    pub(crate) fn rim(&self) -> f32 {
        self.outer + PROP_OUTER_MARGIN
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn lerp_rgb(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        lerp(a[0], b[0], t),
        lerp(a[1], b[1], t),
        lerp(a[2], b[2], t),
    ]
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Distance from the pitch centre, ignoring height.
fn planar_radius(pos: Vec3) -> f32 {
    (pos.x * pos.x + pos.z * pos.z).sqrt()
}

// ---------------------------------------------------------------------------
// Exposure
// ---------------------------------------------------------------------------

/// Radiance one unit of albedo returns from flat, unshadowed ground under the
/// day lighting preset, per channel.
///
/// `lighting_preset` in `main.rs` runs the day at EV100 10.2 — about three
/// stops hotter than the light it sets up — so albedo and screen value are a
/// long way apart out here. Working the preset through Bevy's Lambertian term
/// (`albedo / π · illuminance · N·L`, summed over the 54 klx key at 56°, the
/// 5.8 klx skylight and 520 lx of ambient) and then through the exposure
/// (`exp2(-10.2) / 1.2`) still leaves ten times the albedo, so a landform
/// painted with a raw 0.8 lands eight stops into the tonemapper's shoulder and
/// comes out white. That is what turned the beach into a snowfield.
///
/// The key light is warm, so blue returns the least of the three.
const DAY_FLAT_RESPONSE: [f32; 3] = [10.38, 9.19, 7.29];

/// Albedo that returns the same radiance as an *unlit* surface painted `srgb`.
///
/// The sky dome is unlit and the distance fog is screen-referred, so this is
/// the only way for the ground, the sea and the far haze to agree with the air
/// they dissolve into: a disc rim painted with `day_albedo(horizon)` and the
/// sky texel it came from reach the tonemapper as the same number and so land
/// on the same pixel.
///
/// Everything below the shoulder comes back within a hair of the value written
/// (0.35 renders as 0.35); the brightest values are pulled back a little (0.80
/// renders as ≈0.69), so where a surface really is brighter than the sky the
/// palette is authored past white on purpose — see [`ALPINE_RAMP`].
pub(crate) fn day_albedo(srgb: [f32; 3]) -> [f32; 3] {
    [
        srgb_to_linear(srgb[0]) / DAY_FLAT_RESPONSE[0],
        srgb_to_linear(srgb[1]) / DAY_FLAT_RESPONSE[1],
        srgb_to_linear(srgb[2]) / DAY_FLAT_RESPONSE[2],
    ]
}

/// sRGB transfer function, matching `Color::srgb`. Vertex colours reach the
/// shader as linear, so the decode has to happen on this side. Extends past
/// 1.0 smoothly, which is what lets a snowfield be authored above white.
fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

// ---------------------------------------------------------------------------
// Deterministic kit placement
// ---------------------------------------------------------------------------

/// One kit instance: which model, where it stands, how it is turned and sized.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PropPlacement {
    pub(crate) variant: usize,
    pub(crate) pos: Vec3,
    pub(crate) yaw: f32,
    pub(crate) scale: Vec3,
}

impl PropPlacement {
    pub(crate) fn radius(&self) -> f32 {
        planar_radius(self.pos)
    }

    /// Conservative horizontal half-extent: the widest kit model is a little
    /// over two units across, so one unit of scale is a safe radius.
    pub(crate) fn half_extent(&self) -> f32 {
        self.scale.x * 1.2
    }
}

/// A ring of kit instances scattered over slots around the ground.
pub(crate) struct ScatterSpec {
    /// Angular slots considered; a slot may end up empty.
    pub(crate) slots: usize,
    pub(crate) inner: f32,
    pub(crate) outer: f32,
    pub(crate) variants: usize,
    pub(crate) seed: u32,
    /// Chance a fully weighted slot is filled.
    pub(crate) density: f32,
    /// Uniform scale range applied to the kit model.
    pub(crate) scale: (f32, f32),
    /// Extra Y-only stretch on top of the uniform scale, for skylines.
    pub(crate) stretch: (f32, f32),
}

impl ScatterSpec {
    /// Ring of evenly weighted props at a single uniform scale range.
    fn even(slots: usize, inner: f32, outer: f32, variants: usize, seed: u32) -> Self {
        Self {
            slots,
            inner,
            outer,
            variants,
            seed,
            density: 0.8,
            scale: (1.0, 1.0),
            stretch: (1.0, 1.0),
        }
    }

    fn density(mut self, density: f32) -> Self {
        self.density = density;
        self
    }

    fn scale(mut self, min: f32, max: f32) -> Self {
        self.scale = (min, max);
        self
    }

    fn stretch(mut self, min: f32, max: f32) -> Self {
        self.stretch = (min, max);
        self
    }
}

/// Scatter one ring. `weight` maps an angle to `0..1` and drives both how
/// likely a slot is to be filled and how large its prop grows, which is what
/// gives the city a dense core and the coast a seaward bias.
pub(crate) fn scatter_ring(spec: &ScatterSpec, weight: impl Fn(f32) -> f32) -> Vec<PropPlacement> {
    let slot_arc = TAU / spec.slots as f32;
    let mut out = Vec::with_capacity(spec.slots);
    for slot in 0..spec.slots {
        let s = slot as u32;
        let angle = slot as f32 * slot_arc + (sky_hash(s, 1, spec.seed) - 0.5) * slot_arc * 0.85;
        let w = weight(angle).clamp(0.0, 1.0);
        if sky_hash(s, 2, spec.seed) > spec.density * (0.25 + 0.75 * w) {
            continue;
        }
        // Weight biases size as well as presence: the thin edges of a group are
        // its small stuff, which keeps silhouettes from ending in a hard wall.
        let bias = 0.15 + 0.85 * w;
        let size = lerp(spec.scale.0, spec.scale.1, sky_hash(s, 4, spec.seed) * bias);
        let stretch = lerp(
            spec.stretch.0,
            spec.stretch.1,
            sky_hash(s, 5, spec.seed) * bias,
        );
        let radius = lerp(spec.inner, spec.outer, sky_hash(s, 3, spec.seed));
        out.push(PropPlacement {
            variant: (sky_hash(s, 6, spec.seed) * spec.variants as f32) as usize % spec.variants,
            pos: ring_position(angle, radius, 0.0),
            yaw: sky_hash(s, 7, spec.seed) * TAU,
            scale: Vec3::new(size, size * stretch, size),
        });
    }
    out
}

/// Mature trees grow in clumps, not in rings — this scatters clump centres and
/// then members around each one, clamped back inside the annulus.
pub(crate) struct ClumpSpec {
    pub(crate) clumps: usize,
    pub(crate) per_clump: usize,
    pub(crate) inner: f32,
    pub(crate) outer: f32,
    /// Radius of one clump on the ground.
    pub(crate) spread: f32,
    pub(crate) variants: usize,
    pub(crate) seed: u32,
    pub(crate) density: f32,
    pub(crate) scale: (f32, f32),
}

pub(crate) fn scatter_clumps(spec: &ClumpSpec) -> Vec<PropPlacement> {
    let mut out = Vec::with_capacity(spec.clumps * spec.per_clump);
    for clump in 0..spec.clumps {
        let c = clump as u32;
        let angle = (clump as f32 + sky_hash(c, 1, spec.seed)) / spec.clumps as f32 * TAU;
        let centre_r = lerp(spec.inner, spec.outer, sky_hash(c, 2, spec.seed));
        let centre = ring_position(angle, centre_r, 0.0);
        for member in 0..spec.per_clump {
            let m = (clump * 64 + member) as u32;
            if sky_hash(m, 3, spec.seed) > spec.density {
                continue;
            }
            let local_a = sky_hash(m, 4, spec.seed) * TAU;
            // Square-rooted radius keeps members evenly spread over the disc
            // instead of piling up in the middle of the clump.
            let local_r = spec.spread * sky_hash(m, 5, spec.seed).sqrt();
            let pos = centre + Vec3::new(local_a.cos() * local_r, 0.0, local_a.sin() * local_r);
            let size = lerp(spec.scale.0, spec.scale.1, sky_hash(m, 6, spec.seed));
            out.push(PropPlacement {
                variant: (sky_hash(m, 7, spec.seed) * spec.variants as f32) as usize
                    % spec.variants,
                pos: clamp_radius(pos, spec.inner, spec.outer),
                yaw: sky_hash(m, 8, spec.seed) * TAU,
                scale: Vec3::splat(size),
            });
        }
    }
    out
}

/// Pull a ground position back into the annulus without moving its bearing.
fn clamp_radius(pos: Vec3, inner: f32, outer: f32) -> Vec3 {
    let r = planar_radius(pos);
    if r >= inner && r <= outer {
        return pos;
    }
    let clamped = r.clamp(inner, outer);
    let scale = if r > 1e-3 { clamped / r } else { 0.0 };
    Vec3::new(pos.x * scale, pos.y, pos.z * scale)
}

// ---------------------------------------------------------------------------
// Vertex-coloured mesh assembly
// ---------------------------------------------------------------------------

/// Accumulates flat-shaded, vertex-coloured triangles.
///
/// Landforms are the bulk of the geometry out here, and Bevy exposes no
/// instancing, so each one is merged into a single mesh and drawn once.
///
/// Colours go in screen-referred and come out as albedo: doing the [`day_albedo`]
/// conversion here, at the one place vertices are written, is what keeps every
/// palette in this module readable and stops a colour being converted twice.
#[derive(Default)]
pub(crate) struct ColorMesh {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl ColorMesh {
    /// Quad wound `a → b → c → d`; the face normal follows the right-hand rule
    /// over `(b - a) × (d - a)`.
    fn quad(&mut self, corners: [Vec3; 4], colors: [[f32; 3]; 4]) {
        let normal = (corners[1] - corners[0])
            .cross(corners[3] - corners[0])
            .try_normalize()
            .unwrap_or(Vec3::Y)
            .to_array();
        const UVS: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let base = self.positions.len() as u32;
        for i in 0..4 {
            self.positions.push(corners[i].to_array());
            self.normals.push(normal);
            self.uvs.push(UVS[i]);
            let c = day_albedo(colors[i]);
            self.colors.push([c[0], c[1], c[2], 1.0]);
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    fn flat_quad(&mut self, corners: [Vec3; 4], rgb: [f32; 3]) {
        self.quad(corners, [rgb; 4]);
    }

    fn tri(&mut self, corners: [Vec3; 3], rgb: [f32; 3]) {
        let normal = (corners[1] - corners[0])
            .cross(corners[2] - corners[0])
            .try_normalize()
            .unwrap_or(Vec3::Y)
            .to_array();
        let base = self.positions.len() as u32;
        let albedo = day_albedo(rgb);
        for (i, corner) in corners.iter().enumerate() {
            self.positions.push(corner.to_array());
            self.normals.push(normal);
            self.uvs.push([i as f32 * 0.5, (i / 2) as f32]);
            self.colors.push([albedo[0], albedo[1], albedo[2], 1.0]);
        }
        self.indices.extend_from_slice(&[base, base + 1, base + 2]);
    }

    /// Axis-aligned box, yawed about its own centre.
    fn box_at(&mut self, centre: Vec3, half: Vec3, yaw: f32, rgb: [f32; 3]) {
        let rot = Quat::from_rotation_y(yaw);
        let corner = |sx: f32, sy: f32, sz: f32| {
            centre + rot * Vec3::new(half.x * sx, half.y * sy, half.z * sz)
        };
        // Slightly darker sides than the top reads as ambient occlusion at
        // distance without needing a second material.
        let top = rgb;
        let side = lerp_rgb(rgb, [0.0, 0.0, 0.0], 0.18);
        self.flat_quad(
            [
                corner(-1.0, 1.0, 1.0),
                corner(1.0, 1.0, 1.0),
                corner(1.0, 1.0, -1.0),
                corner(-1.0, 1.0, -1.0),
            ],
            top,
        );
        self.flat_quad(
            [
                corner(-1.0, -1.0, 1.0),
                corner(1.0, -1.0, 1.0),
                corner(1.0, 1.0, 1.0),
                corner(-1.0, 1.0, 1.0),
            ],
            side,
        );
        self.flat_quad(
            [
                corner(1.0, -1.0, -1.0),
                corner(-1.0, -1.0, -1.0),
                corner(-1.0, 1.0, -1.0),
                corner(1.0, 1.0, -1.0),
            ],
            side,
        );
        self.flat_quad(
            [
                corner(1.0, -1.0, 1.0),
                corner(1.0, -1.0, -1.0),
                corner(1.0, 1.0, -1.0),
                corner(1.0, 1.0, 1.0),
            ],
            side,
        );
        self.flat_quad(
            [
                corner(-1.0, -1.0, -1.0),
                corner(-1.0, -1.0, 1.0),
                corner(-1.0, 1.0, 1.0),
                corner(-1.0, 1.0, -1.0),
            ],
            side,
        );
    }

    /// Four-sided pyramid used for spires and roofs.
    fn pyramid(&mut self, base_centre: Vec3, half: f32, height: f32, yaw: f32, rgb: [f32; 3]) {
        let rot = Quat::from_rotation_y(yaw);
        let corner = |sx: f32, sz: f32| base_centre + rot * Vec3::new(half * sx, 0.0, half * sz);
        let apex = base_centre + Vec3::Y * height;
        let base = [
            corner(-1.0, 1.0),
            corner(1.0, 1.0),
            corner(1.0, -1.0),
            corner(-1.0, -1.0),
        ];
        for i in 0..4 {
            self.tri([base[i], base[(i + 1) % 4], apex], rgb);
        }
    }

    fn build(self) -> Mesh {
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

/// Height-keyed colour ramp; stops must be ordered by height fraction.
pub(crate) struct ColorRamp(&'static [(f32, [f32; 3])]);

impl ColorRamp {
    fn sample(&self, t: f32) -> [f32; 3] {
        let t = t.clamp(0.0, 1.0);
        let stops = self.0;
        let mut prev = stops[0];
        for &stop in stops {
            if t <= stop.0 {
                if stop.0 <= prev.0 {
                    return stop.1;
                }
                return lerp_rgb(prev.1, stop.1, (t - prev.0) / (stop.0 - prev.0));
            }
            prev = stop;
        }
        prev.1
    }
}

/// Dry beach sand. Shared by the coastal ground and the islands off it so one
/// material reads the same at both distances.
///
/// Warm but only a third saturated: a beach is a pale, slightly bleached tan,
/// and taking more blue out of it than this — which the earlier passes did to
/// fight the sky's sheen on the apron — turns the whole coast khaki. The sheen
/// belongs to the material, and `retint_ground` takes it off there instead.
const BEACH_SAND: [f32; 3] = [0.83, 0.75, 0.53];

/// Grass, bare rock, then snow above the snow line.
///
/// The snow stops are authored past white: a sunlit snowfield is one of the
/// few things in the scene genuinely brighter than the sky behind it, and
/// clamped to 1.0 the summits come back darker than the horizon they are
/// supposed to stand against.
const ALPINE_RAMP: ColorRamp = ColorRamp(&[
    (0.00, [0.20, 0.28, 0.18]),
    (0.20, [0.27, 0.30, 0.23]),
    (0.44, [0.34, 0.32, 0.30]),
    (0.60, [0.46, 0.45, 0.44]),
    (0.70, [1.02, 1.05, 1.12]),
    (1.00, [1.26, 1.28, 1.34]),
]);

/// Sedimentary banding: ochre floor, red beds, pale caprock. Kept a shade
/// under the desert plain so the mesas read as a landform standing on it
/// rather than more of the same dust.
const MESA_RAMP: ColorRamp = ColorRamp(&[
    (0.00, [0.62, 0.47, 0.30]),
    (0.28, [0.56, 0.32, 0.20]),
    (0.52, [0.67, 0.43, 0.25]),
    (0.78, [0.52, 0.28, 0.18]),
    (1.00, [0.76, 0.61, 0.41]),
]);

/// Distant woodland, dark and near-flat.
const TREELINE_RAMP: ColorRamp = ColorRamp(&[
    (0.00, [0.21, 0.32, 0.17]),
    (0.55, [0.16, 0.28, 0.14]),
    (1.00, [0.25, 0.38, 0.21]),
]);

/// Beach, scrub, forest, then bare summit rock.
const ISLAND_RAMP: ColorRamp = ColorRamp(&[
    (0.00, BEACH_SAND),
    (0.16, [0.44, 0.52, 0.28]),
    (0.55, [0.25, 0.39, 0.20]),
    (0.86, [0.37, 0.35, 0.32]),
    (1.00, [0.62, 0.62, 0.60]),
]);

// ---------------------------------------------------------------------------
// Landforms
// ---------------------------------------------------------------------------

/// Value noise around a ring, wrapping seamlessly at the join.
fn ring_noise(seg: usize, segments: usize, periods: usize, seed: u32) -> f32 {
    let t = seg as f32 / segments as f32 * periods as f32;
    let i0 = t.floor() as usize;
    let f = smoothstep(t - t.floor());
    let a = sky_hash((i0 % periods) as u32, 0, seed);
    let b = sky_hash(((i0 + 1) % periods) as u32, 0, seed);
    lerp(a, b, f)
}

/// Layered ring noise in `0..1`, from broad massifs down to individual summits.
fn ridge_profile(seg: usize, segments: usize, seed: u32) -> f32 {
    0.55 * ring_noise(seg, segments, 5, seed)
        + 0.30 * ring_noise(seg, segments, 13, seed.wrapping_add(41))
        + 0.15 * ring_noise(seg, segments, 31, seed.wrapping_add(97))
}

/// A ring of hills. Alpine ridgelines and desert mesas are the same heightfield
/// with a different crest profile and colour ramp.
pub(crate) struct RidgeSpec {
    pub(crate) segments: usize,
    pub(crate) crest_radius: f32,
    pub(crate) half_width: f32,
    pub(crate) min_height: f32,
    pub(crate) max_height: f32,
    /// Fraction of the half-width held flat at the crest: 0.05 is a knife-edge
    /// ridge, 0.7 a mesa. Never zero, which would collapse the crest quads.
    pub(crate) plateau: f32,
    /// Quantises heights into flat tops when above one; 0 leaves them smooth.
    pub(crate) terraces: usize,
    /// Altitude the colour ramp is measured against. Every ridge in a range
    /// shares one, so the snow line (or the rock banding) is a height in the
    /// world rather than a fraction of each individual ridge.
    pub(crate) ramp_height: f32,
    pub(crate) seed: u32,
    pub(crate) ramp: ColorRamp,
}

impl RidgeSpec {
    /// Outermost ground the ridge touches — must stay inside the sky dome.
    pub(crate) fn outer_foot(&self) -> f32 {
        self.crest_radius + self.half_width
    }

    fn height(&self, seg: usize) -> f32 {
        let mut profile = ridge_profile(seg, self.segments, self.seed);
        if self.terraces > 1 {
            let steps = self.terraces as f32;
            profile = (profile * steps).floor().min(steps - 1.0) / (steps - 1.0);
        }
        lerp(self.min_height, self.max_height, profile)
    }
}

/// Build one ridge ring as a single flat-shaded mesh.
pub(crate) fn ridge_mesh(spec: &RidgeSpec) -> Mesh {
    let mut mesh = ColorMesh::default();
    let top_half = spec.half_width * spec.plateau.max(0.02);
    let colour = |y: f32| spec.ramp.sample(y / spec.ramp_height.max(1e-3));

    // Four ground-plane offsets from the crest: outer foot, top edges, inner
    // foot. Walking them in order makes every band wind the same way, so the
    // inner slopes face the stadium and the outer slopes face the horizon.
    let bands = [
        (-spec.half_width, 0.0_f32),
        (-top_half, 1.0),
        (top_half, 1.0),
        (spec.half_width, 0.0),
    ];

    for seg in 0..spec.segments {
        let next = (seg + 1) % spec.segments;
        let a0 = seg as f32 / spec.segments as f32 * TAU;
        let a1 = (seg + 1) as f32 / spec.segments as f32 * TAU;
        // Wander the crest line so the ring never reads as a drawn circle.
        let crest = |s: usize| {
            spec.crest_radius
                + (ring_noise(s, spec.segments, 7, spec.seed.wrapping_add(11)) - 0.5)
                    * spec.half_width
                    * 0.55
        };
        let (c0, c1) = (crest(seg), crest(next));
        let (h0, h1) = (spec.height(seg), spec.height(next));
        let point = |angle: f32, crest_r: f32, offset: f32, height: f32| {
            ring_position(angle, crest_r + offset, height)
        };

        for band in 0..bands.len() - 1 {
            let (near_off, near_h) = bands[band];
            let (far_off, far_h) = bands[band + 1];
            let a = point(a0, c0, near_off, h0 * near_h);
            let b = point(a1, c1, near_off, h1 * near_h);
            let c = point(a1, c1, far_off, h1 * far_h);
            let d = point(a0, c0, far_off, h0 * far_h);
            mesh.quad(
                [a, b, c, d],
                [colour(a.y), colour(b.y), colour(c.y), colour(d.y)],
            );
        }
    }
    mesh.build()
}

/// Radial heightfield blob for the distant islands off the coastal ground.
pub(crate) fn island_mesh(radius: f32, height: f32, seed: u32) -> Mesh {
    const SEGMENTS: usize = 24;
    const RINGS: usize = 6;
    let mut mesh = ColorMesh::default();
    let point = |seg: usize, ring: usize| {
        let angle = seg as f32 / SEGMENTS as f32 * TAU;
        // Irregular coastline, then a dome that peaks off-centre.
        let edge = radius * (0.72 + 0.28 * ring_noise(seg, SEGMENTS, 6, seed));
        let f = ring as f32 / RINGS as f32;
        let r = edge * f;
        let dome = (1.0 - f * f).max(0.0).powf(0.85);
        let y = height * dome * (0.7 + 0.3 * ring_noise(seg, SEGMENTS, 11, seed.wrapping_add(7)));
        Vec3::new(angle.cos() * r, y, angle.sin() * r)
    };
    let colour = |p: Vec3| ISLAND_RAMP.sample(p.y / height.max(1e-3));

    // Walked from the summit outward so every quad winds the same way and the
    // slopes face away from the island's centre.
    for seg in 0..SEGMENTS {
        for ring in 0..RINGS {
            let next = (seg + 1) % SEGMENTS;
            let a = point(seg, ring);
            let b = point(next, ring);
            let c = point(next, ring + 1);
            let d = point(seg, ring + 1);
            mesh.quad([a, b, c, d], [colour(a), colour(b), colour(c), colour(d)]);
        }
    }
    mesh.build()
}

/// Sector of open water reaching from the beach to the horizon.
pub(crate) struct OceanSpec {
    /// Bearing the sea faces.
    pub(crate) centre_angle: f32,
    /// Angular width of the bay; the water pinches out at both ends.
    pub(crate) sweep: f32,
    pub(crate) shore_radius: f32,
    pub(crate) outer_radius: f32,
    pub(crate) segments: usize,
    pub(crate) rings: usize,
    pub(crate) shallow: [f32; 3],
    pub(crate) deep: [f32; 3],
    /// The last of the water washes into this, matching the ground disc's own
    /// fade so the sea reads as running all the way to the horizon. Kept light:
    /// the camera's own distance fog already does most of the recession, and
    /// washing the whole outer half of the bay into the sky is what stopped the
    /// water reading as water.
    pub(crate) horizon: [f32; 3],
    pub(crate) seed: u32,
}

impl OceanSpec {
    /// Where the waterline sits at `f` of the way across the sweep.
    ///
    /// The two ends taper out to the horizon radius so the sector closes into a
    /// bay instead of ending in a straight edge across the sand.
    fn shore_at(&self, f: f32) -> f32 {
        let taper = smoothstep(f / 0.22) * smoothstep((1.0 - f) / 0.22);
        let seg = (f * self.segments as f32) as usize;
        let wobble = (ring_noise(seg, self.segments, 9, self.seed) - 0.5) * 26.0;
        lerp(self.outer_radius, self.shore_radius + wobble, taper)
    }

    /// Swell height at a point, a couple of low crossing wave trains.
    fn wave(&self, radius: f32, angle: f32) -> f32 {
        0.55 * (radius * 0.05 + angle * 6.0).sin() + 0.35 * (radius * 0.11 - angle * 3.0).sin()
    }

    /// Screen colour of the water `g` of the way from the shore to the horizon.
    ///
    /// Depth does the work: the sea darkens and saturates as the bottom drops
    /// away, and only the last fifth of the bay is allowed to lift toward the
    /// sky. Without that ordering the shallows and the sand meet at the same
    /// value and the waterline disappears.
    fn water_color(&self, g: f32) -> [f32; 3] {
        let water = lerp_rgb(self.shallow, self.deep, smoothstep(g * 1.45));
        lerp_rgb(water, self.horizon, smoothstep((g - 0.78) / 0.22) * 0.30)
    }
}

/// The bay off the coastal ground.
pub(crate) fn coastal_sea(layout: &EnvLayout, horizon: [f32; 3]) -> OceanSpec {
    OceanSpec {
        centre_angle: SEA_ANGLE,
        sweep: PI * 1.25,
        shore_radius: layout.at(0.20),
        // Water is flat, so unlike the props it may run out to the rim and
        // meet the sky rather than stopping on the sand.
        outer_radius: layout.rim() - 6.0,
        segments: 72,
        rings: 10,
        // Turquoise over pale sand, dropping to open-ocean blue. Both sit well
        // under the beach in value and on the other side of neutral in hue,
        // which is what makes the waterline read from the stands.
        shallow: [0.33, 0.71, 0.63],
        deep: [0.09, 0.25, 0.44],
        horizon,
        seed: 3407,
    }
}

pub(crate) fn ocean_mesh(spec: &OceanSpec) -> Mesh {
    let mut mesh = ColorMesh::default();
    let point = |seg: usize, ring: usize| {
        let f = seg as f32 / spec.segments as f32;
        let angle = spec.centre_angle + (f - 0.5) * spec.sweep;
        let shore = spec.shore_at(f);
        // Bias samples toward the shore, where the eye can resolve the water.
        let g = (ring as f32 / spec.rings as f32).powf(1.7);
        let r = lerp(shore, spec.outer_radius, g);
        ring_position(angle, r, spec.wave(r, angle))
    };
    let colour = |ring: usize| spec.water_color(ring as f32 / spec.rings as f32);

    for seg in 0..spec.segments {
        for ring in 0..spec.rings {
            let a = point(seg, ring);
            let b = point(seg + 1, ring);
            let c = point(seg + 1, ring + 1);
            let d = point(seg, ring + 1);
            let (near, far) = (colour(ring), colour(ring + 1));
            mesh.quad([a, b, c, d], [near, near, far, far]);
        }
    }
    mesh.build()
}

/// Concrete of the deepest layer of the skyline, before any haze.
const FAR_TOWER: [f32; 3] = [0.42, 0.44, 0.48];

/// Fraction of the way to the horizon colour a tower `t` of the way across the
/// far band is painted.
///
/// Only enough to seat the blocks in their own air: the camera's distance fog
/// is already worth half the colour of anything out here, and the previous
/// pass' 0.42–0.77 on top of that left the skyline as white cut-outs with no
/// silhouette left to read.
fn skyline_haze(t: f32) -> f32 {
    0.12 + smoothstep(t) * 0.30
}

/// Merged silhouette of far-off tower blocks, hazed toward the horizon colour.
///
/// Beyond ~450 m the kit models are a few pixels wide, so the deepest layer of
/// the skyline is baked into one mesh whose vertex colours already carry the
/// aerial perspective.
pub(crate) fn far_skyline_mesh(
    inner: f32,
    outer: f32,
    slots: usize,
    horizon: [f32; 3],
    seed: u32,
) -> Mesh {
    let mut mesh = ColorMesh::default();
    for slot in 0..slots {
        let s = slot as u32;
        let angle = (slot as f32 + sky_hash(s, 1, seed)) / slots as f32 * TAU;
        let w = city_weight(angle);
        if sky_hash(s, 2, seed) > 0.35 + 0.6 * w {
            continue;
        }
        let radius = lerp(inner, outer, sky_hash(s, 3, seed));
        let height = lerp(45.0, 190.0, sky_hash(s, 4, seed) * (0.25 + 0.75 * w));
        let width = lerp(18.0, 46.0, sky_hash(s, 5, seed));
        let depth = lerp(18.0, 40.0, sky_hash(s, 6, seed));
        let haze = skyline_haze((radius - inner) / (outer - inner).max(1e-3));
        mesh.box_at(
            ring_position(angle, radius, height * 0.5),
            Vec3::new(width * 0.5, height * 0.5, depth * 0.5),
            -angle - FRAC_PI_2,
            lerp_rgb(FAR_TOWER, horizon, haze),
        );
    }
    mesh.build()
}

/// Hedged field boundaries: arcs of clipped hedge broken by radial spurs.
pub(crate) fn hedgerow_mesh(layout: &EnvLayout) -> Mesh {
    const HEDGE: [f32; 3] = [0.22, 0.34, 0.18];
    const ARCS: [(f32, usize, usize); 3] = [(0.10, 96, 7), (0.24, 128, 5), (0.40, 160, 6)];
    let mut mesh = ColorMesh::default();
    for (ring, &(t, segments, gap_every)) in ARCS.iter().enumerate() {
        let radius = layout.at(t);
        let seg_arc = TAU / segments as f32;
        for seg in 0..segments {
            if seg.is_multiple_of(gap_every) {
                continue;
            }
            let angle = seg as f32 * seg_arc;
            let height = lerp(2.2, 3.6, sky_hash(seg as u32, ring as u32, 733));
            mesh.box_at(
                ring_position(angle, radius, height * 0.5),
                Vec3::new(radius * seg_arc * 0.55, height * 0.5, 0.9),
                -angle - FRAC_PI_2,
                lerp_rgb(
                    HEDGE,
                    [0.30, 0.42, 0.24],
                    sky_hash(seg as u32, 9, 733) * 0.5,
                ),
            );
        }
        // Spurs running away from the ground split the fields up.
        for spur in 0..8 {
            let angle = (spur as f32 + 0.5) / 8.0 * TAU + ring as f32 * 0.11;
            let length = (layout.at(t + 0.12) - radius).max(10.0);
            mesh.box_at(
                ring_position(angle, radius + length * 0.5, 1.4),
                Vec3::new(0.9, 1.4, length * 0.5),
                -angle - FRAC_PI_2,
                HEDGE,
            );
        }
    }
    mesh.build()
}

/// Village church: nave, west tower and spire, built in ring-local space.
pub(crate) fn church_mesh() -> Mesh {
    const STONE: [f32; 3] = [0.62, 0.60, 0.54];
    const ROOF: [f32; 3] = [0.30, 0.29, 0.31];
    let mut mesh = ColorMesh::default();
    mesh.box_at(
        Vec3::new(2.0, 4.0, 0.0),
        Vec3::new(9.0, 4.0, 4.5),
        0.0,
        STONE,
    );
    mesh.box_at(
        Vec3::new(2.0, 8.6, 0.0),
        Vec3::new(9.2, 0.7, 4.8),
        0.0,
        ROOF,
    );
    mesh.box_at(
        Vec3::new(-9.0, 9.0, 0.0),
        Vec3::new(3.2, 9.0, 3.2),
        0.0,
        STONE,
    );
    mesh.pyramid(Vec3::new(-9.0, 18.0, 0.0), 3.4, 14.0, 0.0, ROOF);
    mesh.build()
}

// ---------------------------------------------------------------------------
// Themes
// ---------------------------------------------------------------------------

/// Bearing of the downtown core. Everything thins out away from it so the
/// skyline has a peak rather than ringing the ground evenly.
const CITY_CORE_ANGLE: f32 = 2.15;
/// Highest alpine summit, and so the altitude the snow line is measured from.
const ALPINE_SUMMIT: f32 = 215.0;
/// Height of the tallest mesa, which the ochre/red banding is keyed to.
const MESA_CAPROCK: f32 = 88.0;
/// Bearing the coastal ground's sea faces (away from the main stand at `PI`).
const SEA_ANGLE: f32 = 0.0;

/// Weight in `0..1` peaking at the downtown core.
pub(crate) fn city_weight(angle: f32) -> f32 {
    (0.5 + 0.5 * (angle - CITY_CORE_ANGLE).cos()).powf(1.5)
}

/// Weight in `0..1` peaking out to sea.
pub(crate) fn sea_weight(angle: f32) -> f32 {
    (0.5 + 0.5 * (angle - SEA_ANGLE).cos()).powf(1.2)
}

const CITY_TOWERS: [&str; 10] = [
    "environment/city/tower-a.glb",
    "environment/city/tower-c.glb",
    "environment/city/tower-f.glb",
    "environment/city/tower-g.glb",
    "environment/city/tower-i.glb",
    "environment/city/tower-j.glb",
    "environment/city/tower-k.glb",
    "environment/city/tower-l.glb",
    "environment/city/tower-m.glb",
    "environment/city/tower-n.glb",
];
const CITY_SKYSCRAPERS: [&str; 5] = [
    "environment/city/skyscraper-a.glb",
    "environment/city/skyscraper-b.glb",
    "environment/city/skyscraper-c.glb",
    "environment/city/skyscraper-d.glb",
    "environment/city/skyscraper-e.glb",
];
const CITY_BLOCKS: [&str; 14] = [
    "environment/city/block-a.glb",
    "environment/city/block-b.glb",
    "environment/city/block-c.glb",
    "environment/city/block-d.glb",
    "environment/city/block-e.glb",
    "environment/city/block-f.glb",
    "environment/city/block-g.glb",
    "environment/city/block-h.glb",
    "environment/city/block-i.glb",
    "environment/city/block-j.glb",
    "environment/city/block-k.glb",
    "environment/city/block-l.glb",
    "environment/city/block-m.glb",
    "environment/city/block-n.glb",
];
const PINES: [&str; 5] = [
    "environment/nature/pine-tall-a.glb",
    "environment/nature/pine-tall-b.glb",
    "environment/nature/pine-round-a.glb",
    "environment/nature/pine-round-b.glb",
    "environment/nature/pine-small.glb",
];
const PALMS: [&str; 4] = [
    "environment/nature/palm-tall.glb",
    "environment/nature/palm-detailed.glb",
    "environment/nature/palm-bend.glb",
    "environment/nature/palm-short.glb",
];
const ROCKS: [&str; 5] = [
    "environment/nature/rock-large-a.glb",
    "environment/nature/rock-large-b.glb",
    "environment/nature/rock-tall-a.glb",
    "environment/nature/rock-tall-b.glb",
    "environment/nature/stone-large.glb",
];
const SHORE_ROCKS: [&str; 4] = [
    "environment/nature/cliff-large.glb",
    "environment/nature/rock-large-a.glb",
    "environment/nature/rock-tall-a.glb",
    "environment/nature/stone-large.glb",
];
const TREES: [&str; 4] = [
    "environment/nature/tree-oak.glb",
    "environment/nature/tree-tall.glb",
    "environment/nature/tree-detailed.glb",
    "environment/nature/tree-default.glb",
];
const SCRUB: [&str; 2] = [
    "environment/nature/bush-detailed.glb",
    "environment/nature/bush-large.glb",
];
const HOUSES: [&str; 12] = [
    "environment/suburb/house-a.glb",
    "environment/suburb/house-b.glb",
    "environment/suburb/house-c.glb",
    "environment/suburb/house-e.glb",
    "environment/suburb/house-g.glb",
    "environment/suburb/house-h.glb",
    "environment/suburb/house-j.glb",
    "environment/suburb/house-l.glb",
    "environment/suburb/house-n.glb",
    "environment/suburb/house-p.glb",
    "environment/suburb/house-r.glb",
    "environment/suburb/house-t.glb",
];

/// A batch of kit instances sharing one set of glTF scenes, so each `.glb` is
/// loaded once per stadium and the handle cloned per instance.
pub(crate) struct PropGroup {
    pub(crate) scenes: &'static [&'static str],
    pub(crate) placements: Vec<PropPlacement>,
}

/// Every kit prop a theme places. Pure, so the layout can be tested without a
/// renderer and cannot drift from what is actually spawned.
pub(crate) fn theme_prop_groups(theme: StadiumEnvironment, layout: &EnvLayout) -> Vec<PropGroup> {
    match theme {
        StadiumEnvironment::Metropolis => vec![
            PropGroup {
                scenes: &CITY_TOWERS,
                placements: scatter_ring(
                    &ScatterSpec::even(84, layout.at(0.04), layout.at(0.20), 10, 1301)
                        .density(0.85)
                        .scale(11.0, 22.0)
                        .stretch(0.9, 1.9),
                    city_weight,
                ),
            },
            PropGroup {
                scenes: &CITY_SKYSCRAPERS,
                placements: scatter_ring(
                    &ScatterSpec::even(64, layout.at(0.22), layout.at(0.44), 5, 1607)
                        .density(0.82)
                        .scale(13.0, 25.0)
                        .stretch(0.9, 2.1),
                    city_weight,
                ),
            },
            PropGroup {
                scenes: &CITY_BLOCKS,
                placements: scatter_ring(
                    &ScatterSpec::even(110, layout.at(0.47), layout.at(0.72), 14, 1913)
                        .density(0.82)
                        .scale(20.0, 40.0)
                        .stretch(0.7, 1.7),
                    city_weight,
                ),
            },
        ],
        StadiumEnvironment::Alpine => vec![
            PropGroup {
                scenes: &PINES,
                placements: scatter_ring(
                    &ScatterSpec::even(140, layout.at(0.01), layout.at(0.13), 5, 2207)
                        .density(0.85)
                        .scale(6.0, 12.0),
                    |_| 1.0,
                ),
            },
            PropGroup {
                scenes: &PINES,
                placements: scatter_ring(
                    &ScatterSpec::even(96, layout.at(0.14), layout.at(0.26), 5, 2311)
                        .density(0.55)
                        .scale(7.0, 14.0),
                    |_| 1.0,
                ),
            },
            PropGroup {
                scenes: &ROCKS,
                placements: scatter_ring(
                    &ScatterSpec::even(44, layout.at(0.06), layout.at(0.30), 5, 2417)
                        .density(0.5)
                        .scale(6.0, 16.0),
                    |_| 1.0,
                ),
            },
        ],
        StadiumEnvironment::Coastal => vec![
            PropGroup {
                scenes: &PALMS,
                placements: scatter_ring(
                    &ScatterSpec::even(120, layout.at(0.00), layout.at(0.13), 4, 3109)
                        .density(0.8)
                        .scale(6.0, 11.0),
                    sea_weight,
                ),
            },
            PropGroup {
                scenes: &SHORE_ROCKS,
                placements: scatter_ring(
                    &ScatterSpec::even(72, layout.at(0.13), layout.at(0.18), 4, 3203)
                        .density(0.75)
                        .scale(9.0, 26.0),
                    sea_weight,
                ),
            },
            PropGroup {
                scenes: &SCRUB,
                // Kept inside the closest the waterline ever comes, so no bush
                // ends up standing in the sea.
                placements: scatter_ring(
                    &ScatterSpec::even(90, layout.at(0.03), layout.at(0.17), 2, 3301)
                        .density(0.7)
                        .scale(4.0, 9.0),
                    |angle| 1.0 - sea_weight(angle),
                ),
            },
        ],
        StadiumEnvironment::Parkland => vec![
            PropGroup {
                scenes: &TREES,
                placements: scatter_clumps(&ClumpSpec {
                    clumps: 17,
                    per_clump: 11,
                    inner: layout.at(0.01),
                    outer: layout.at(0.34),
                    spread: 26.0,
                    variants: 4,
                    seed: 4111,
                    density: 0.78,
                    scale: (9.0, 15.0),
                }),
            },
            PropGroup {
                scenes: &SCRUB,
                placements: scatter_ring(
                    &ScatterSpec::even(90, layout.at(0.02), layout.at(0.40), 2, 4217)
                        .density(0.6)
                        .scale(4.0, 8.0),
                    |_| 1.0,
                ),
            },
            PropGroup {
                scenes: &HOUSES,
                placements: scatter_ring(
                    &ScatterSpec::even(110, layout.at(0.44), layout.at(0.66), 12, 4327)
                        .density(0.7)
                        .scale(7.0, 12.0),
                    |_| 1.0,
                ),
            },
        ],
        StadiumEnvironment::Desert => vec![
            PropGroup {
                scenes: &ROCKS,
                placements: scatter_ring(
                    &ScatterSpec::even(96, layout.at(0.02), layout.at(0.44), 5, 5101)
                        .density(0.6)
                        .scale(7.0, 22.0),
                    |_| 1.0,
                ),
            },
            PropGroup {
                scenes: &SCRUB,
                placements: scatter_ring(
                    &ScatterSpec::even(170, layout.at(0.00), layout.at(0.40), 2, 5209)
                        .density(0.7)
                        .scale(3.0, 7.0),
                    |_| 1.0,
                ),
            },
        ],
    }
}

/// The merged landform meshes a theme needs, in local (stadium) space.
pub(crate) fn theme_landforms(
    theme: StadiumEnvironment,
    layout: &EnvLayout,
) -> Vec<(Mesh, Transform, LandformSurface)> {
    let horizon = sky_horizon_color(theme, false);
    match theme {
        StadiumEnvironment::Metropolis => vec![(
            far_skyline_mesh(layout.at(0.78), layout.at(0.92), 120, horizon, 1723),
            Transform::IDENTITY,
            LandformSurface::Terrain,
        )],
        // Three ranges at increasing distance and height give the valley
        // parallax; only the back one carries snow, because they all read the
        // colour ramp against the same summit altitude.
        StadiumEnvironment::Alpine => [
            (0.26_f32, 78.0_f32, 46.0_f32, 90.0_f32, 2503_u32),
            (0.55, 96.0, 70.0, 150.0, 2609),
            (0.74, 84.0, 110.0, ALPINE_SUMMIT, 2711),
        ]
        .iter()
        .map(|&(t, width, min_h, max_h, seed)| {
            (
                ridge_mesh(&RidgeSpec {
                    segments: 96,
                    crest_radius: layout.at(t),
                    half_width: width,
                    min_height: min_h,
                    max_height: max_h,
                    plateau: 0.06,
                    terraces: 0,
                    ramp_height: ALPINE_SUMMIT,
                    seed,
                    ramp: ALPINE_RAMP,
                }),
                Transform::IDENTITY,
                LandformSurface::Terrain,
            )
        })
        .collect(),
        StadiumEnvironment::Coastal => {
            let mut out = vec![(
                ocean_mesh(&coastal_sea(layout, horizon)),
                // Sea level sits just proud of the sand so the swell never
                // clips through the beach at the waterline.
                Transform::from_translation(Vec3::Y * 0.95),
                LandformSurface::Water,
            )];
            // Islands out to sea, then two headlands behind the main stand so
            // the landward half of the horizon is not bare sand.
            for (i, &(bearing, t, radius, height)) in [
                (-0.62_f32, 0.68_f32, 78.0_f32, 52.0_f32),
                (0.18, 0.78, 88.0, 68.0),
                (0.94, 0.60, 56.0, 38.0),
                (PI - 0.52, 0.58, 90.0, 46.0),
                (PI + 0.46, 0.62, 74.0, 34.0),
            ]
            .iter()
            .enumerate()
            {
                out.push((
                    island_mesh(radius, height, 3511 + i as u32 * 17),
                    Transform::from_translation(ring_position(
                        SEA_ANGLE + bearing,
                        layout.at(t),
                        0.2,
                    )),
                    LandformSurface::Terrain,
                ));
            }
            out
        }
        StadiumEnvironment::Parkland => vec![
            (
                hedgerow_mesh(layout),
                Transform::IDENTITY,
                LandformSurface::Terrain,
            ),
            (
                ridge_mesh(&RidgeSpec {
                    segments: 72,
                    crest_radius: layout.at(0.82),
                    half_width: 60.0,
                    min_height: 12.0,
                    max_height: 26.0,
                    plateau: 0.45,
                    terraces: 0,
                    ramp_height: 26.0,
                    seed: 4409,
                    ramp: TREELINE_RAMP,
                }),
                Transform::IDENTITY,
                LandformSurface::Terrain,
            ),
            (
                church_mesh(),
                ring_segment_transform(0.82, layout.at(0.55), 0.0),
                LandformSurface::Terrain,
            ),
        ],
        // Flat-topped and terraced, with the rock banding read against a shared
        // altitude so the beds line up between the near and far mesas.
        StadiumEnvironment::Desert => [
            (0.42_f32, 74.0_f32, 22.0_f32, 52.0_f32, 5303_u32, 4_usize),
            (0.70, 92.0, 40.0, MESA_CAPROCK, 5407, 5),
        ]
        .iter()
        .map(|&(t, width, min_h, max_h, seed, terraces)| {
            (
                ridge_mesh(&RidgeSpec {
                    segments: 84,
                    crest_radius: layout.at(t),
                    half_width: width,
                    min_height: min_h,
                    max_height: max_h,
                    plateau: 0.72,
                    terraces,
                    ramp_height: MESA_CAPROCK,
                    seed,
                    ramp: MESA_RAMP,
                }),
                Transform::IDENTITY,
                LandformSurface::Terrain,
            )
        })
        .collect(),
    }
}

/// Which shared material a landform is drawn with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LandformSurface {
    Terrain,
    Water,
}

/// Ground colour under and around the stadium for each theme, as it should
/// read on screen.
///
/// Each has to say what the site is made of on its own, and every one of them
/// has to sit under the bowl's concrete next door (`0x8A8E92`, which renders at
/// about 0.90): ground brighter than the building standing on it is the single
/// thing that made all five of these read as snow.
pub(crate) fn ground_srgb(theme: StadiumEnvironment) -> [f32; 3] {
    match theme {
        // Service tarmac and car parks: dark, faintly blue-grey.
        StadiumEnvironment::Metropolis => [0.32, 0.34, 0.38],
        // High pasture — paler and yellower than lowland turf.
        StadiumEnvironment::Alpine => [0.44, 0.56, 0.31],
        StadiumEnvironment::Coastal => BEACH_SAND,
        // English turf, a shade duller and darker than the mown square.
        StadiumEnvironment::Parkland => [0.35, 0.60, 0.28],
        // Sun-bleached ochre dust, warmer and lighter than the mesa beds.
        StadiumEnvironment::Desert => [0.70, 0.53, 0.32],
    }
}

/// Ground tint under and around the stadium for each theme.
pub(crate) fn ground_palette(theme: StadiumEnvironment) -> GroundPalette {
    GroundPalette {
        base: day_albedo(ground_srgb(theme)),
        // The rim has to dissolve into this theme's own air, not a fixed blue.
        // Converted the same way as everything else, so the last ring of the
        // disc and the sky texel above it reach the tonemapper as one number.
        horizon: day_albedo(sky_horizon_color(theme, false)),
    }
}

// ---------------------------------------------------------------------------
// Spawning
// ---------------------------------------------------------------------------

/// Populate the world outside the bowl for `ctx.stadium.environment`.
pub(crate) fn spawn_environment(
    p: &mut ChildSpawnerCommands,
    ctx: &mut StadiumBuildCtx<'_>,
    spawn_count: &mut usize,
) {
    let theme = ctx.stadium.environment;
    let layout = EnvLayout::new(ctx.bowl.outer_radius());

    retint_ground(ctx, theme);
    spawn_landforms(p, ctx, theme, &layout, spawn_count);

    for group in theme_prop_groups(theme, &layout) {
        let scenes: Vec<Handle<Scene>> = group
            .scenes
            .iter()
            .map(|path| {
                ctx.asset_server
                    .load(GltfAssetLabel::Scene(0).from_asset(*path))
            })
            .collect();
        for placement in &group.placements {
            p.spawn((
                EnvironmentProp,
                SceneRoot(scenes[placement.variant % scenes.len()].clone()),
                Transform::from_translation(placement.pos)
                    .with_rotation(Quat::from_rotation_y(placement.yaw))
                    .with_scale(placement.scale),
                Visibility::default(),
                InheritedVisibility::default(),
                ViewVisibility::default(),
            ));
            track_spawn(spawn_count);
        }
    }
}

/// Repaint the apron disc that `build_stadium` already spawned: same handles,
/// theme's ground and horizon colours.
fn retint_ground(ctx: &mut StadiumBuildCtx<'_>, theme: StadiumEnvironment) {
    let palette = ground_palette(theme);
    let radius = stadium_ground_radius(ctx.bowl.outer_radius());
    if let Some(mesh) = ctx.meshes.get_mut(&ctx.apron_disc_mesh) {
        *mesh = stadium_ground_disc_mesh_tinted(radius, GROUND_DISC_SEGMENTS, palette);
    }
    if let Some(material) = ctx.materials.get_mut(&ctx.shared.apron_mat) {
        // Asphalt is the only one of these with a sheen worth having. The
        // establishing lens looks along the apron rather than down onto it, and
        // at that angle Fresnel lifts a slice of sky off any reflectance left
        // here — which is what turned the beach grey-green even once its albedo
        // was right. Sand and dust are matte in life; keep them matte.
        let (roughness, reflectance) = match theme {
            StadiumEnvironment::Metropolis => (0.84, 0.28),
            StadiumEnvironment::Coastal => (0.92, 0.18),
            _ => (0.95, 0.16),
        };
        material.perceptual_roughness = roughness;
        material.reflectance = reflectance;
    }
}

fn spawn_landforms(
    p: &mut ChildSpawnerCommands,
    ctx: &mut StadiumBuildCtx<'_>,
    theme: StadiumEnvironment,
    layout: &EnvLayout,
    spawn_count: &mut usize,
) {
    let landforms = theme_landforms(theme, layout);
    if landforms.is_empty() {
        return;
    }
    // Two shared materials cover every landform; the colour lives in the mesh.
    let terrain = ctx.materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.95,
        reflectance: 0.16,
        ..default()
    });
    let water = ctx.materials.add(StandardMaterial {
        base_color: Color::WHITE,
        // Water is F0 ≈ 0.04, which `reflectance` squares into 0.5, and a bay
        // has enough chop to spread the sun. A mirror finish on flat-shaded
        // swell quads scattered blown-out glints across the whole sea instead.
        perceptual_roughness: 0.16,
        reflectance: 0.5,
        metallic: 0.0,
        ..default()
    });
    for (mesh, transform, surface) in landforms {
        let material = match surface {
            LandformSurface::Terrain => terrain.clone(),
            LandformSurface::Water => water.clone(),
        };
        p.spawn((
            EnvironmentProp,
            Mesh3d(ctx.meshes.add(mesh)),
            MeshMaterial3d(material),
            transform,
            // Far outside the shadow cascades; rendering them into the maps
            // would cost a lot and change nothing on screen.
            NotShadowCaster,
            NotShadowReceiver,
        ));
        track_spawn(spawn_count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::stadium::BowlLayout;

    /// Standard bowl used across the layout tests (~101 m outer radius).
    fn bowl_outer() -> f32 {
        BowlLayout::from_boundary(65.0).outer_radius()
    }

    fn layout() -> EnvLayout {
        EnvLayout::new(bowl_outer())
    }

    fn all_placements(theme: StadiumEnvironment) -> Vec<PropPlacement> {
        theme_prop_groups(theme, &layout())
            .into_iter()
            .flat_map(|g| g.placements)
            .collect()
    }

    fn mesh_colors(mesh: &Mesh) -> Vec<[f32; 4]> {
        match mesh.attribute(Mesh::ATTRIBUTE_COLOR) {
            Some(bevy::mesh::VertexAttributeValues::Float32x4(c)) => c.clone(),
            other => panic!("expected float4 vertex colours, got {other:?}"),
        }
    }

    fn mesh_positions(mesh: &Mesh) -> Vec<Vec3> {
        match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
            Some(bevy::mesh::VertexAttributeValues::Float32x3(v)) => {
                v.iter().map(|p| Vec3::from_array(*p)).collect()
            }
            other => panic!("expected float3 positions, got {other:?}"),
        }
    }

    #[test]
    fn annulus_starts_outside_the_bowl_and_ends_inside_the_dome() {
        let l = layout();
        assert!(l.inner > bowl_outer() + 40.0, "inner edge {}", l.inner);
        assert!(l.outer < stadium_ground_radius(bowl_outer()));
        assert!(l.at(0.0) == l.inner && l.at(1.0) == l.outer);
    }

    #[test]
    fn props_keep_clear_of_the_bowl() {
        for theme in StadiumEnvironment::ALL {
            for placement in all_placements(theme) {
                let clearance = placement.radius() - placement.half_extent();
                assert!(
                    clearance > bowl_outer() + 20.0,
                    "{theme:?} prop at r={:.1} intrudes on the bowl",
                    placement.radius()
                );
            }
        }
    }

    #[test]
    fn props_stay_inside_the_sky_dome() {
        let limit = stadium_ground_radius(bowl_outer());
        for theme in StadiumEnvironment::ALL {
            for placement in all_placements(theme) {
                assert!(
                    placement.radius() + placement.half_extent() < limit,
                    "{theme:?} prop at r={:.1} pokes past the ground disc",
                    placement.radius()
                );
            }
        }
    }

    #[test]
    fn props_clear_the_broadcast_sightline() {
        // A high camera anywhere on the bowl looking at the pitch must not have
        // a prop between it and the middle.
        let eye_radius = bowl_outer() - 6.0;
        for theme in StadiumEnvironment::ALL {
            let placements = all_placements(theme);
            for step in 0..12 {
                let angle = step as f32 / 12.0 * TAU;
                let eye_pos = ring_position(angle, eye_radius, 40.0);
                let eye = Vec2::new(eye_pos.x, eye_pos.z);
                for placement in &placements {
                    let prop = Vec2::new(placement.pos.x, placement.pos.z);
                    let d = point_to_segment_distance(prop, eye, Vec2::ZERO);
                    assert!(
                        d > placement.half_extent(),
                        "{theme:?} prop at {:?} blocks the sightline from {eye:?}",
                        placement.pos
                    );
                }
            }
        }
    }

    fn point_to_segment_distance(p: Vec2, a: Vec2, b: Vec2) -> f32 {
        let ab = b - a;
        let t = if ab.length_squared() < 1e-6 {
            0.0
        } else {
            ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
        };
        (p - (a + ab * t)).length()
    }

    #[test]
    fn layout_is_deterministic() {
        for theme in StadiumEnvironment::ALL {
            assert_eq!(
                all_placements(theme),
                all_placements(theme),
                "{theme:?} layout changed between calls"
            );
        }
    }

    #[test]
    fn every_theme_lands_in_the_entity_budget() {
        let mut worst = 0usize;
        for theme in StadiumEnvironment::ALL {
            let props = all_placements(theme).len();
            let landforms = theme_landforms(theme, &layout()).len();
            let total = props + landforms + 1; // + the themed sky dome
            assert!(props >= 60, "{theme:?} world is bare: {props} props");
            assert!(total < 400, "{theme:?} spawns {total} entities");
            worst = worst.max(total);
        }
        assert!(worst < 1500, "surroundings budget blown: {worst}");
    }

    #[test]
    fn variants_index_their_own_scene_list() {
        for theme in StadiumEnvironment::ALL {
            for group in theme_prop_groups(theme, &layout()) {
                for placement in &group.placements {
                    assert!(
                        placement.variant < group.scenes.len(),
                        "{theme:?} variant {} out of range",
                        placement.variant
                    );
                }
            }
        }
    }

    #[test]
    fn props_are_scaled_and_upright() {
        for theme in StadiumEnvironment::ALL {
            for placement in all_placements(theme) {
                assert!(placement.scale.min_element() > 0.5);
                assert!(placement.scale.max_element() < 90.0);
                assert_eq!(placement.pos.y, 0.0, "{theme:?} prop floats or sinks");
            }
        }
    }

    #[test]
    fn city_weight_peaks_at_the_core() {
        assert!((city_weight(CITY_CORE_ANGLE) - 1.0).abs() < 1e-5);
        assert!(city_weight(CITY_CORE_ANGLE + PI) < 1e-5);
        // Wrapping the circle must not change the weighting.
        assert!((city_weight(CITY_CORE_ANGLE + TAU) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn skyline_rises_toward_the_downtown_core() {
        let placements = all_placements(StadiumEnvironment::Metropolis);
        let mean_height = |near_core: bool| {
            let heights: Vec<f32> = placements
                .iter()
                .filter(|p| {
                    let angle = p.pos.z.atan2(p.pos.x);
                    ((angle - CITY_CORE_ANGLE).cos() > 0.5) == near_core
                })
                .map(|p| p.scale.y)
                .collect();
            assert!(!heights.is_empty());
            heights.iter().sum::<f32>() / heights.len() as f32
        };
        assert!(
            mean_height(true) > mean_height(false) * 1.25,
            "core {:.1} vs fringe {:.1}",
            mean_height(true),
            mean_height(false)
        );
    }

    #[test]
    fn coastal_palms_favour_the_seaward_side() {
        let palms = theme_prop_groups(StadiumEnvironment::Coastal, &layout())
            .into_iter()
            .next()
            .expect("coastal palms")
            .placements;
        assert!(!palms.is_empty());
        let mean_weight = palms
            .iter()
            .map(|p| sea_weight(p.pos.z.atan2(p.pos.x)))
            .sum::<f32>()
            / palms.len() as f32;
        // An unbiased scatter would average the weight over the whole circle,
        // which is a little under 0.5.
        assert!(
            mean_weight > 0.55,
            "palms ignore the shoreline: mean seaward weight {mean_weight:.2}"
        );
    }

    #[test]
    fn landforms_stay_inside_the_dome_and_on_the_ground() {
        let limit = stadium_ground_radius(bowl_outer());
        for theme in StadiumEnvironment::ALL {
            for (mesh, transform, _) in theme_landforms(theme, &layout()) {
                let positions = mesh_positions(&mesh);
                assert!(!positions.is_empty(), "{theme:?} landform is empty");
                for local in positions {
                    let world = transform.transform_point(local);
                    assert!(
                        planar_radius(world) < limit,
                        "{theme:?} landform reaches r={:.1}, past the ground disc",
                        planar_radius(world)
                    );
                    assert!(world.y >= -0.5, "{theme:?} landform dips below the ground");
                    assert!(
                        world.y < 260.0,
                        "{theme:?} landform spikes to {:.1} m",
                        world.y
                    );
                }
            }
        }
    }

    #[test]
    fn landforms_clear_the_bowl() {
        for theme in StadiumEnvironment::ALL {
            for (mesh, transform, _) in theme_landforms(theme, &layout()) {
                for local in mesh_positions(&mesh) {
                    let world = transform.transform_point(local);
                    assert!(
                        planar_radius(world) > bowl_outer() + 20.0,
                        "{theme:?} landform at r={:.1} crowds the stands",
                        planar_radius(world)
                    );
                }
            }
        }
    }

    #[test]
    fn ridge_mesh_is_deterministic_and_bounded() {
        let spec = RidgeSpec {
            segments: 48,
            crest_radius: 300.0,
            half_width: 60.0,
            min_height: 40.0,
            max_height: 120.0,
            plateau: 0.1,
            terraces: 0,
            ramp_height: 120.0,
            seed: 17,
            ramp: ALPINE_RAMP,
        };
        let a = mesh_positions(&ridge_mesh(&spec));
        let b = mesh_positions(&ridge_mesh(&spec));
        assert_eq!(a, b);
        assert!(!a.is_empty());
        let max_height = a.iter().fold(0.0_f32, |acc, p| acc.max(p.y));
        assert!(
            max_height > spec.min_height && max_height <= spec.max_height + 1e-3,
            "ridge peaked at {max_height}"
        );
        for p in &a {
            let r = planar_radius(*p);
            // Crest wander is capped at ±27.5% of the half-width.
            assert!(r > spec.crest_radius - spec.half_width * 1.3);
            assert!(r < spec.outer_foot() + spec.half_width * 0.3);
        }
    }

    #[test]
    fn only_the_high_range_wears_snow() {
        let ranges = theme_landforms(StadiumEnvironment::Alpine, &layout());
        // Snow is counted by what it comes out as on screen: as an albedo it is
        // a sixth of that, because the exposure gives most of it back.
        let snow_fraction = |mesh: &Mesh| {
            let colors = mesh_colors(mesh);
            let white = colors
                .iter()
                .map(|c| rendered_srgb([c[0], c[1], c[2]]))
                .filter(|s| s[0] > 0.8 && s[1] > 0.8 && s[2] > 0.8)
                .count();
            white as f32 / colors.len() as f32
        };
        // One snow line across the valley: the foothills stay green even though
        // they are as tall relative to themselves as the back range is.
        assert_eq!(
            snow_fraction(&ranges[0].0),
            0.0,
            "the near foothills should be below the snow line"
        );
        assert!(
            snow_fraction(&ranges[2].0) > 0.05,
            "the back range should be capped with snow"
        );
    }

    #[test]
    fn mesa_terraces_flatten_the_tops() {
        let spec = RidgeSpec {
            segments: 96,
            crest_radius: 300.0,
            half_width: 60.0,
            min_height: 20.0,
            max_height: 60.0,
            plateau: 0.7,
            terraces: 4,
            ramp_height: 60.0,
            seed: 23,
            ramp: MESA_RAMP,
        };
        let heights: Vec<f32> = (0..spec.segments).map(|s| spec.height(s)).collect();
        let mut distinct: Vec<f32> = heights.clone();
        distinct.sort_by(f32::total_cmp);
        distinct.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
        assert!(
            distinct.len() <= spec.terraces,
            "terracing produced {} levels",
            distinct.len()
        );
        assert!(distinct.len() > 1, "mesas collapsed to one height");
    }

    #[test]
    fn ocean_covers_the_seaward_arc_only() {
        let l = layout();
        let spec = OceanSpec {
            centre_angle: SEA_ANGLE,
            sweep: PI * 1.25,
            shore_radius: l.at(0.20),
            outer_radius: l.rim() - 6.0,
            segments: 48,
            rings: 6,
            shallow: [0.2, 0.6, 0.6],
            deep: [0.0, 0.1, 0.3],
            horizon: sky_horizon_color(StadiumEnvironment::Coastal, false),
            seed: 5,
        };
        // The bay pinches shut at both ends, leaving dry land behind the stands.
        assert!((spec.shore_at(0.0) - spec.outer_radius).abs() < 1.0);
        assert!((spec.shore_at(1.0) - spec.outer_radius).abs() < 1.0);
        assert!(spec.shore_at(0.5) < spec.shore_radius + 30.0);

        let positions = mesh_positions(&ocean_mesh(&spec));
        assert!(!positions.is_empty());
        for p in &positions {
            let angle = p.z.atan2(p.x);
            let offset = (angle - SEA_ANGLE)
                .abs()
                .min(TAU - (angle - SEA_ANGLE).abs());
            assert!(
                offset <= spec.sweep * 0.5 + 1e-3,
                "water spilled to bearing {angle}"
            );
            assert!(p.y.abs() < 1.5, "swell of {} m", p.y);
        }
    }

    /// Screen colour a lit albedo comes back as, i.e. the inverse of
    /// [`day_albedo`]. The tonemapper is left out on purpose: it is applied to
    /// the sky and to the ground alike, so two surfaces that agree here agree
    /// on screen as well.
    fn rendered_srgb(albedo: [f32; 3]) -> [f32; 3] {
        let encode = |c: f32| {
            if c <= 0.003_130_8 {
                c * 12.92
            } else {
                1.055 * c.powf(1.0 / 2.4) - 0.055
            }
        };
        [
            encode(albedo[0] * DAY_FLAT_RESPONSE[0]),
            encode(albedo[1] * DAY_FLAT_RESPONSE[1]),
            encode(albedo[2] * DAY_FLAT_RESPONSE[2]),
        ]
    }

    fn luma(rgb: [f32; 3]) -> f32 {
        rgb[0] * 0.299 + rgb[1] * 0.587 + rgb[2] * 0.114
    }

    /// The bowl's concourse concrete (`0x8A8E92` in `stadium.rs`) — the
    /// brightest large surface any of these grounds is seen next to.
    const STADIUM_CONCRETE: [f32; 3] = [0.541, 0.557, 0.573];

    #[test]
    fn day_albedo_round_trips_to_the_colour_it_was_asked_for() {
        for srgb in [
            [0.0, 0.0, 0.0],
            [0.35, 0.37, 0.40],
            BEACH_SAND,
            [1.26, 1.28, 1.34],
        ] {
            let back = rendered_srgb(day_albedo(srgb));
            for c in 0..3 {
                assert!(
                    (back[c] - srgb[c]).abs() < 1e-3,
                    "{srgb:?} came back as {back:?}"
                );
            }
        }
        // The whole point: a colour that would clip as a raw albedo lands well
        // inside range once the exposure is taken out of it.
        for c in day_albedo([0.8, 0.8, 0.8]) {
            assert!(
                c < 0.12,
                "day_albedo still returns a near-white albedo: {c}"
            );
        }
    }

    #[test]
    fn ground_palettes_are_distinct_and_match_their_sky() {
        let palettes: Vec<GroundPalette> = StadiumEnvironment::ALL
            .iter()
            .map(|t| ground_palette(*t))
            .collect();
        for (i, a) in palettes.iter().enumerate() {
            let theme = StadiumEnvironment::ALL[i];
            // The rim has to reach the tonemapper as the same number the sky
            // texel above it does, or the edge of the world glares.
            let rim = rendered_srgb(a.horizon);
            let sky = sky_horizon_color(theme, false);
            for c in 0..3 {
                assert!(
                    (rim[c] - sky[c]).abs() < 1e-3,
                    "{theme:?} rim renders {rim:?}, sky is {sky:?}"
                );
            }
            for b in palettes.iter().skip(i + 1) {
                assert_ne!(a.base, b.base, "two themes share a ground colour");
            }
        }
    }

    /// Radiance an albedo returns to the camera before tonemapping — the space
    /// two surfaces have to be compared in to know which looks brighter.
    fn radiance(albedo: [f32; 3]) -> [f32; 3] {
        [
            albedo[0] * DAY_FLAT_RESPONSE[0],
            albedo[1] * DAY_FLAT_RESPONSE[1],
            albedo[2] * DAY_FLAT_RESPONSE[2],
        ]
    }

    #[test]
    fn every_ground_reads_darker_than_the_stadium_on_it() {
        // `stadium.rs` paints the concourse with an sRGB base colour, so its
        // albedo is that colour decoded — which the exposure then returns ten
        // times over. That is the wall of white the ground has to sit under.
        let concrete = luma(radiance(STADIUM_CONCRETE.map(srgb_to_linear)));
        for theme in StadiumEnvironment::ALL {
            let ground = ground_srgb(theme);
            let lit = luma(radiance(day_albedo(ground)));
            assert!(
                lit < concrete * 0.5,
                "{theme:?} ground {ground:?} returns {lit:.2} against concrete's {concrete:.2}"
            );
            // And it must survive the trip through the exposure as an albedo
            // that is nowhere near clipping.
            for c in day_albedo(ground) {
                assert!(c > 0.0 && c < 0.2, "{theme:?} albedo channel {c} is unsafe");
            }
        }
    }

    #[test]
    fn sand_is_warm_and_the_meadows_are_green() {
        let sand = ground_srgb(StadiumEnvironment::Coastal);
        let dust = ground_srgb(StadiumEnvironment::Desert);
        for warm in [sand, dust] {
            assert!(
                warm[0] > warm[1] && warm[1] > warm[2],
                "{warm:?} is not warm"
            );
        }
        // What tells the two warm grounds apart is value, not hue: bleached
        // beach sand against darker ochre dust.
        assert!(
            luma(sand) - luma(dust) > 0.05,
            "sand {sand:?} and dust {dust:?} are the same material"
        );
        for green in [
            ground_srgb(StadiumEnvironment::Alpine),
            ground_srgb(StadiumEnvironment::Parkland),
        ] {
            assert!(
                green[1] > green[0] && green[1] > green[2],
                "{green:?} is not green"
            );
        }
        // Tarmac is the only one that leans cool.
        let tarmac = ground_srgb(StadiumEnvironment::Metropolis);
        assert!(tarmac[2] > tarmac[0], "{tarmac:?} is not a cool grey");
    }

    #[test]
    fn color_ramp_interpolates_between_stops() {
        let ramp = ColorRamp(&[(0.0, [0.0, 0.0, 0.0]), (1.0, [1.0, 1.0, 1.0])]);
        assert_eq!(ramp.sample(-1.0), [0.0, 0.0, 0.0]);
        assert_eq!(ramp.sample(2.0), [1.0, 1.0, 1.0]);
        let mid = ramp.sample(0.5);
        assert!((mid[0] - 0.5).abs() < 1e-5);
    }

    #[test]
    fn clumped_trees_stay_in_their_annulus() {
        let l = layout();
        let spec = ClumpSpec {
            clumps: 8,
            per_clump: 9,
            inner: l.at(0.05),
            outer: l.at(0.30),
            spread: 30.0,
            variants: 4,
            seed: 3,
            density: 1.0,
            scale: (9.0, 14.0),
        };
        let placements = scatter_clumps(&spec);
        assert_eq!(placements.len(), spec.clumps * spec.per_clump);
        for placement in &placements {
            assert!(placement.radius() >= spec.inner - 1e-3);
            assert!(placement.radius() <= spec.outer + 1e-3);
        }
        // Clumps, not a ring: members should sit far closer to each other than
        // an even scatter of the same count would.
        let mut nearest_sum = 0.0;
        for a in &placements {
            let nearest = placements
                .iter()
                .filter(|b| b.pos != a.pos)
                .map(|b| a.pos.distance(b.pos))
                .fold(f32::MAX, f32::min);
            nearest_sum += nearest;
        }
        let mean_nearest = nearest_sum / placements.len() as f32;
        assert!(mean_nearest < 20.0, "trees are not clumped: {mean_nearest}");
    }

    #[test]
    fn church_and_hedges_build_geometry() {
        assert!(!mesh_positions(&church_mesh()).is_empty());
        let hedges = hedgerow_mesh(&layout());
        let positions = mesh_positions(&hedges);
        assert!(positions.len() > 100);
        for p in &positions {
            assert!(p.y > -1e-3 && p.y < 5.0, "hedge height {}", p.y);
        }
    }

    #[test]
    fn far_skyline_hazes_toward_the_horizon() {
        let l = layout();
        let horizon = sky_horizon_color(StadiumEnvironment::Metropolis, false);
        let mesh = far_skyline_mesh(l.at(0.78), l.at(0.92), 120, horizon, 1723);
        let colors = mesh_colors(&mesh);
        assert!(!colors.is_empty());
        let sky = luma(horizon);
        // The shaded sides of a block are 18% under its lit top, so that is the
        // floor; the sky is the ceiling. Between the two the towers still have
        // a silhouette to read.
        let floor = luma(FAR_TOWER) * 0.8;
        for c in &colors {
            let value = luma(rendered_srgb([c[0], c[1], c[2]]));
            assert!(
                value > floor,
                "far building is darker than its own concrete: {c:?}"
            );
            assert!(
                value < sky - 0.05,
                "far building has dissolved into the sky: {c:?}"
            );
        }
    }

    #[test]
    fn skyline_haze_only_seats_the_towers_in_their_air() {
        assert!(skyline_haze(0.0) > 0.0, "the far band gets no haze at all");
        assert!(
            skyline_haze(1.0) > skyline_haze(0.0),
            "haze is not distance-keyed"
        );
        // Distance fog is worth about half the colour of anything this far out
        // on its own, so the baked haze has to stay a minority share or the
        // silhouette goes.
        assert!(
            skyline_haze(1.0) < 0.5,
            "haze of {} leaves nothing to read",
            skyline_haze(1.0)
        );
    }

    fn coastal_ocean() -> OceanSpec {
        coastal_sea(
            &layout(),
            sky_horizon_color(StadiumEnvironment::Coastal, false),
        )
    }

    #[test]
    fn the_sea_separates_from_the_sand() {
        let spec = coastal_ocean();
        let sand = ground_srgb(StadiumEnvironment::Coastal);
        for water in [spec.water_color(0.0), spec.water_color(0.5)] {
            assert!(
                luma(sand) - luma(water) > 0.08,
                "sand {sand:?} and water {water:?} share a value"
            );
            // Opposite sides of neutral: the sand runs warm, the sea cool.
            assert!(
                sand[0] - sand[2] > 0.1 && water[2] - water[0] > 0.1,
                "sand {sand:?} and water {water:?} share a hue"
            );
        }
    }

    #[test]
    fn the_sea_deepens_and_saturates_with_distance() {
        let spec = coastal_ocean();
        let saturation = |c: [f32; 3]| {
            let max = c[0].max(c[1]).max(c[2]);
            let min = c[0].min(c[1]).min(c[2]);
            if max > 1e-4 { (max - min) / max } else { 0.0 }
        };
        // Depth owns everything up to the horizon wash, and has to darken and
        // saturate the whole way: that gradient is the only thing that says
        // "water" rather than "more haze".
        let deepening: Vec<[f32; 3]> = (0..=8)
            .map(|i| spec.water_color(i as f32 / 8.0 * 0.78))
            .collect();
        for pair in deepening.windows(2) {
            assert!(
                luma(pair[1]) < luma(pair[0]) + 1e-4,
                "the sea does not darken: {:?} then {:?}",
                pair[0],
                pair[1]
            );
        }
        let shore = deepening[0];
        let far = *deepening.last().expect("water samples");
        assert!(
            luma(shore) - luma(far) > 0.2,
            "the sea barely deepens at all"
        );
        assert!(
            saturation(far) > saturation(shore) + 0.05,
            "the sea does not saturate as it deepens: {shore:?} then {far:?}"
        );
        // The horizon wash may tint the last of the water but must not hand it
        // over to the sky, or the sea reads as more haze.
        let rim = spec.water_color(1.0);
        let horizon = spec.horizon;
        for c in 0..3 {
            assert!(
                (rim[c] - horizon[c]).abs() > (spec.deep[c] - horizon[c]).abs() * 0.5,
                "the far water has washed out into the sky: {rim:?}"
            );
        }
    }

    #[test]
    fn snow_is_the_only_thing_brighter_than_white() {
        // Everything else has to be a colour a screen can show; snow is the one
        // surface that really does out-run the sky behind it.
        let summit = ALPINE_RAMP.sample(1.0);
        assert!(
            summit.iter().all(|c| *c > 1.0),
            "snow {summit:?} is clamped"
        );
        for t in [0.0, 0.2, 0.44, 0.6] {
            let stop = ALPINE_RAMP.sample(t);
            assert!(
                stop.iter().all(|c| *c <= 1.0),
                "{stop:?} at {t} is over white"
            );
        }
        for ramp in [&MESA_RAMP, &TREELINE_RAMP, &ISLAND_RAMP] {
            for step in 0..=10 {
                let stop = ramp.sample(step as f32 / 10.0);
                assert!(stop.iter().all(|c| *c > 0.0 && *c <= 1.0), "{stop:?}");
            }
        }
    }

    #[test]
    fn the_islands_are_made_of_the_same_sand_as_the_beach() {
        assert_eq!(
            ISLAND_RAMP.sample(0.0),
            ground_srgb(StadiumEnvironment::Coastal)
        );
        // And they still band up out of it rather than staying one pale lump.
        let banded: Vec<[f32; 3]> = [0.0, 0.3, 0.6, 1.0]
            .iter()
            .map(|t| ISLAND_RAMP.sample(*t))
            .collect();
        for (i, a) in banded.iter().enumerate() {
            for b in banded.iter().skip(i + 1) {
                assert!(
                    (luma(*a) - luma(*b)).abs() > 0.04,
                    "island bands {a:?} and {b:?} are the same value"
                );
            }
        }
    }
}

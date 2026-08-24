//! Stadium crowd population.
//!
//! Spectators are placed directly onto [`sg::SeatGrid`] — the same seat layout
//! `stand_geometry` cuts the seat shells from — so a spectator is *in* seat
//! `(seg, k)` by construction. The crowd used to carry its own segmentation,
//! pitch and aisle rules, which is how it ended up standing on the tread floor
//! between the seats instead of sitting in them.
//!
//! Two render bands share that grid:
//! - near-camera lower tiers use the glTF crowd kit for readable silhouettes
//! - everything else is merged into one vertex-coloured mesh per deck
//!
//! Both bands derive their stature from the seat's own dimensions, so the seam
//! between them is invisible.

use bevy::gltf::{GltfAssetLabel, GltfMaterialName};
use bevy::prelude::*;

use crate::core::geometry;
use crate::render::ring_geometry::ring_face_center_rotation;
use crate::render::stadium::{
    BowlLayout, LOWER_TIER_COUNT, StadiumBuildCtx, TIER_COUNT, track_spawn,
};
use crate::render::stand_geometry as sg;

#[derive(Clone, Copy, Debug)]
pub struct CrowdVariant {
    pub path: &'static str,
    pub seated: bool,
}

/// All 15 posed-human GLBs under `assets/crowd/posed/`.
pub const CROWD_VARIANTS: [CrowdVariant; 15] = [
    CrowdVariant {
        path: "crowd/posed/male_sit_hair1.glb",
        seated: true,
    },
    CrowdVariant {
        path: "crowd/posed/male_sit_hair3.glb",
        seated: true,
    },
    CrowdVariant {
        path: "crowd/posed/male_sit_bald.glb",
        seated: true,
    },
    CrowdVariant {
        path: "crowd/posed/male_cheer_hair1.glb",
        seated: true,
    },
    CrowdVariant {
        path: "crowd/posed/male_cheer_hair3.glb",
        seated: true,
    },
    CrowdVariant {
        path: "crowd/posed/male_cheer_bald.glb",
        seated: true,
    },
    CrowdVariant {
        path: "crowd/posed/male_wave_hair1.glb",
        seated: false,
    },
    CrowdVariant {
        path: "crowd/posed/male_wave_hair3.glb",
        seated: false,
    },
    CrowdVariant {
        path: "crowd/posed/male_wave_bald.glb",
        seated: false,
    },
    CrowdVariant {
        path: "crowd/posed/female_sit_hair1.glb",
        seated: true,
    },
    CrowdVariant {
        path: "crowd/posed/female_sit_hair2.glb",
        seated: true,
    },
    CrowdVariant {
        path: "crowd/posed/female_cheer_hair1.glb",
        seated: true,
    },
    CrowdVariant {
        path: "crowd/posed/female_cheer_hair2.glb",
        seated: true,
    },
    CrowdVariant {
        path: "crowd/posed/female_wave_hair1.glb",
        seated: false,
    },
    CrowdVariant {
        path: "crowd/posed/female_wave_hair2.glb",
        seated: false,
    },
];

const SEATED_INDICES: [usize; 10] = [0, 1, 2, 3, 4, 5, 9, 10, 11, 12];
const STANDING_INDICES: [usize; 5] = [6, 7, 8, 13, 14];
const SIT_INDICES: [usize; 5] = [0, 1, 2, 9, 10];
const CHEER_INDICES: [usize; 5] = [3, 4, 5, 11, 12];

pub fn seated_variants() -> &'static [usize] {
    &SEATED_INDICES
}

pub fn standing_variants() -> &'static [usize] {
    &STANDING_INDICES
}

pub fn sit_variants() -> &'static [usize] {
    &SIT_INDICES
}

pub fn cheer_variants() -> &'static [usize] {
    &CHEER_INDICES
}

/// Shared palette handles — one material per colour slot, reused by every spectator.
#[derive(Resource)]
pub struct CrowdPalette {
    pub skin: Vec<Handle<StandardMaterial>>,
    pub shirt: Vec<Handle<StandardMaterial>>,
    pub pants: Vec<Handle<StandardMaterial>>,
    pub shoes: Vec<Handle<StandardMaterial>>,
    pub hair: Vec<Handle<StandardMaterial>>,
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpectatorOutfit {
    pub skin: u8,
    pub shirt: u8,
    pub pants: u8,
    pub shoes: u8,
    pub hair: u8,
}

#[derive(Component)]
pub struct CrowdStyled;

fn crowd_material(
    materials: &mut Assets<StandardMaterial>,
    color: Color,
) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: color,
        perceptual_roughness: 0.95,
        metallic: 0.0,
        reflectance: 0.05,
        ..default()
    })
}

/// Build the shared spectator palette once at app startup.
pub fn build_crowd_palette(materials: &mut Assets<StandardMaterial>) -> CrowdPalette {
    let skin = [
        Color::srgb_u8(0xFF, 0xE0, 0xBD),
        Color::srgb_u8(0xF1, 0xC9, 0x9D),
        Color::srgb_u8(0xE0, 0xAC, 0x69),
        Color::srgb_u8(0xC6, 0x86, 0x42),
        Color::srgb_u8(0x8D, 0x55, 0x2A),
        Color::srgb_u8(0x5C, 0x3A, 0x21),
    ];
    let shirt = [
        Color::srgb_u8(0xE8, 0x2C, 0x2C),
        Color::srgb_u8(0x1E, 0x6F, 0xC4),
        Color::srgb_u8(0x2E, 0x9E, 0x4A),
        Color::srgb_u8(0xF4, 0xA6, 0x12),
        Color::srgb_u8(0x7B, 0x3F, 0xC4),
        Color::srgb_u8(0x00, 0x96, 0x88),
        Color::srgb_u8(0xD4, 0x3F, 0x6A),
        Color::srgb_u8(0xF5, 0xF5, 0xF5),
        Color::srgb_u8(0xB0, 0xB0, 0xB0),
        Color::srgb_u8(0x1A, 0x1A, 0x2E),
    ];
    let pants = [
        Color::srgb_u8(0x3D, 0x5A, 0x80),
        Color::srgb_u8(0x3A, 0x3A, 0x3A),
        Color::srgb_u8(0xA8, 0x9A, 0x6E),
        Color::srgb_u8(0x1C, 0x1C, 0x1C),
        Color::srgb_u8(0x5C, 0x3D, 0x2E),
    ];
    let shoes = [
        Color::srgb_u8(0x2A, 0x2A, 0x2A),
        Color::srgb_u8(0x1A, 0x1A, 0x1A),
        Color::srgb_u8(0x3D, 0x2B, 0x1F),
    ];
    let hair = [
        Color::srgb_u8(0x1A, 0x1A, 0x1A),
        Color::srgb_u8(0x3B, 0x2A, 0x1A),
        Color::srgb_u8(0x6B, 0x4C, 0x2E),
        Color::srgb_u8(0xD4, 0xB8, 0x6A),
        Color::srgb_u8(0x9A, 0x9A, 0x9A),
    ];

    CrowdPalette {
        skin: skin.iter().map(|&c| crowd_material(materials, c)).collect(),
        shirt: shirt
            .iter()
            .map(|&c| crowd_material(materials, c))
            .collect(),
        pants: pants
            .iter()
            .map(|&c| crowd_material(materials, c))
            .collect(),
        shoes: shoes
            .iter()
            .map(|&c| crowd_material(materials, c))
            .collect(),
        hair: hair.iter().map(|&c| crowd_material(materials, c)).collect(),
    }
}

pub fn init_crowd_palette(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.insert_resource(build_crowd_palette(&mut materials));
}

/// Small deterministic integer hash shared with `player.rs` for per-player
/// skin-tone assignment — same mixing, different table.
pub(crate) fn mix_hash(seed: u32, salt: u32) -> u32 {
    let mut h = seed.wrapping_add(salt.wrapping_mul(0x9E37_79B1));
    h ^= h << 13;
    h ^= h >> 17;
    h ^= h << 5;
    h
}

/// Deterministic per-seat seed for outfit, posture, and placement jitter.
pub fn spectator_seed(seg: usize, tier: usize, k: usize) -> u32 {
    (seg as u32)
        .wrapping_mul(0x9E37_79B1)
        .wrapping_add((tier as u32).wrapping_mul(0x85EB_CA6B))
        .wrapping_add((k as u32).wrapping_mul(0xC2B2_AE35))
}

/// Deterministic outfit indices from a seat seed — no `rand`.
pub fn outfit_for(seed: u32) -> SpectatorOutfit {
    SpectatorOutfit {
        skin: (mix_hash(seed, 1) % 6) as u8,
        shirt: (mix_hash(seed, 2) % 10) as u8,
        pants: (mix_hash(seed, 3) % 5) as u8,
        shoes: (mix_hash(seed, 4) % 3) as u8,
        hair: (mix_hash(seed, 5) % 5) as u8,
    }
}

/// Pick a variant table index: ~1/9 standing (`wave`), seated mix ~3/4 sit / ~1/4 cheer.
pub fn variant_index_for_seat(seg: usize, tier: usize, k: usize) -> usize {
    let seed = spectator_seed(seg, tier, k);
    if mix_hash(seed, 6).is_multiple_of(9) {
        let pool = standing_variants();
        pool[(mix_hash(seed, 7) as usize) % pool.len()]
    } else if mix_hash(seed, 8).is_multiple_of(4) {
        let pool = cheer_variants();
        pool[(mix_hash(seed, 9) as usize) % pool.len()]
    } else {
        let pool = sit_variants();
        pool[(mix_hash(seed, 10) as usize) % pool.len()]
    }
}

/// Per-spectator height scale in metres (life-sized GLBs, small jitter only).
pub fn height_scale_for_seat(seg: usize, tier: usize, k: usize) -> f32 {
    let seed = spectator_seed(seg, tier, k);
    let t = (mix_hash(seed, 11) % 1000) as f32 / 1000.0;
    0.95 + t * 0.11
}

/// Small deterministic yaw jitter (±0.12 rad) on top of ring-facing rotation.
pub fn yaw_jitter_for_seat(seg: usize, tier: usize, k: usize) -> f32 {
    let seed = spectator_seed(seg, tier, k);
    let t = (mix_hash(seed, 12) % 1000) as f32 / 1000.0;
    (t - 0.5) * 0.24
}

/// Seated and standing poses both have feet at y ≈ 0 in the GLB; no offset needed.
pub fn posture_y_offset(_variant_idx: usize) -> f32 {
    0.0
}

/// Swap imported glTF slot materials for shared palette handles.
#[allow(clippy::type_complexity)]
pub fn apply_crowd_materials(
    mut commands: Commands,
    palette: Res<CrowdPalette>,
    outfits: Query<&SpectatorOutfit>,
    parents: Query<&ChildOf>,
    meshes: Query<(Entity, &GltfMaterialName), (With<Mesh3d>, Without<CrowdStyled>)>,
) {
    for (entity, mat_name) in &meshes {
        let mut cur = parents.get(entity).ok().map(ChildOf::parent);
        let mut outfit = None;
        for _ in 0..32 {
            let Some(parent) = cur else { break };
            if let Ok(found) = outfits.get(parent) {
                outfit = Some(found);
                break;
            }
            cur = parents.get(parent).ok().map(ChildOf::parent);
        }
        let Some(outfit) = outfit else { continue };

        let handle = match mat_name.as_ref() {
            "Skin" => palette.skin.get(outfit.skin as usize),
            "Shirt" => palette.shirt.get(outfit.shirt as usize),
            "Pants" => palette.pants.get(outfit.pants as usize),
            "Shoes" => palette.shoes.get(outfit.shoes as usize),
            "Hair" => palette.hair.get(outfit.hair as usize),
            _ => {
                commands.entity(entity).insert(CrowdStyled);
                continue;
            }
        };
        let Some(handle) = handle else {
            commands.entity(entity).insert(CrowdStyled);
            continue;
        };
        commands
            .entity(entity)
            .insert((MeshMaterial3d(handle.clone()), CrowdStyled));
    }
}

/// Deepest tier still drawn with glTF figures. The near band is what the
/// broadcast camera actually resolves; beyond it a merged box figure is
/// indistinguishable and costs one entity for the whole deck instead of one each.
const DETAILED_TIERS: usize = 4;
/// How square-on a seat may be and still get a glTF figure, per tier. Behind the
/// arm the camera is closest, so the band reaches further round there.
const DETAILED_AXIS_THRESHOLD: [f32; DETAILED_TIERS] = [0.55, 0.62, 0.72, 0.80];

// --- Seated adult, measured against the seat it has to fit in --------------
// The seat is 0.56 m pitch, 0.46 m deep, backrest crowning 0.88 m over the
// tread. Shoulders 0.46 m across nearly fill the pitch, which is what makes a
// row read as people rather than a picket fence of posts; and the crown of the
// head lands 0.44 m above the backrest so it separates against the seat behind.
/// Shoulder width. A seated adult is close to as wide as the visible torso is
/// tall — the old 0.34-wide, 0.66-tall body read as a bollard at any distance.
/// Standing stature the posed pack is normalised to (male 1.78 m, female
/// 1.66 m — see `assets/crowd/ATTRIBUTION.md`). The merged band's box figure is
/// built to the same height, which is what keeps the two bands' head lines level.
const POSED_STANDING_HEIGHT: f32 = 1.78;

const TORSO_WIDTH: f32 = 0.46;
const TORSO_DEPTH: f32 = 0.26;
/// Pelvis to shoulder, seated.
const SEATED_TORSO_HEIGHT: f32 = 0.56;
/// Standing torsos read a little longer because the spine straightens.
const STANDING_TORSO_HEIGHT: f32 = 0.58;
/// Hip height of a spectator on their feet, above the tread.
const STANDING_HIP: f32 = 0.90;
const HEAD_WIDTH: f32 = 0.21;
const HEAD_HEIGHT: f32 = 0.25;
const HEAD_DEPTH: f32 = 0.22;
/// Shoulder-to-jaw gap that reads as a neck instead of a head bolted to a chest.
const NECK_GAP: f32 = 0.06;
/// Slight forward carry of the head — watching the game, not the sky.
const HEAD_FORWARD: f32 = 0.01;
/// Thighs across the pan. Buttock-to-knee is longer than the 0.46 m pan, so the
/// knees break the seat line, which is the cue that the seat is occupied.
const LAP_WIDTH: f32 = 0.36;
const LAP_HEIGHT: f32 = 0.16;
const LAP_DEPTH: f32 = 0.44;
/// Forward reach of the thigh centre from the pelvis.
const LAP_REACH: f32 = 0.20;
const LAP_LIFT: f32 = 0.04;
/// Legs of a standing spectator, pelvis down to the tread.
const LEG_WIDTH: f32 = 0.30;
const LEG_DEPTH: f32 = 0.28;
/// Boxes per merged figure: legs or lap, torso, head.
const FIGURE_BOXES: usize = 3;

/// Contiguous wedges of the bowl in one team's colours. Eight segments is one
/// full bay between aisles — roughly seventy seats wide and the whole height of
/// the bowl — so a block reads as a block from the far side of the ground
/// instead of as noise.
const SUPPORT_BLOCK_SEGMENTS: usize = 8;
const SUPPORT_BLOCKS: [SupportSection; 12] = [
    SupportSection::Batting,
    SupportSection::Batting,
    SupportSection::Neutral,
    SupportSection::Fielding,
    SupportSection::Fielding,
    SupportSection::Neutral,
    SupportSection::Batting,
    SupportSection::Neutral,
    SupportSection::Fielding,
    SupportSection::Fielding,
    SupportSection::Neutral,
    SupportSection::Batting,
];

/// Share of a partisan block actually in team colours. Even in the loudest end
/// half the block is in ordinary clothes; pushing this higher is what turned the
/// bowl into two flat slabs of coral and teal.
const BLOCK_TEAM_SHARE: f32 = 0.58;
/// Replica shirts scattered through the neutral sections.
const NEUTRAL_TEAM_SHARE: f32 = 0.09;

/// Everyday clothing: muted, varied, and worn by most of the ground. The crowd
/// has to read as a texture of thousands of individuals first and resolve into
/// team blocks only where those blocks are.
const EVERYDAY_TONES: [Color; 12] = [
    Color::srgb_u8(0x3A, 0x3E, 0x46),
    Color::srgb_u8(0x6B, 0x70, 0x78),
    Color::srgb_u8(0x9A, 0x9E, 0xA4),
    Color::srgb_u8(0xC6, 0xC1, 0xB4),
    Color::srgb_u8(0xE0, 0xDB, 0xD0),
    Color::srgb_u8(0x35, 0x4A, 0x63),
    Color::srgb_u8(0x54, 0x6A, 0x5E),
    Color::srgb_u8(0x7C, 0x5F, 0x46),
    Color::srgb_u8(0x8C, 0x4B, 0x42),
    Color::srgb_u8(0x4A, 0x3C, 0x50),
    Color::srgb_u8(0xB0, 0x95, 0x62),
    Color::srgb_u8(0x2C, 0x38, 0x3C),
];

/// Trousers and skirts: darker and duller than shirts, which is what stops the
/// lap row from flaring brighter than the faces above it.
const LEG_TONES: [Color; 5] = [
    Color::srgb_u8(0x2A, 0x2E, 0x36),
    Color::srgb_u8(0x2E, 0x3C, 0x50),
    Color::srgb_u8(0x4C, 0x4A, 0x42),
    Color::srgb_u8(0x60, 0x62, 0x66),
    Color::srgb_u8(0x4A, 0x3A, 0x30),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CrowdBand {
    Detailed,
    LowerMerged,
    UpperMerged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SupportSection {
    Batting,
    Fielding,
    Neutral,
}

/// One occupied seat. Position is not stored: it is always recomputed from the
/// tier's [`sg::SeatGrid`], so there is no second copy of the layout to drift.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Spectator {
    tier: u8,
    seg: u16,
    seat: u8,
    band: CrowdBand,
    section: SupportSection,
    standing: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CrowdLayout {
    spectators: Vec<Spectator>,
    tier_capacity: [usize; TIER_COUNT],
    tier_filled: [usize; TIER_COUNT],
}

impl CrowdLayout {
    fn total_count(&self) -> usize {
        self.spectators.len()
    }

    fn band_count(&self, band: CrowdBand) -> usize {
        self.spectators.iter().filter(|s| s.band == band).count()
    }

    fn detailed_count(&self) -> usize {
        self.band_count(CrowdBand::Detailed)
    }

    fn capacity(&self) -> usize {
        self.tier_capacity.iter().sum()
    }

    fn occupancy_for_tier(&self, tier: usize) -> f32 {
        let cap = self.tier_capacity[tier];
        if cap == 0 {
            0.0
        } else {
            self.tier_filled[tier] as f32 / cap as f32
        }
    }
}

/// Team colours the partisan blocks are painted in.
struct TeamColors {
    batting: Color,
    fielding: Color,
}

/// One spectator's clothing, resolved to merged-mesh vertex colours.
struct SpectatorTones {
    shirt: [f32; 4],
    legs: [f32; 4],
    skin: [f32; 4],
}

/// One spectator's body, anchored on the seat they occupy.
struct SpectatorPose {
    /// Pelvis: on the seat pan, or at standing hip height over the same spot.
    hip: Vec3,
    /// Tread directly under that pelvis, where a figure's feet land.
    foot: Vec3,
    /// Ring angle of the seat, i.e. the direction it faces.
    angle: f32,
    /// Shoulder turn away from dead centre.
    yaw: f32,
    /// Lean, positive toward the pitch.
    lean: f32,
    /// Sideways sway.
    roll: f32,
    /// Stature multiplier on everything above the pelvis.
    scale: f32,
    /// Tread to crown of the head. The glTF band is scaled to this so the two
    /// bands share a head line.
    height: f32,
}

fn crowd_hash_u32(a: u32, b: u32, c: u32, seed: u32) -> u32 {
    let mut n = a
        .wrapping_mul(374_761_393)
        .wrapping_add(b.wrapping_mul(668_265_263))
        .wrapping_add(c.wrapping_mul(2_147_483_647))
        .wrapping_add(seed.wrapping_mul(982_451_653));
    n = (n ^ (n >> 13)).wrapping_mul(1_274_126_177);
    n ^ (n >> 16)
}

fn crowd_hash(a: u32, b: u32, c: u32, seed: u32) -> f32 {
    (crowd_hash_u32(a, b, c, seed) & 0x00FF_FFFF) as f32 / 16_777_215.0
}

/// Per-seat hash, so occupancy, dress and posture are all fixed by seat number.
fn seat_hash(s: Spectator, seed: u32) -> f32 {
    crowd_hash(s.seg as u32, s.tier as u32, s.seat as u32, seed)
}

/// Seat layout of one tier — the same call `stand_geometry` builds seats from.
fn tier_grid(bowl: &BowlLayout, tier: usize) -> sg::SeatGrid {
    sg::SeatGrid::on_tread(bowl.tier_mid_radius(tier), bowl.tread_top(tier))
}

fn tier_grids(bowl: &BowlLayout) -> [sg::SeatGrid; TIER_COUNT] {
    std::array::from_fn(|tier| tier_grid(bowl, tier))
}

fn support_section(seg: usize) -> SupportSection {
    SUPPORT_BLOCKS[(seg / SUPPORT_BLOCK_SEGMENTS) % SUPPORT_BLOCKS.len()]
}

fn crowd_band_for_seat(tier: usize, angle: f32) -> CrowdBand {
    if tier >= LOWER_TIER_COUNT {
        return CrowdBand::UpperMerged;
    }
    let detailed = DETAILED_AXIS_THRESHOLD
        .get(tier)
        .is_some_and(|&th| angle.cos().abs() >= th);
    if detailed {
        CrowdBand::Detailed
    } else {
        CrowdBand::LowerMerged
    }
}

/// Share of a segment's seats that sell for this fixture.
///
/// A well-attended match: the lower deck is nearly full, the upper deck visibly
/// thinner, behind the arm fuller than square, partisan ends fuller than the
/// neutral sections. Empty seats have to stay legible as texture rather than
/// becoming the impression the bowl gives.
fn segment_occupancy(tier: usize, seg: usize, angle: f32) -> f32 {
    let upper = tier >= LOWER_TIER_COUNT;
    let row = if upper { tier - LOWER_TIER_COUNT } else { tier } as f32;
    // (base, per-row decline, behind-the-arm gain, square-of-the-wicket loss,
    //  partisan-block bonus, segment-to-segment noise)
    let (base, decline, axis_gain, square_loss, block, noise) = if upper {
        (0.790, 0.018, 0.070, 0.110, 0.045, 0.060)
    } else {
        (0.905, 0.010, 0.045, 0.055, 0.030, 0.050)
    };

    let axis = angle.cos().abs();
    let mut occ = base - decline * row + axis.powf(1.3) * axis_gain - (1.0 - axis) * square_loss;
    occ += match support_section(seg) {
        SupportSection::Batting => block,
        SupportSection::Fielding => block * 0.7,
        SupportSection::Neutral => -block,
    };
    occ += (crowd_hash(seg as u32, tier as u32, 0, 211) - 0.5) * noise;
    occ.clamp(0.50, 0.98)
}

/// Whether seat `(seg, k)` is sold. The predicate is over the seat grid itself,
/// so occupancy is literally "which seats are filled".
fn seat_is_occupied(tier: usize, seg: usize, k: usize, angle: f32) -> bool {
    crowd_hash(seg as u32, tier as u32, k as u32, 353) < segment_occupancy(tier, seg, angle)
}

/// Share of a section on its feet — cheering, queueing, or just restless.
fn standing_rate(tier: usize, section: SupportSection, angle: f32) -> f32 {
    let mut rate = if tier < 2 {
        0.13
    } else if tier < LOWER_TIER_COUNT {
        0.09
    } else {
        0.06
    };
    rate += angle.cos().abs() * 0.03;
    if !matches!(section, SupportSection::Neutral) {
        rate += 0.03;
    }
    rate.clamp(0.04, 0.24)
}

fn build_crowd_layout(bowl: &BowlLayout) -> CrowdLayout {
    let grids = tier_grids(bowl);
    let mut spectators = Vec::with_capacity(10_000);
    let mut tier_capacity = [0usize; TIER_COUNT];
    let mut tier_filled = [0usize; TIER_COUNT];

    for (tier, grid) in grids.iter().enumerate() {
        tier_capacity[tier] = grid.total_seats();
        for (seg, k) in grid.seats() {
            let angle = grid.seat_angle(seg, k);
            if !seat_is_occupied(tier, seg, k, angle) {
                continue;
            }
            let section = support_section(seg);
            let standing = crowd_hash(seg as u32, tier as u32, k as u32, 907)
                < standing_rate(tier, section, angle);
            spectators.push(Spectator {
                tier: tier as u8,
                seg: seg as u16,
                seat: k as u8,
                band: crowd_band_for_seat(tier, angle),
                section,
                standing,
            });
            tier_filled[tier] += 1;
        }
    }

    CrowdLayout {
        spectators,
        tier_capacity,
        tier_filled,
    }
}

/// Height from the tread to the crown of the head.
///
/// Seated, the pelvis is pinned to the pan whatever the person's stature — a
/// seat height is a seat height — so only the body above it scales.
fn figure_height(grid: sg::SeatGrid, standing: bool, scale: f32) -> f32 {
    let torso = if standing {
        STANDING_TORSO_HEIGHT
    } else {
        SEATED_TORSO_HEIGHT
    };
    let above_hip = (torso + NECK_GAP + HEAD_HEIGHT) * scale;
    if standing {
        STANDING_HIP * scale + above_hip
    } else {
        (grid.hip_height() - grid.tread_top) + above_hip
    }
}

fn spectator_pose(grid: sg::SeatGrid, s: Spectator) -> SpectatorPose {
    let seg = s.seg as usize;
    let k = s.seat as usize;
    let foot = grid.seat_foot(seg, k);
    let scale = 0.94 + seat_hash(s, 3067) * 0.13;
    let hip_y = if s.standing {
        grid.tread_top + STANDING_HIP * scale
    } else {
        grid.hip_height()
    };

    SpectatorPose {
        hip: Vec3::new(foot.x, hip_y, foot.z),
        foot,
        angle: grid.seat_angle(seg, k),
        yaw: (seat_hash(s, 1499) - 0.5) * 0.34,
        // Watching the game leans a crowd very slightly forward; a standing
        // spectator leans further, over the row in front.
        lean: 0.05 + if s.standing { 0.05 } else { 0.0 } + (seat_hash(s, 2089) - 0.5) * 0.10,
        roll: (seat_hash(s, 1289) - 0.5) * 0.10,
        scale,
        height: figure_height(grid, s.standing, scale),
    }
}

/// The boxes a merged figure is built from, as `(offset, half-extent)` pairs in
/// the pelvis frame: `+X` along the row, `+Z` toward the pitch.
fn figure_parts(standing: bool, scale: f32) -> [(Vec3, Vec3); FIGURE_BOXES] {
    let torso_h = if standing {
        STANDING_TORSO_HEIGHT
    } else {
        SEATED_TORSO_HEIGHT
    };
    let (lower_offset, lower_size) = if standing {
        (
            Vec3::new(0.0, -STANDING_HIP * 0.5, 0.02),
            Vec3::new(LEG_WIDTH, STANDING_HIP, LEG_DEPTH),
        )
    } else {
        (
            Vec3::new(0.0, LAP_LIFT, LAP_REACH),
            Vec3::new(LAP_WIDTH, LAP_HEIGHT, LAP_DEPTH),
        )
    };
    [
        (lower_offset * scale, lower_size * 0.5 * scale),
        (
            Vec3::new(0.0, torso_h * 0.5, 0.0) * scale,
            Vec3::new(TORSO_WIDTH, torso_h, TORSO_DEPTH) * 0.5 * scale,
        ),
        (
            Vec3::new(0.0, torso_h + NECK_GAP + HEAD_HEIGHT * 0.5, HEAD_FORWARD) * scale,
            Vec3::new(HEAD_WIDTH, HEAD_HEIGHT, HEAD_DEPTH) * 0.5 * scale,
        ),
    ]
}

/// Pelvis frame of a posed figure: seat-aligned, then turned by its own posture.
fn pelvis_frame(grid: sg::SeatGrid, s: Spectator, pose: &SpectatorPose) -> Transform {
    let seat = grid.seat_frame(s.seg as usize, s.seat as usize);
    Transform::from_translation(pose.hip).with_rotation(
        seat.rotation
            * Quat::from_rotation_y(pose.yaw)
            * Quat::from_rotation_x(pose.lean)
            * Quat::from_rotation_z(pose.roll),
    )
}

fn crowd_fallback_team_colors(ctx: &StadiumBuildCtx<'_>) -> TeamColors {
    let stand = ctx.stadium.stand_color.to_srgba();
    let outfield = ctx.outfield_base.to_srgba();

    // Synthesise two clearly-separated fan blocks from stadium palette cues.
    TeamColors {
        batting: Color::srgb(
            (stand.red * 0.45 + 0.55).clamp(0.0, 1.0),
            (stand.green * 0.20 + 0.12).clamp(0.0, 1.0),
            (stand.blue * 0.15 + 0.10).clamp(0.0, 1.0),
        ),
        fielding: Color::srgb(
            (outfield.red * 0.28 + 0.11).clamp(0.0, 1.0),
            (outfield.green * 0.52 + 0.20).clamp(0.0, 1.0),
            (outfield.blue * 0.34 + 0.48).clamp(0.0, 1.0),
        ),
    }
}

/// Shirt colour for one spectator.
///
/// Team colours are concentrated: inside a partisan block a good half of the
/// seats wear them, elsewhere only a scattering does. Everything else comes from
/// [`EVERYDAY_TONES`], which is what keeps the bowl from collapsing into two
/// saturated slabs.
fn spectator_shirt(s: Spectator, palette: &TeamColors) -> Color {
    let everyday = EVERYDAY_TONES[crowd_hash_u32(s.seg as u32, s.tier as u32, s.seat as u32, 8089)
        as usize
        % EVERYDAY_TONES.len()];
    let pick = seat_hash(s, 4153);
    match s.section {
        SupportSection::Batting => partisan_shirt(palette.batting, everyday, pick),
        SupportSection::Fielding => partisan_shirt(palette.fielding, everyday, pick),
        SupportSection::Neutral => {
            if pick < NEUTRAL_TEAM_SHARE {
                palette.batting
            } else if pick < NEUTRAL_TEAM_SHARE * 2.0 {
                palette.fielding
            } else {
                everyday
            }
        }
    }
}

/// Replica shirt, half-and-half scarf-over-jacket, or ordinary clothes.
fn partisan_shirt(team: Color, everyday: Color, pick: f32) -> Color {
    if pick < BLOCK_TEAM_SHARE {
        team
    } else if pick < BLOCK_TEAM_SHARE + 0.14 {
        lerp_color(team, everyday, 0.55)
    } else {
        everyday
    }
}

fn crowd_skin_color(idx: usize) -> Color {
    const SKIN_TONES: [Color; 6] = [
        Color::srgb_u8(0xF2, 0xD0, 0xB4),
        Color::srgb_u8(0xDF, 0xB2, 0x8E),
        Color::srgb_u8(0xBF, 0x8A, 0x66),
        Color::srgb_u8(0x9A, 0x67, 0x45),
        Color::srgb_u8(0x76, 0x4E, 0x37),
        Color::srgb_u8(0x58, 0x39, 0x28),
    ];
    SKIN_TONES[idx % SKIN_TONES.len()]
}

fn spectator_tones(s: Spectator, palette: &TeamColors) -> SpectatorTones {
    let skin =
        crowd_skin_color(crowd_hash_u32(s.seg as u32, s.tier as u32, s.seat as u32, 1129) as usize);
    let legs = LEG_TONES[crowd_hash_u32(s.seg as u32, s.tier as u32, s.seat as u32, 6763) as usize
        % LEG_TONES.len()];
    SpectatorTones {
        shirt: shaded(spectator_shirt(s, palette), seat_hash(s, 4931)),
        legs: shaded(legs, seat_hash(s, 5197)),
        skin: shaded(skin, seat_hash(s, 5381) * 0.5),
    }
}

/// Per-person brightness jitter. Two people in the same shirt never catch the
/// light the same way, and without this a block reads as one flat slab.
fn shaded(color: Color, jitter: f32) -> [f32; 4] {
    let c = color.to_srgba();
    let k = 0.86 + jitter * 0.26;
    [
        (c.red * k).clamp(0.0, 1.0),
        (c.green * k).clamp(0.0, 1.0),
        (c.blue * k).clamp(0.0, 1.0),
        1.0,
    ]
}

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    let sa = a.to_srgba();
    let sb = b.to_srgba();
    let k = t.clamp(0.0, 1.0);
    Color::srgb(
        sa.red + (sb.red - sa.red) * k,
        sa.green + (sb.green - sa.green) * k,
        sa.blue + (sb.blue - sa.blue) * k,
    )
}

/// Posed variants suitable for a given posture.
///
/// The pack ships sit, cheer and wave poses; the first two are authored seated
/// and the third standing, so the layout's own standing flag picks the set
/// rather than a hash that knows nothing about the seat.
fn posed_variant_index(s: Spectator) -> usize {
    let pool = if s.standing {
        standing_variants()
    } else {
        seated_variants()
    };
    pool[crowd_hash_u32(s.seg as u32, s.tier as u32, s.seat as u32, 1811) as usize % pool.len()]
}

fn crowd_variant_handles(ctx: &StadiumBuildCtx<'_>) -> Vec<Handle<Scene>> {
    CROWD_VARIANTS
        .iter()
        .map(|v| {
            ctx.asset_server
                .load(GltfAssetLabel::Scene(0).from_asset(v.path))
        })
        .collect()
}

fn spawn_detailed_crowd(
    p: &mut ChildSpawnerCommands,
    ctx: &StadiumBuildCtx<'_>,
    layout: &CrowdLayout,
    crowd_variants: &[Handle<Scene>],
    spawn_count: &mut usize,
) {
    let grids = tier_grids(&ctx.bowl);
    for s in layout
        .spectators
        .iter()
        .copied()
        .filter(|s| s.band == CrowdBand::Detailed)
    {
        let grid = grids[s.tier as usize];
        let pose = spectator_pose(grid, s);
        // The face-centre frame looks *inward* along its own `-Z`, so a forward
        // lean is a negative rotation about the tangent here.
        let rot = ring_face_center_rotation(pose.angle)
            * Quat::from_rotation_y(pose.yaw)
            * Quat::from_rotation_z(pose.roll)
            * Quat::from_rotation_x(-pose.lean);
        let variant = posed_variant_index(s);
        p.spawn((
            SceneRoot(crowd_variants[variant].clone()),
            // The outfit rides on the root; `apply_crowd_materials` walks down
            // to the glTF slots and swaps in the shared palette handles.
            outfit_for(spectator_seed(
                s.seg as usize,
                s.tier as usize,
                s.seat as usize,
            )),
            // These models are authored at life size, so the seat's own stature
            // is a scale about 1.0 rather than a division by a rig height.
            Transform::from_translation(pose.foot + Vec3::Y * posture_y_offset(variant))
                .with_rotation(rot)
                .with_scale(Vec3::splat(pose.scale)),
            Visibility::default(),
            InheritedVisibility::default(),
            ViewVisibility::default(),
        ));
        track_spawn(spawn_count);
    }
}

fn spawn_merged_crowd_band(
    p: &mut ChildSpawnerCommands,
    ctx: &mut StadiumBuildCtx<'_>,
    layout: &CrowdLayout,
    band: CrowdBand,
    palette: &TeamColors,
    spawn_count: &mut usize,
) {
    let mesh = build_merged_crowd_mesh(&ctx.bowl, layout, band, palette);
    if sg::mesh_is_empty(&mesh) {
        return;
    }

    let merged_mesh = ctx.meshes.add(mesh);
    let merged_mat = ctx.materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.96,
        reflectance: 0.08,
        cull_mode: None,
        ..default()
    });
    p.spawn((
        Mesh3d(merged_mesh),
        MeshMaterial3d(merged_mat),
        Transform::default(),
    ));
    track_spawn(spawn_count);
}

/// Every spectator in one band as a single vertex-coloured mesh. Bevy exposes no
/// instancing here and the deck holds thousands of people, so the whole band is
/// one draw with the clothing riding on [`Mesh::ATTRIBUTE_COLOR`].
fn build_merged_crowd_mesh(
    bowl: &BowlLayout,
    layout: &CrowdLayout,
    band: CrowdBand,
    palette: &TeamColors,
) -> Mesh {
    let grids = tier_grids(bowl);
    // Five faces per box (nothing is ever seen from underneath), four vertices
    // per face.
    let mut m = sg::StandMesh::with_capacity(layout.band_count(band) * FIGURE_BOXES * 20);

    for s in layout.spectators.iter().copied().filter(|s| s.band == band) {
        let grid = grids[s.tier as usize];
        let pose = spectator_pose(grid, s);
        let frame = pelvis_frame(grid, s, &pose);
        let tones = spectator_tones(s, palette);
        let colors = [tones.legs, tones.shirt, tones.skin];
        for ((offset, half), color) in figure_parts(s.standing, pose.scale).into_iter().zip(colors)
        {
            m.push_box_open_bottom(frame * Transform::from_translation(offset), half, color);
        }
    }
    m.build()
}

pub(crate) fn spawn_crowd(
    p: &mut ChildSpawnerCommands,
    ctx: &mut StadiumBuildCtx<'_>,
    spawn_count: &mut usize,
) -> usize {
    let palette = crowd_fallback_team_colors(ctx);
    spawn_crowd_with_palette(p, ctx, spawn_count, &palette)
}

/// Alternate entry point so stadium assembly can pass real team colours later
/// without changing crowd layout maths.
pub(crate) fn spawn_crowd_with_team_colors(
    p: &mut ChildSpawnerCommands,
    ctx: &mut StadiumBuildCtx<'_>,
    spawn_count: &mut usize,
    batting_color: Color,
    fielding_color: Color,
) -> usize {
    let palette = TeamColors {
        batting: batting_color,
        fielding: fielding_color,
    };
    spawn_crowd_with_palette(p, ctx, spawn_count, &palette)
}

fn spawn_crowd_with_palette(
    p: &mut ChildSpawnerCommands,
    ctx: &mut StadiumBuildCtx<'_>,
    spawn_count: &mut usize,
    palette: &TeamColors,
) -> usize {
    let layout = build_crowd_layout(&ctx.bowl);
    let crowd_variants = crowd_variant_handles(ctx);

    spawn_detailed_crowd(p, ctx, &layout, &crowd_variants, spawn_count);
    for band in [CrowdBand::LowerMerged, CrowdBand::UpperMerged] {
        spawn_merged_crowd_band(p, ctx, &layout, band, palette, spawn_count);
    }

    layout.total_count()
}

/// Expected crowd count for a standard bowl (used by tests and capacity checks).
pub fn expected_crowd_count() -> usize {
    let bowl = BowlLayout::from_boundary(geometry::DEFAULT_BOUNDARY_RADIUS);
    build_crowd_layout(&bowl).total_count()
}

#[cfg(test)]
mod tests {
    use bevy::camera::primitives::MeshAabb;

    use super::*;

    fn standard_bowl() -> BowlLayout {
        BowlLayout::from_boundary(geometry::DEFAULT_BOUNDARY_RADIUS)
    }

    fn standard_layout() -> CrowdLayout {
        build_crowd_layout(&standard_bowl())
    }

    fn test_palette() -> TeamColors {
        TeamColors {
            batting: Color::srgb(0.80, 0.18, 0.16),
            fielding: Color::srgb(0.16, 0.34, 0.68),
        }
    }

    #[test]
    fn crowd_reads_as_a_well_attended_match() {
        let layout = standard_layout();
        let n = layout.total_count();
        assert_eq!(n, expected_crowd_count());
        // Bounded by the seats that exist: the bowl seats a little over 10,000.
        assert!(n >= 7_000, "crowd too sparse: {n}");
        assert!(n <= 9_800, "crowd too dense: {n}");
        let filled = n as f32 / layout.capacity() as f32;
        assert!(
            (0.74..0.92).contains(&filled),
            "bowl {:.1}% full does not read as a well-attended match",
            filled * 100.0
        );
    }

    #[test]
    fn detailed_band_stays_within_budget() {
        let layout = standard_layout();
        let n = layout.detailed_count();
        // Every one of these is a `SceneRoot`; the merged bands cost two entities
        // between them, so this is where the crowd's entity budget goes.
        assert!(n >= 900, "detailed crowd too sparse: {n}");
        assert!(n <= 2_400, "detailed crowd too dense: {n}");
        assert!(
            layout
                .spectators
                .iter()
                .all(|s| s.band != CrowdBand::Detailed || (s.tier as usize) < DETAILED_TIERS),
            "glTF figures escaped the near band"
        );
    }

    /// The invariant the two-grid bug broke: a spectator's body is anchored on a
    /// seat position, not near one.
    #[test]
    fn every_spectator_sits_in_a_seat() {
        let bowl = standard_bowl();
        let layout = standard_layout();
        let mut checked = [0usize; TIER_COUNT];

        for s in layout.spectators.iter().copied() {
            let tier = s.tier as usize;
            let grid = tier_grid(&bowl, tier);
            let seat = grid.seat_hip(s.seg as usize, s.seat as usize);
            let pose = spectator_pose(grid, s);

            // Same seat on the ground plan, whether sitting or standing in it.
            let drift = Vec2::new(pose.hip.x - seat.x, pose.hip.z - seat.z).length();
            assert!(
                drift < 1e-4,
                "tier {tier} seg {} seat {} is {drift} m off its seat",
                s.seg,
                s.seat
            );
            assert!(pose.foot.y >= grid.tread_top - 1e-4);
            if s.standing {
                assert!(pose.hip.y > seat.y, "standing hip should be above the pan");
            } else {
                assert!(
                    (pose.hip.y - seat.y).abs() < 1e-4,
                    "seated pelvis at {} is not on the pan at {}",
                    pose.hip.y,
                    seat.y
                );
                assert!(
                    (pose.hip.y - grid.hip_height()).abs() < 1e-4,
                    "seated pelvis left the pan height"
                );
            }
            // Head clear of the backrest crown, in both bands.
            let crown = grid.tread_top + pose.height;
            assert!(
                crown > grid.backrest_top() + 0.2,
                "tier {tier} head at {crown} is buried behind a backrest topping out at {}",
                grid.backrest_top()
            );
            checked[tier] += 1;
        }

        for (tier, n) in checked.iter().enumerate() {
            assert!(*n > 0, "tier {tier} contributed no spectators to check");
        }
    }

    #[test]
    fn aisles_and_vomitory_mouths_stay_clear() {
        let bowl = standard_bowl();
        let layout = standard_layout();
        let voms = sg::vomitory_angles(sg::SEAT_SEGMENTS, sg::SEAT_AISLE_EVERY, 2);
        let half_segment = std::f32::consts::TAU / sg::SEAT_SEGMENTS as f32 * 0.5;

        for s in &layout.spectators {
            let tier = s.tier as usize;
            let grid = tier_grid(&bowl, tier);
            assert!(
                !grid.is_aisle(s.seg as usize),
                "spectator standing in the stair aisle at segment {}",
                s.seg
            );
            // Vomitories are cut on aisle centres, so skipping the aisle segments
            // is what keeps the tunnel mouths from being walled up with people.
            let angle = grid.seat_angle(s.seg as usize, s.seat as usize);
            for v in &voms {
                assert!(
                    (angle - v).abs() > half_segment * 0.5,
                    "spectator at {angle} is inside the vomitory mouth at {v}"
                );
            }
        }
    }

    #[test]
    fn all_tiers_receive_crowd() {
        let layout = standard_layout();
        for tier in 0..TIER_COUNT {
            assert!(
                layout.tier_filled[tier] > 400,
                "tier {tier} too empty: {}",
                layout.tier_filled[tier]
            );
        }
    }

    #[test]
    fn occupancy_profile_is_plausible() {
        let layout = standard_layout();

        let mut lower_sum = 0.0;
        let mut upper_sum = 0.0;
        for tier in 0..TIER_COUNT {
            let occ = layout.occupancy_for_tier(tier);
            if tier < LOWER_TIER_COUNT {
                assert!(
                    (0.80..=0.95).contains(&occ),
                    "lower tier {tier} at {occ:.3} is not a well-attended lower deck"
                );
                lower_sum += occ;
            } else {
                assert!(
                    (0.60..=0.85).contains(&occ),
                    "upper tier {tier} at {occ:.3} is out of range"
                );
                upper_sum += occ;
            }
        }

        let lower_avg = lower_sum / LOWER_TIER_COUNT as f32;
        let upper_avg = upper_sum / (TIER_COUNT - LOWER_TIER_COUNT) as f32;
        assert!(
            lower_avg > upper_avg + 0.07,
            "lower deck should read denser (lower={lower_avg:.3}, upper={upper_avg:.3})"
        );
    }

    #[test]
    fn behind_the_arm_fills_before_square_of_the_wicket() {
        // The pitch runs along X, so `cos(angle) == ±1` is behind the arm.
        let behind = segment_occupancy(0, 4, 0.0);
        let square = segment_occupancy(0, 4, std::f32::consts::FRAC_PI_2);
        assert!(
            behind > square + 0.05,
            "behind the arm {behind:.3} vs square {square:.3}"
        );
    }

    #[test]
    fn partisan_blocks_are_contiguous_wedges() {
        // A block must not break up inside itself, or team colour reads as noise.
        for seg in 0..sg::SEAT_SEGMENTS {
            if !seg.is_multiple_of(SUPPORT_BLOCK_SEGMENTS) {
                assert_eq!(support_section(seg), support_section(seg - 1));
            }
        }
        let blocks = sg::SEAT_SEGMENTS / SUPPORT_BLOCK_SEGMENTS;
        assert_eq!(blocks, SUPPORT_BLOCKS.len());
        for section in [
            SupportSection::Batting,
            SupportSection::Fielding,
            SupportSection::Neutral,
        ] {
            let n = (0..sg::SEAT_SEGMENTS)
                .filter(|&s| support_section(s) == section)
                .count();
            assert!(n > 0, "{section:?} has no block at all");
            assert!(
                n < sg::SEAT_SEGMENTS / 2,
                "{section:?} swamps the bowl: {n} segments"
            );
        }
    }

    #[test]
    fn team_colour_concentrates_into_the_blocks() {
        let palette = test_palette();
        let layout = standard_layout();
        let is_team = |c: Color| {
            let s = c.to_srgba();
            let b = palette.batting.to_srgba();
            let f = palette.fielding.to_srgba();
            let near = |t: bevy::color::Srgba| {
                (s.red - t.red).abs() < 1e-5
                    && (s.green - t.green).abs() < 1e-5
                    && (s.blue - t.blue).abs() < 1e-5
            };
            near(b) || near(f)
        };

        let mut partisan = (0usize, 0usize);
        let mut neutral = (0usize, 0usize);
        for s in layout.spectators.iter().copied() {
            let counts = if matches!(s.section, SupportSection::Neutral) {
                &mut neutral
            } else {
                &mut partisan
            };
            counts.1 += 1;
            if is_team(spectator_shirt(s, &palette)) {
                counts.0 += 1;
            }
        }

        let block_share = partisan.0 as f32 / partisan.1 as f32;
        let loose_share = neutral.0 as f32 / neutral.1 as f32;
        assert!(
            (0.50..0.70).contains(&block_share),
            "partisan blocks should be about half replica shirts, got {block_share:.3}"
        );
        assert!(
            loose_share < 0.25,
            "team colours are sprayed across the neutral sections: {loose_share:.3}"
        );
        assert!(block_share > loose_share * 2.0);
    }

    #[test]
    fn figures_read_as_people_not_posts() {
        let grid = tier_grid(&standard_bowl(), 0);
        let seated = figure_parts(false, 1.0);
        let (_, torso) = seated[1];
        // Shoulders nearly fill the seat pitch and are wider than the torso is
        // deep — the silhouette cue that separates a person from a bollard.
        assert!(torso.x * 2.0 > grid.seat_pitch() * 0.75);
        assert!(torso.x * 2.0 < grid.seat_pitch() * 1.05);
        assert!(torso.x > torso.z * 1.5);

        let (head_offset, head) = seated[2];
        assert!(head.x * 2.0 > 0.18, "head too small to read at distance");
        // The crown clears the backrest, and the head is smaller than the chest.
        let crown = grid.hip_height() + head_offset.y + head.y;
        assert!(
            crown > grid.backrest_top() + 0.35,
            "head crown {crown} barely clears the backrest at {}",
            grid.backrest_top()
        );
        assert!(head.x < torso.x * 0.6);

        // Knees break the front edge of the pan, so an occupied seat reads as
        // occupied even when the body behind it is hidden.
        let (lap_offset, lap) = seated[0];
        let knee_z = grid.hip_offset().z + lap_offset.z + lap.z;
        assert!(
            knee_z > 0.15 + 0.23,
            "knees at {knee_z} stop short of the pan's front edge"
        );
    }

    #[test]
    fn seated_and_standing_figures_share_a_stature_model() {
        let grid = tier_grid(&standard_bowl(), 2);
        let seated = figure_height(grid, false, 1.0);
        let standing = figure_height(grid, true, 1.0);
        assert!(
            (1.25..1.40).contains(&seated),
            "seated figure {seated} m tall over the tread"
        );
        assert!(
            (1.70..1.90).contains(&standing),
            "standing figure {standing} m tall"
        );
        // The posed models are life-size, so the glTF band's scale is a stature
        // ratio about 1.0 — anything far from it means the two bands disagree.
        assert!((0.95..1.07).contains(&(standing / POSED_STANDING_HEIGHT)));
        // Taller people are taller, and only above the pelvis when seated.
        assert!(figure_height(grid, false, 1.07) > seated);
        assert!(figure_height(grid, false, 1.07) - seated < 0.07);
    }

    #[test]
    fn merged_band_meshes_sit_on_their_decks() {
        let bowl = standard_bowl();
        let layout = standard_layout();
        let palette = test_palette();

        for (band, first_tier) in [
            (CrowdBand::LowerMerged, 0),
            (CrowdBand::UpperMerged, LOWER_TIER_COUNT),
        ] {
            let mesh = build_merged_crowd_mesh(&bowl, &layout, band, &palette);
            assert!(!sg::mesh_is_empty(&mesh));
            let verts = mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap().len();
            assert_eq!(verts, layout.band_count(band) * FIGURE_BOXES * 20);
            assert!(mesh.attribute(Mesh::ATTRIBUTE_COLOR).is_some());

            let aabb = mesh.compute_aabb().unwrap();
            let low = aabb.center.y - aabb.half_extents.y;
            let deck = bowl.tread_top(first_tier);
            assert!(
                low > deck - 0.35 && low < deck + 0.6,
                "{band:?} band starts at {low}, not on its deck at {deck}"
            );
            // Nobody hangs off the outside of the bowl.
            let reach = aabb.half_extents.x.max(aabb.half_extents.z);
            assert!(reach < bowl.outer_radius(), "band reaches {reach}");
        }
    }

    #[test]
    fn crowd_layout_is_deterministic() {
        let a = standard_layout();
        let b = standard_layout();
        assert_eq!(a, b);

        let bowl = standard_bowl();
        let grid = tier_grid(&bowl, 5);
        let s = a.spectators[a.spectators.len() / 2];
        let first = spectator_pose(grid, s);
        let again = spectator_pose(grid, s);
        assert_eq!(first.hip, again.hip);
        assert_eq!(first.height, again.height);
    }

    use std::collections::HashSet;
    use std::path::Path;

    #[test]
    fn outfit_for_indices_in_range_and_cover_palette() {
        let mut skin_used = HashSet::new();
        let mut shirt_used = HashSet::new();
        let mut pants_used = HashSet::new();
        let mut shoes_used = HashSet::new();
        let mut hair_used = HashSet::new();

        for seed in 0..5000_u32 {
            let o = outfit_for(seed);
            assert!(o.skin < 6);
            assert!(o.shirt < 10);
            assert!(o.pants < 5);
            assert!(o.shoes < 3);
            assert!(o.hair < 5);
            skin_used.insert(o.skin);
            shirt_used.insert(o.shirt);
            pants_used.insert(o.pants);
            shoes_used.insert(o.shoes);
            hair_used.insert(o.hair);
        }

        assert_eq!(skin_used.len(), 6);
        assert_eq!(shirt_used.len(), 10);
        assert_eq!(pants_used.len(), 5);
        assert_eq!(shoes_used.len(), 3);
        assert_eq!(hair_used.len(), 5);
    }

    #[test]
    fn adjacent_seeds_do_not_share_outfits() {
        for seed in 0..2000_u32 {
            assert_ne!(outfit_for(seed), outfit_for(seed + 1));
        }
    }

    #[test]
    fn variant_table_size_posture_and_paths() {
        assert_eq!(CROWD_VARIANTS.len(), 15);
        let standing = CROWD_VARIANTS.iter().filter(|v| !v.seated).count();
        assert_eq!(standing, 5);
        assert_eq!(standing_variants().len(), standing);
        assert_eq!(seated_variants().len(), 10);

        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        for variant in &CROWD_VARIANTS {
            let path = manifest_dir.join("assets").join(variant.path);
            assert!(path.is_file(), "missing crowd asset: {}", path.display());
        }
    }

    #[test]
    fn posture_index_tables_match_variant_paths_and_seated_flags() {
        for &idx in sit_variants() {
            let v = &CROWD_VARIANTS[idx];
            assert!(v.path.contains("_sit_"), "sit index {idx}: {}", v.path);
            assert!(v.seated, "sit index {idx} must be seated");
        }
        for &idx in cheer_variants() {
            let v = &CROWD_VARIANTS[idx];
            assert!(v.path.contains("_cheer_"), "cheer index {idx}: {}", v.path);
            assert!(v.seated, "cheer index {idx} must be seated");
        }
        for &idx in standing_variants() {
            let v = &CROWD_VARIANTS[idx];
            assert!(v.path.contains("_wave_"), "wave index {idx}: {}", v.path);
            assert!(!v.seated, "wave index {idx} must be standing");
        }
        for &idx in seated_variants() {
            assert!(
                CROWD_VARIANTS[idx].seated,
                "seated index {idx} has seated=false"
            );
        }
    }
}

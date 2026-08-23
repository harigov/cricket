//! Stadium spectator appearance: posed Quaternius GLB variants, shared colour
//! palette, and per-spectator outfit indices applied after scene load.

use bevy::gltf::GltfMaterialName;
use bevy::prelude::*;

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

fn mix_hash(seed: u32, salt: u32) -> u32 {
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

#[cfg(test)]
mod tests {
    use super::*;
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

# Crowd Asset Attribution

## Quaternius — Background Posed Humans

- **Pack:** Background Posed Humans
- **Author:** [Quaternius](https://quaternius.com/)
- **License:** [CC0 1.0 Universal](https://creativecommons.org/publicdomain/zero/1.0/)
- **Source:** https://quaternius.com/packs/backgroundposedhumans.html
- **Credit line:** Background characters by Quaternius

These posed-human GLBs replace the earlier Kenney blocky crowd models for stadium
spectators. The pack is intended for **distant spectators**, not close-up broadcast shots.

### Generated files (`assets/crowd/posed/`)

- `female_cheer_hair1.glb`
- `female_cheer_hair2.glb`
- `female_sit_hair1.glb`
- `female_sit_hair2.glb`
- `female_wave_hair1.glb`
- `female_wave_hair2.glb`
- `male_cheer_bald.glb`
- `male_cheer_hair1.glb`
- `male_cheer_hair3.glb`
- `male_sit_bald.glb`
- `male_sit_hair1.glb`
- `male_sit_hair3.glb`
- `male_wave_bald.glb`
- `male_wave_hair1.glb`
- `male_wave_hair3.glb`

### Regeneration

```bash
python3 scripts/build_crowd_assets.py
```

Downloads source OBJ/MTL from the Quaternius pack (cached under `target/crowd-src/`),
fits hairstyles via rigid head alignment, normalises standing height (male 1.78 m,
female 1.66 m), and converts merged meshes to GLB with `npx -y obj2gltf@3`.


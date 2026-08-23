# Crowd Asset Attribution

- **Blocky Characters** by **Kenney** – https://kenney.nl/assets/blocky-characters
  - License: **CC0 1.0 Universal** (https://creativecommons.org/publicdomain/zero/1.0/)
  - Files: `crowd-a.glb` … `crowd-d.glb` (4 of 20 variants, each ~111KB, from `kenney_blocky-characters_20.zip`)
  - Used to replace procedural cuboid crowd blobs in `src/render/stadium.rs` with ~120 low-poly humans (1k tris each, PBR).
  - CC0 is public domain, fully MIT-compatible, no attribution required (provided here for completeness).

Extracted via `unzip -j Models/GLB\ format/character-*.glb` and renamed to `crowd-*.glb`.

## Expanded variant set (2026-08)

- Crowd variants extended from 4 to 14 (`crowd-a.glb` … `crowd-n.glb`), taken from the same
  **Blocky Characters** CC0 pack (`character-a` … `character-n`), with their matching
  `Textures/texture-*.png` colour maps. More variants means far less visible repetition
  across a full-capacity bowl.

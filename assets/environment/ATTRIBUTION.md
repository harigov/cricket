# Environment Asset Attribution

All models here are **CC0 1.0 Universal** (public domain) — MIT-compatible, no attribution
required. Credit is recorded anyway.

- **City Kit (Commercial)** by **Kenney** – https://kenney.nl/assets/city-kit-commercial
  - License: **CC0 1.0** (https://creativecommons.org/publicdomain/zero/1.0/)
  - Files: `city/skyscraper-a..e.glb` (from `building-skyscraper-*`), `city/tower-*.glb`
    (from `building-*`), `city/block-*.glb` (from `low-detail-building-*`, used as the
    distance LOD), plus the shared `city/Textures/colormap.png`.
  - Used by the `Metropolis` environment theme in `src/render/environment.rs`.

- **City Kit (Suburban)** by **Kenney** – https://kenney.nl/assets/city-kit-suburban
  - License: **CC0 1.0**
  - Files: `suburb/house-*.glb` (from `building-type-*`), `suburb/Textures/colormap.png`.
  - Used by the `Parkland` theme and by the low-rise outskirts of `Metropolis`.

- **Nature Kit** by **Kenney** – https://kenney.nl/assets/nature-kit
  - License: **CC0 1.0**
  - Files: `nature/palm-*.glb`, `nature/pine-*.glb`, `nature/tree-*.glb`,
    `nature/rock-*.glb`, `nature/stone-large.glb`, `nature/bush-*.glb`,
    `nature/cliff-large.glb` (renamed from `tree_palm*`, `tree_pine*`, `tree_*`,
    `rock_*`, `stone_*`, `plant_bush*`, `cliff_large_rock`).
  - Used by the `Coastal`, `Alpine` and `Parkland` themes.

Geometry is unmodified; models are only scaled, rotated and placed. The `Textures/`
subfolders must stay beside their `.glb` files — the glTF files reference them by
relative URI (`Textures/colormap.png`).

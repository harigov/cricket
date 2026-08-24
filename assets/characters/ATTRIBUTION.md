# Character Asset Attribution

## MakeHuman / MPFB2 — Player Characters

- **Generator:** [MPFB2](https://static.makehumancommunity.org/mpfb.html) 2.0.17
  (MakeHuman Plugin for Blender), installed from the Blender extensions registry
- **Addon licence:** GPL-3.0-or-later — this is the *tool*, and it is not
  redistributed here
- **Asset licence:** [CC0 1.0 Universal](https://creativecommons.org/publicdomain/zero/1.0/)
  — MakeHuman's base mesh, morph targets and rigs are all released CC0, and so
  is anything generated from them. No attribution is legally required; this file
  records provenance anyway.
- **Source:** https://static.makehumancommunity.org/about/license.html

The generated characters in `assets/characters/players/` contain **no**
third-party MakeHuman community assets (no downloaded skins, clothes or hair
packs). Everything is derived from the CC0 core plus geometry this repository
generates itself, so the whole set is unambiguously redistributable under the
project's MIT licence.

> If you later add MakeHuman community skins or clothes, check each asset's own
> licence — the CC0 grant covers the core only.

### Generated files (`assets/characters/players/`)

One GLB per body archetype, named `<height>_<build>_<ancestry>.glb`:

- **height** — `short` / `medium` / `tall`
- **build** — `thin` / `regular` / `heavy`
- **ancestry** — `caucasian` / `south_asian` / `african`

MakeHuman ships only asian/caucasian/african morph axes, so `south_asian` is a
weighted blend of those. Ancestry here drives **body and facial proportion
only** — visible skin tone is applied at runtime from the Rust palette, so any
archetype can wear any skin tone.

### Asset contract

Everything downstream in `src/render/` depends on these properties:

- **Scene root node name** — must be exactly `Armature`, matching `Xbot.glb`.
  Bevy's glTF loader derives each animation target's id from the node-name path
  starting at the scene root, *including the root's own name* (`collect_path` in
  `bevy_gltf`). The bundled idle/run clips come from Xbot, so any other root
  name silently fails to bind every channel and figures freeze in bind pose
  with their arms out. This is not cosmetic — do not rename it.
- **A baked `RestPose` action** — every archetype must contain at least one
  animation of its own, even though it is never played. Bevy's glTF loader only
  attaches `AnimationPlayer` and `AnimationTargetId` to nodes that are
  animation roots *in the file being loaded*, and `attach_animation_players`
  keys off `Added<AnimationPlayer>`. Export an archetype with no animations and
  the shared Xbot idle/run clips have nothing to bind to: fielders, bowler and
  keeper all freeze in bind pose. Batters are unaffected, since their stance is
  procedural — which makes this fail in a deceptively partial way.
- **Skeleton** — MPFB's built-in Mixamo rig, 52 bones, `mixamorig:`-prefixed.
  Contains every bone `bone_kind_for_name` in `src/render/player.rs` matches, so
  the procedural quaternion animation drives these characters unchanged.
- **Rest pose** — T-pose, arms level to within 0.1°, matching the convention the
  pose deltas in `arms_bind_neutral` were authored against.
- **Scale** — metres, armature root at scale 1.0, so bone-local units *are*
  metres (`BONE_UNITS_PER_METRE = 1.0`). This differs from the legacy
  `Xbot.glb`, whose armature is scaled 0.01 with centimetre bone translations.
  The Xbot-authored clips still play correctly because
  `strip_skeleton_root_motion` overwrites the hip translation track every
  frame, leaving only rotations — which retarget cleanly across the two scales.
- **Ground plane** — every mesh sits at `y >= -0.004 m` (shoe soles are clamped
  flat at build time), so the runtime applies no ground offset
  (`SCENE_GROUND_Y = 0.0`).
- **Facing** — +Z, same as `Xbot.glb` (`MODEL_FORWARD_XZ`).
- **Meshes and material slots** — five skinned meshes on one armature, each
  mesh name paired with its material slot name: `Body`/`Skin`, `Shirt`/`Shirt`,
  `Pants`/`Pants`, `Shoes`/`Shoes`, `Hair`/`Hair`. The runtime recolours by
  matching the **material slot name**, so those names are load-bearing — the
  build script clears orphaned datablocks between archetypes precisely so
  Blender cannot silently uniquify them to `Skin.001`.
- **Shirt UVs** — cylindrical, seam under the left arm:
  `u = 0.25` back centre, `u = 0.75` front centre, `v = 0` shoulder line,
  `v = 1` hem. The texture tiles horizontally. See `src/render/kit.rs`.

Garment shells are derived from the body's own bone weight groups, so they
inherit its skinning exactly and need no weight transfer.

### Regeneration

```bash
scripts/build_player_asset.py --install   # once: fetch MPFB into target/mpfb/
scripts/build_player_asset.py --list      # show the archetype matrix
scripts/build_player_asset.py             # rebuild every archetype
```

Requires Blender >= 4.2 on `PATH`. The script installs MPFB into `target/mpfb/`
and leaves the user's own Blender configuration untouched.

## three.js — Xbot (legacy)

- **File:** `assets/characters/Xbot.glb`
- **Licence:** MIT, via the [three.js](https://github.com/mrdoob/three.js)
  examples (`examples/models/gltf/Xbot.glb`)
- **Rig:** Mixamo humanoid, 67 bones

Retained as the source of the bundled `idle` and `run` mocap clips, which
`build_locomotion_clips` loads by animation index.

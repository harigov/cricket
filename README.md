# Willow Cricket

A 3D cricket simulation game built with [Bevy](https://bevy.org) in Rust —
inspired by the Big Ant Studios cricket titles. Play quick matches against
the AI or compete in a knockout tournament across multiple stadiums, with
full batting / bowling / fielding gameplay on both keyboard and gamepad.

## Features

- **Full match simulation** — T20-style matches (5/10/20 overs) with
  **realistic ball physics**: 9.81 m/s² gravity, quadratic drag (`Cd≈0.47`,
  `ρ=1.225`, `A=πr²`, `m=0.16 kg`), Magnus swing (`Cl≈0.055·|v|`) and
  post-bounce spin Magnus, pitch-aware restitution/grip/turn
  (green seamers swing & bounce, dusty turners grip).
- **Batting** — timing-based shot play with a timing meter, leg/off-side
  aiming, lofted (risky) shots. Perfect/good/late/edge timing tiers drive
  shot power, elevation and direction; edges can be caught behind.
- **Bowling** — two-stage aim mechanic (lock length, then line) with an
  execution scatter based on bowler skill; AI batsmen punish bad balls and
  respect match situation (required run rate pressure). Includes yorkers,
  bouncers and slower balls.
- **Fielding** — standard field layouts (keeper + 10 fielders), predictive
  chase AI, catches near the landing point, run-outs on risky second runs,
  automatic running between wickets with run-bob animation.
- **Dismissals** — bowled, caught, caught behind, run out, plus wides.
- **Match flow** — overs/wickets/balls bookkeeping, strike rotation,
  over changes with bowler rotation, innings break, target chases,
  results & margins, end-of-match scorecard summary.
- **Tournament mode** — 4-team knockout championship seeded across three
  stadiums; your matches are played, the rest are simulated by a
  rating-driven quick-sim engine.
- **10 teams** of fictional players with individual batting/bowling ratings
  and bowling styles (fast, fast-medium, medium, off-spin, leg-spin):
  India, Australia, England, Pakistan, South Africa, New Zealand,
  West Indies, Sri Lanka, Bangladesh, Afghanistan. Every side has an original
  crest, two-color match kit, uniform badge, and match-day stadium branding.
- **4 stadiums** — Harbour Oval, Rose Bowl Gardens, Fortress Arena,
  Highveld Dome — each with its own boundary size and pitch behaviour
  (green seamer / hard & true / dusty turner / dry), striped mown outfield
  with outer shell from **Poly Pizza Stylized Stadium** (CC-BY 3.0,
  `assets/stadium/poly_stadium.glb` 104KB) and tiered stands.
- **Realistic crowd** — ~120 **Kenney Blocky Characters** (CC0,
  `assets/crowd/crowd-*.glb` 111KB each) seated on stands, varied scale/yaw,
  replacing 480 cuboid blobs; 1k tris each, instanced via `SceneRoot`.
- **Realistic human models** — MIT-licensed **Xbot glTF** (Mixamo rig,
  `assets/characters/Xbot.glb` 2.8 MB via three.js `examples` — MIT, PBR,
  16k tris, 55-bone humanoid). Skinned, code-driven skeletal animation
  (idle waggle, run, bowling windmill, bat swing) — fully MIT-distributable
  and retarget-friendly for future Mixamo clips.
- **Atmosphere** — procedural sky sphere (day: pale→deep blue, night:
  starry navy with floodlights, `N` to toggle), distance fog, sun/moon +
  4 floodlights, ball trail, camera shake, sponsor ribbons & crest pylons.
- **Ground** — photoreal outfield grass albedo (embedded, ~4 m tile repeat,
  linear + mipmapped + 8× anisotropic filtering) with runtime mow-band tinting
  and stadium-specific hue; procedural dirt pitch (worn centre), PBR turf
  roughness ~0.88 / reflectance ~0.42 (matte live grass, no metallic).
- **Sound** — tiered bat cracks (light/heavy/edge), woody wicket/catch/bounce,
  distinct four/six crowd swells, stadium murmur + music bed with smooth side-chain
  ducking, Kenney CC0 UI sounds, and a **broadcast commentary partnership**
  (British lead + Australian analyst, 108 mastered clips): short context-safe
  calls, fact-verified analysis (fifties, hundreds, required rate, dot streaks,
  clutch wickets), natural gaps between deliveries. Male/female lead toggle;
  Master/SFX/Music/Commentary controls.
- **Animation** — skeletal `slerp` blending (14 rad/s) on Mixamo bones:
  batter idle waggle, bowler windmill, run cycle, bat follow-through.
- **Camera** — four modes (batting end, bowling end, broadcast, follow ball)
  with `C` / `Y` to cycle; smooth blending and wicket shake.
- **Input** — keyboard + gamepad throughout; fully remappable keyboard
  bindings persisted to `~/.config/willow_cricket/controls.json` and a
  Settings screen with volume sliders.

## Controls

| Action | Default Keyboard | Gamepad | Notes |
|---|---|---|---|
| Confirm / play shot / lock aim | Space | A |  |
| Back | Esc | B |  |
| Lofted shot (hold) | Left Shift | LT |  |
| Aim left/right | A / D | Left stick / D-pad |  |
| Cycle delivery type | Q | X | bowling |
| Cycle camera | C | Y | `Batting → Bowling → Broadcast → Follow` |
| Menu navigation | W/S or ↑/↓ | D-pad |  |
| Menu Left/Right | A/D or ←/→ | — | volumes, etc. |

All keyboard bindings can be changed in **Main → Settings**. Gamepad layout is fixed.

### Bowling sequence
1. Press **Confirm** to start your run-up from the Ready screen.
2. A marker sweeps down the pitch — press **Confirm** to lock the **length**.
3. The marker then sweeps across — press **Confirm** to lock the **line**.

Skillful timing keeps the ball on a good line and length; sloppy timing
sprays it (wides get called).

### Batting
Watch the run-up, then press **Confirm** as the ball arrives at the bat.
The timing meter shows your swing relative to the perfect-contact window
(green band). Hold **Loft** for a big hit at extra risk, steer with
**A/D**.

## Building

Requires Rust 1.85+ (edition 2024).

```bash
cargo build --release
./target/release/cricket
```

Or run directly:

```bash
cargo run --release
```

The game opens at **1920×1080** by default (window remains resizable). The
3D match camera uses **4× MSAA** and 4096 px directional shadow maps.

### Linux dependencies
X11 development headers are required (`libx11`, `libxcursor`, `libxi`,
`libxrandr`). For gamepad support install `libudev`:

```bash
# Debian/Ubuntu
sudo apt install libxkbcommon-dev libudev-dev
# Fedora
sudo dnf install libudev-devel
```

Windows and macOS need no extra system dependencies — `cargo build`
handles everything.

### Automated smoke test

A scripted self-test drives the menus into a match, plays deliveries and
saves screenshots:

```bash
CRICKET_AUTOTEST=1 ./cricket              # quick match
CRICKET_AUTOTEST=tournament ./cricket     # tournament
CRICKET_AUTOTEST=settings  ./cricket      # settings screen
CRICKET_AUTOTEST=stadium ./cricket        # day broadcast establishing shot @ 16s
CRICKET_AUTOTEST=stadium-night ./cricket  # night broadcast establishing shot @ 16s
```

Press **F12** any time to save a screenshot to `/tmp/opencode/`.

## Tests

Domain logic (rules, geometry, teams, tournaments) has unit tests:

```bash
cargo test
```

## Formatting & linting

Formatting is `rustfmt` with the settings in `rustfmt.toml`; lints are
`clippy`, configured by the `[lints]` table in `Cargo.toml` and enforced as
errors in CI. Install the components once:

```bash
rustup component add rustfmt clippy
```

Then run the whole gate — formatting, lints and tests — with:

```bash
scripts/check.sh          # check only (what CI runs)
scripts/check.sh --fix    # reformat and apply machine-applicable lint fixes
```

Or invoke the individual tools:

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
```

## Project structure

```
src/
├── core/            # Pure domain logic (no engine deps)
│   ├── geometry.rs  # Pitch dimensions, field positions, angles
│   ├── rules.rs     # Scorecards, over/inning progression, results
│   ├── stadiums.rs  # Stadium + pitch-surface definitions
│   ├── teams.rs     # Teams, players, ratings
│   └── tournament.rs# Knockout bracket, quick-sim
├── game/
│   ├── audio.rs     # Hybrid audio: 44.1 kHz procedural SFX + Kenney CC0 OGG + 142 neural commentary (Guy/Jenny) + dynamic music
│   ├── ball.rs      # Realistic flight: quadratic drag, Magnus swing/spin, pitch-aware bounce
│   ├── fielding.rs  # Fielder entities, predictive chase AI
│   └── match_flow.rs# Delivery cycle, contact resolution, AI
├── render/
│   ├── camera_rig.rs# Camera modes + toggle + shake
│   ├── player.rs    # Xbot glTF human (MIT) + skeletal code-driven anim
│   └── stadium.rs   # Procedural stadium: stripes, crowd, sight screens
├── ui/
│   ├── hud.rs       # Scoreboard, prompts, timing meter, match summary
│   └── menus.rs     # Menus, setup wizard, tournament bracket, settings
├── input.rs         # Action mapping, KeyBindings (persisted), rebind
└── state.rs         # App states
```

## Roadmap

- Manual running between wickets (Sprint already mapped, assist-run today)
- LBW / stumped dismissals, no-balls, DRS
- Test-match format with declaration/innings rules
- Player career stats persistence, team editor
- Replays, Hawk-Eye, wagon wheels
- Gamepad rumble on wickets

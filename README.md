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
  West Indies, Sri Lanka, Bangladesh, Afghanistan.
- **4 stadiums** — Harbour Oval, Rose Bowl Gardens, Fortress Arena,
  Highveld Dome — each with its own boundary size and pitch behaviour
  (green seamer / hard & true / dusty turner / dry), striped mown outfield,
  crowd blobs and sight screens.
- **Realistic human models** — MIT-licensed **Xbot glTF** (Mixamo rig,
  `assets/characters/Xbot.glb` 2.8 MB via three.js `examples` — MIT, PBR,
  16k tris, 55-bone humanoid). Skinned, code-driven skeletal animation
  (idle waggle, run, bowling windmill, bat swing) — fully MIT-distributable
  and retarget-friendly for future Mixamo clips.
- **Atmosphere** — sky gradient, distance fog, warm late-afternoon sun,
  ball trail on big hits, camera shake on wickets/boundaries.
- **Sound** — procedural audio (bat crack, stump clatter, crowd, murmur)
  + Master/SFX volume controls.
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
```

Press **F12** any time to save a screenshot to `/tmp/opencode/`.

## Tests

Domain logic (rules, geometry, teams, tournaments) has unit tests:

```bash
cargo test
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
│   ├── audio.rs     # Hybrid audio: 44.1 kHz procedural SFX + Kenney CC0 OGG + music beds
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

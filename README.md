# Willow Cricket

A 3D cricket simulation game built with [Bevy](https://bevy.org) in Rust —
inspired by the Big Ant Studios cricket titles. Play quick matches against
the AI or compete in a knockout tournament across multiple stadiums, with
full batting / bowling / fielding gameplay on both keyboard and gamepad.

## Features

- **Full match simulation** — T20-style matches (5/10/20 overs) with
  realistic ball physics: gravity, drag, swing in the air, seam/spin
  deviation off the pitch, bounce variation by surface.
- **Batting** — timing-based shot play with a timing meter, leg/off-side
  aiming, lofted (risky) shots. Perfect/good/late/edge timing tiers drive
  shot power, elevation and direction; edges can be caught behind.
- **Bowling** — two-stage aim mechanic (lock length, then line) with an
  execution scatter based on bowler skill; AI batsmen punish bad balls and
  respect match situation (required run rate pressure).
- **Fielding** — standard field layouts (keeper + 10 fielders), chase AI,
  catches near the landing point, run-outs on risky second runs,
  automatic running between wickets.
- **Dismissals** — bowled, caught, caught behind, run out, plus wides.
- **Match flow** — overs/wickets/balls bookkeeping, strike rotation,
  over changes with bowler rotation, innings break, target chases,
  results & margins.
- **Tournament mode** — 4-team knockout championship seeded across three
  stadiums; your matches are played, the rest are simulated by a
  rating-driven quick-sim engine.
- **6 teams** of fictional players with individual batting/bowling ratings
  and bowling styles (fast, fast-medium, medium, off-spin, leg-spin).
- **4 stadiums** — Harbour Oval, Rose Bowl Gardens, Fortress Arena,
  Highveld Dome — each with its own boundary size and pitch behaviour
  (green seamer / hard & true / dusty turner / dry).
- **Keyboard + gamepad** input throughout.

## Controls

| Action | Keyboard | Gamepad |
|---|---|---|
| Confirm / play shot / lock aim | Space / Enter | A |
| Back | Esc | B |
| Lofted shot (hold while playing shot) | Left Shift | LT |
| Aim left/right | ← → or A/D | Left stick / D-pad |
| Menu navigation | ↑ ↓ or W/S | D-pad / left stick |

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
**left/right**.

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
CRICKET_AUTOTEST=1 ./cricket          # quick match path
CRICKET_AUTOTEST=tournament ./cricket # tournament path
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
│   ├── ball.rs      # Ball flight: gravity/drag/swing/bounce/turn
│   ├── fielding.rs  # Fielder entities, chase AI
│   └── match_flow.rs# Delivery cycle orchestration, contact resolution
├── render/
│   ├── camera_rig.rs# Camera modes (batting end, bowling end, follow…)
│   ├── player.rs    # Procedural figures + animation
│   └── stadium.rs   # Procedural stadium construction
├── ui/
│   ├── hud.rs      # Scoreboard, prompts, timing meter
│   └── menus.rs    # Menus, setup wizard, tournament bracket
├── input.rs        # Keyboard/gamepad action mapping
└── state.rs        # App states
```

## Roadmap ideas

- Manual running between wickets (Sprint button already mapped)
- LBW / stumped dismissals, no-balls, DRS
- Test-match format with declaration/innings rules
- Player career stats persistence, team editor
- Audio (bat crack, crowd), replays, more camera modes
- Gamepad rumble on wickets

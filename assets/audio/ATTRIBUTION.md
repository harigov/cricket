# Audio Attribution — Willow Cricket

All audio in this game is MIT-compatible and safe to redistribute under the project's MIT license.

## 1. Embedded UI Sounds (CC0 — no attribution required)

Menu and HUD feedback sounds are sourced from Kenney's CC0 audio packs:

- **Kenney Interface Sounds** (CC0 1.0) — https://kenney.nl/assets/interface-sounds
  - `assets/audio/ui/confirm.ogg` ← `confirmation_001.ogg` (menu confirm)
  - `assets/audio/ui/back.ogg` ← `back_001.ogg` (menu back/cancel)
  - `assets/audio/ui/nav.ogg` ← `select_001.ogg` (list navigation tick)
  - `assets/audio/ui/tick.ogg` ← `tick_001.ogg` (slider tick)
  - `assets/audio/ui/error.ogg` ← `error_001.ogg` (invalid)
  - `assets/audio/ui/bong.ogg` ← `bong_001.ogg` (innings break)
- **Kenney UI Audio** (CC0 1.0) — https://kenney.nl/assets/ui-audio
  - `assets/audio/ui/click1.ogg` ← `click1.ogg` (alternate soft tick)
  - `assets/audio/ui/rollover.ogg` ← `rollover1.ogg` (hover, reserved)

> License: Creative Commons Zero 1.0 Universal — https://creativecommons.org/publicdomain/zero/1.0/
> No attribution required. Files are unmodified apart from filename. See `LICENSE_KENNEY.txt`.

These packs were selected because every file is CC0 with no per-file split licensing, normalized volume, and a consistent style — ideal for MIT distribution (see research notes in `docs/AUDIO_RESEARCH.md`). They replace the previous 22 kHz mono blip with professionally mixed stereo OGG at 44.1 kHz.

## 2. Procedural SFX & Music (MIT — original synthesis)

All cricket-specific sounds are procedurally synthesized at startup in `src/game/audio.rs` and therefore owned by the project under MIT:

- `bat_light`, `bat_heavy`, `bat_edge` — layered noise + harmonic thump (44.1 kHz, ADSR envelope)
- `wicket` — three wood-knock resonances with clatter
- `cheer_four` / `cheer_six` — distinct crowd swells (four: 0.9 s bright, six: 2.1 s stadium-wide)
- `catch` — glove thud + gasp
- `bounce` — pitch impact thud
- `crowd_ambient` / `stadium_ambient` — 8 s brown-noise murmur loop (menu 0.08, match 0.18)
- `menu_music` / `match_music` — 32 s procedural chord loop (C–G–Am–F @ 96 BPM, pad + bass + soft kick)
- fallback UI tones (`nav`, `confirm`, `back`, `error`) — FM synthesis, used only if embedded OGG not yet loaded

Synthesis runs at 44.1 kHz mono 16-bit WAV in memory (no external files needed), so the binary remains self-contained per the existing `target/release/cricket` promise.

## 3. Recommended drop-in replacements (all CC0/CC-BY, MIT-compatible)

If you want to replace the procedural beds with recorded samples, these sources are verified MIT-compatible (no NC/SA restrictions):

| Category | Source | License | URL |
|---|---|---|---|
| Bat crack | directory.audio — Wooden Bat Hits (ID 281) | CC0 | https://directory.audio/sound-effects/sports/281-wooden-bat-hits |
| Bat drop | directory.audio — Wooden Bat Dropped On Asphalt (ID 279) | CC0 | https://directory.audio/sound-effects/sports/279-wooden-bat-dropped-on-asphalt |
| Crowd cheer / stadium | Freesound filtered CC0, e.g. "CricketBall Hitting Bat" by DanielRousseau | CC0 | https://freesound.org/people/DanielRousseau/sounds/366780/ |
| Crowd ambient | Pixabay — Cricket Stadium Crowd (check per file) | Pixabay Content License (commercial-safe, no attribution; not CC0 but allows game distribution) | https://pixabay.com/sound-effects/search/cricket%20stadium/ |
| Background music | OpenGameArt — CC0 Music / CC0 BGM collections | CC0 | https://opengameart.org/content/cc0-music-0, https://opengameart.org/content/cc0-bgm |
| Music alternative | Incompetech (Kevin MacLeod) | CC-BY 4.0 (requires one-line credit) | https://incompetech.com |
| Extra SFX atlas | Sonniss GDC bundles | Royalty-free, no attribution, commercial-safe | https://sonniss.com |

> Pixabay Content License allows distribution inside a game but not resale of the unaltered file as stock audio — placing the file under `assets/audio/` and shipping it inside the executable is explicitly allowed. For strict MIT purity, prefer the CC0 rows above.

To use a replacement, drop an OGG/WAV into `assets/audio/sfx/` or `assets/audio/music/` and update the handle name in `src/game/audio.rs::setup_sfx` — the engine will fallback to procedural if the file is absent (see `AudioPlugin` docs).

## 4. Big Ant Studios Cricket 26 — Sound Research Summary

See `docs/AUDIO_RESEARCH.md` for the full analysis of what makes Cricket 26 sound "premium" (commentary from Gilchrist/Mitchell, dynamic crowd layers, Ashes theatre, press conferences, etc.) and how Willow Cricket maps those to feasible MIT-safe equivalents.

---

*If you credit optional sources, please add: "UI sounds by Kenney (kenney.nl, CC0)" and list any CC-BY tracks you add.*

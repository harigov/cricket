# Audio Research — Willow Cricket vs Big Ant Studios Cricket 26

> Goal: understand what makes Cricket 26 sound premium and find MIT-compatible replacements.

## 1. What Cricket 26 actually sounds like

Based on reviews, patch notes, and feature lists (Wisden 2026-01-02, Big Ant board 2026-05-07, Nacon/ECB announcements):

- **Licensed theatre**: Ashes mode with national anthems, press conferences, stadium-specific PA. Immersion before first ball — not just SFX.
- **Commentary**: Adam Gilchrist + Ali Mitchell delivering context-aware lines (ebb/flow, fielding mistakes, momentum swings). Patch 7-May explicitly notes "improvements to commentary accuracy and environmental sound."
- **Environmental sound**: crowd layers reacting to run rate/pressure, distance fog-matched ambience, boundary rope crowd surges.
- **Gameplay audio**: convincing bat/ball impacts, edges that may/may not carry, bowler run-up, fielder ball-tracking audio cues.
- **Menu/UX**: Pro Team, Academy, Ashes hub — each with distinct background music beds (anthem-like, orchestral tension).

**Take-away**: premium feel = *music + layered ambience + differentiated UI feedback + context-aware SFX* (not monolithic crowd noise or single blip).

## 2. Mapping to Willow Cricket (feasible scope)

| Big Ant layer | Willow Implementation | Quality bar |
|---|---|---|
| Menu anthem (orchestral/lo-fi stadium) | 32 s procedural loop C–G–Am–F @ 96 BPM, pad + bass + soft kick; CC0 OGG fallback via embedded asset | Loopable, not harsh sine; 44.1 kHz, gentle side-chain |
| UI navigation | Kenney CC0 Interface/UI packs (100 + 50 files, 44.1 kHz stereo OGG) — distinct: nav tick, confirm arpeggio, back descending, error buzz | No single blip; up/down vs confirm vs cancel audibly different |
| Bat impact | Tiered synthesis: light (defensive), heavy (drive/six), edge click; driven by timing tier + vel + loft | Heavy= 34 m/s + harmonic 145 Hz thump + 1800 Hz crack; edge= squirt + high click |
| Stumps/catch | Wood-knock triplet + clatter, glove thud + crowd gasp | <0.7 s, woody, not sine |
| Crowd | Two layers: `crowd_ambient` (brown-noise murmur 8 s loop, 0.10 menu vs 0.20 match) + `cheer_four` (0.9 s) vs `cheer_six` (2.1 s rising) + wicket roar | Four is bright short swell, six is stadium-wide long swell; wicket has shake |
| Pitch bounce | Low thump 90 Hz + filtered noise | Subtle, not silent |
| Between-overs tension | Bong/drum + music ducking | |
| In-match background music | 12 s stadium bed (55 Hz drone + swell, 0.14 match, ducks to 0.05 under commentary) + 8 s crowd murmur | Audible but not intrusive; ducks 65% when commentary plays |
| Commentary | 142 neural lines (Guy male + Jenny female, edge-tts excited) — 17 categories × 2–7 variants, random non-repeating, ducking music | TV-level energy: Guy `excited +12-15%`, Jenny `excited/friendly`, calm for dots |

## 3. Commentary TTS — broadcast partnership

- **Engine:** `edge-tts` plain text (`Communicate(text, voice, rate=…)`) — the earlier
  SSML-as-text bug caused markup to be spoken, producing 41–49 s clips; fixed and
  guarded by automatic duration validation (0.8–6.5 s).
- **Voices:** `en-GB-RyanNeural` (lead) + `en-AU-NatashaNeural` (analyst) for an
  authentic international-cricket booth. Female selection swaps roles.
- **Editorial:** short context-safe calls (<8 words), no routine exclamations,
  probability-gated routine events + cooldown for natural gaps, superlatives
  reserved for sixes/wickets.
- **Facts:** contextual lines fire only on verified crossings (fifty/century,
  team hundred, RRR climb >11, three consecutive dots, clutch wicket ≤45 needed,
  bowler spell ≥2 wkts <7 econ). Each fact is its own category — variants only
  within a fact.
- **Mastering:** loudnorm I=-16 TP=-1.5 LRA=11 → Vorbis q4 48 kHz; real durations
  in durations.json drive scheduling; smooth side-chain ducking (attack 0.25 s,
  release 2 s) keeps crowd audible under speech.
- **Fallbacks:** Piper (MIT, offline) and Kokoro-82M (Apache 2.0) documented below.

## 4. License research — what is MIT-safe?

| Source | License | MIT-OK? | Notes |
|---|---|---|---|
| **Kenney.nl all audio** | CC0 1.0 | ✅ | No attribution, no fee, commercial-safe. Every file same license (Interface Sounds, UI Audio, Impact, SCI-Fi etc.). Includes `License.txt` in download. |
| **directory.audio** wooden bat hits | CC0 | ✅ | Single-file CC0, login required but redistribution allowed. |
| **Freesound.org CC0 subset** | CC0 | ✅ | Filter `license: CC0`; requires free account. Avoid CC-BY-NC or CC-BY-SA. |
| **OpenGameArt CC0** | CC0 | ✅ | Filter by license; OGA-BY also OK with attribution. Avoid GPL audio. |
| **Pixabay Music/SFX** | Pixabay Content License | ✅ with caveat | Commercial-safe, no attribution, but can't resell unaltered as standalone stock. Shipping inside game is allowed. Prefer CC0 for strict MIT. |
| **Sonniss GDC bundles** | Royalty-free | ✅ | Yearly free bundle, explicitly commercial-safe. |
| **Mixkit** | Mixkit Free License | ✅ | Allows commercial, no attribution, not CC0 but permissive. |
| **Zapsplat (free)** | Standard (requires attribution) | ⚠️ | Requires credit; OK if you add credits. CC0 subset is OK. |
| **Envato/Pond5/Storyblocks** | Paid royalty-free | ❌ for open source | Requires purchase, not redistributable. |

**Decision**: ship Kenney CC0 for UI (real files in repo) + superior procedural synthesis for cricket SFX/music (MIT, no external dependency) + documented CC0 drop-in paths for anyone who wants recorded crowd/bat samples. This keeps the binary self-contained while meeting "find open versions and use them."

## 5. Why not just ship Pixabay/Envato?

- Pixabay is *almost* MIT but its "no standalone distribution" clause makes strict MIT purists nervous.
- Envato etc. are not free (paywall).
- Freesound CC0 is excellent but requires manual cherry-picking + account; for onboarding we prefer Kenney (one zip, 100 consistent files).

## 6. Implementation notes

- Bevy 0.18 `bevy_audio` supports WAV + Vorbis (OGG). Use 44.1 kHz 16-bit mono WAV for procedural, OGG for Kenney (already Vorbis, ~6–14 kB each, negligible binary growth via `embedded_asset!`) and for commentary (Vorbis q4, ~260 KB, streaming).
- `AudioSettings` now has `master` (0.85), `sfx` (0.90), `music` (0.70), `commentary` (Male/Female/Off) and `commentary_volume` (0.85); `GlobalVolume` handles master, per-source `PlaybackSettings.volume` handles ducking. Settings persisted via UI (A/D) and shown in `Settings` screen (17 rows).
- Music uses `PlaybackMode::Loop`, crowd ambient loops separately; crossfade via `music_control` system on `AppState` changes with 65% duck (0.14→0.05) when `CommentaryPlaying.remaining>0.1`.
- SFX use `PlaybackMode::Despawn` one-shots with slight pitch variance (`0.96–1.04`) for realism.
- Commentary: `CommentaryHandles` holds `HashMap<String, Vec<Handle<AudioSource>>>` for male/female, loaded via `asset_server.load("audio/commentary/{gender}/{cat}_{02}.ogg")` in `setup_commentary`. `commentary_on_result` maps `ResultPause` text → category, `commentary_on_phase` maps `OverBreak`/`InningsBreak`/`MatchOver`/`welcome`, picks random non-repeating via `CommentaryHistory` (avoids immediate repeat + 30% re-random), spawns `AudioPlayer` despawn with `commentary_volume`, and sets `CommentaryPlaying` 2–3 s to block overlap and duck music. Fallback to `dot` 40% if unknown.
- Background music when game is being played: `match_loop` (12 s drone + swell, 55 Hz + 110 Hz) at 0.14 (vs menu 0.26) provides stadium bed without masking commentary; crowd murmur at 0.18 in-match vs 0.08 menu.

## 7. References

- Wisden review — https://www.wisden.com/cricket-features/cricket-26-review-big-ant-studios
- Patch notes 7-May — https://board.bigant.com/t/extended-patch-notes-7-may/4824
- Kenney Interface Sounds — https://kenney.nl/assets/interface-sounds
- Kenney UI Audio — https://kenney.nl/assets/ui-audio
- directory.audio Wooden Bat Hits (CC0) — https://directory.audio/sound-effects/sports/281-wooden-bat-hits
- Cinevva free SFX guide (license matrix) — https://app.cinevva.com/guides/free-sound-effects-music
- Pixabay cricket stadium crowd — https://pixabay.com/sound-effects/search/cricket%20stadium/
- edge-tts (Guy/Jenny Neural, SSML excited) — https://github.com/rany2/edge-tts
- Piper TTS (MIT, offline) — https://github.com/rhasspy/piper
- Kokoro-82M (Apache 2.0, 82M, StyleTTS2, 6× real-time CPU) — https://huggingface.co/hexgrad/Kokoro-82M and https://github.com/remsky/Kokoro-FastAPI


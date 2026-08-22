# Commentary — Willow Cricket

Broadcast-style partnership: a **lead** commentator calls the action; an
**analyst** adds verified-fact analysis. 54 clips per role, 108 total.

## Roles & Voices
- **Lead (default male):** `en-GB-RyanNeural` — British, classic play-by-play.
- **Analyst (default female):** `en-AU-NatashaNeural` — Australian, statistics.
Settings → Commentary selects who leads; the other becomes the analyst.

## Fact-exact categories
Action calls (random variant, same fact): `dot` `beaten` `single` `two`
`three` `four` `six` `wide` `bowled` `caught` `caught_behind` `run_out`
`welcome` `innings_break` `match_win` `match_tie`.

Fact-exact analyst lines (single variant, triggered only when verified):
`context_fifty` `context_century` `context_team_hundred` `context_rrr`
`context_dots` `context_clutch_wicket` `context_bowler`.
`over_complete` has three fact variants chosen by runs in the over.

## Engine behaviour (src/game/audio.rs)
- Real durations from `durations.json` drive scheduling — no overlap, no premature duck-release.
- Smooth side-chain: music ducks to 30% with 0.25 s attack / 2 s release while a clip plays.
- Routine calls gated by probability (dot 30%, single 50%…) + 7 s cooldown; key moments always called.
- Analyst follow-ups queue ~0.45 s after the lead call finishes (~35% of qualifying moments).

## Regeneration
```bash
pip install edge-tts
python3 scripts/generate_commentary_fixed.py   # regenerates missing/bad clips only
```
Validation rejects any clip outside 0.8–6.5 s (catches spoken-markup bugs).
Mastered with ffmpeg loudnorm I=-16 TP=-1.5 LRA=11 → Vorbis q4.

License: generated audio is original MIT content. See ../ATTRIBUTION.md.

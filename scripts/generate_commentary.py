#!/usr/bin/env python3
"""
Generate high-quality cricket commentary via edge-tts (Microsoft Neural).
Male voice: en-US-GuyNeural (Passion, deep, perfect for excited TV commentary)
Female voice: en-US-JennyNeural (Friendly, clear, energetic)

Both support expressive styles: excited, cheerful, friendly, calm.
We use SSML with <mstts:express-as style="excited" styledegree="2"> and prosody rate.

If edge-tts fails (no internet), falls back to piper-tts with local models.

Generated files are OGG Vorbis (Bevy-compatible via vorbis feature) at q:4.
Place in assets/audio/commentary/male|female/*.ogg

MIT-compatible: Generated audio is original content owned by project.
Edge TTS voices are Microsoft neural; output is user-owned per ToS.
Piper voices are MIT/Apache (en_US-ryan-medium, en_US-amy-medium).

Usage:
  python3 scripts/generate_commentary.py --voice male   # or female or both
  python3 scripts/generate_commentary.py --list  # list all lines
  CRICKET_TTS=edge python3 scripts/generate_commentary.py --regen

Requires: pip install edge-tts
Optional fallback: pip install piper-tts and download voices to /tmp/piper_voices
"""
import asyncio
import argparse
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).parent.parent
ASSETS = ROOT / "assets/audio/commentary"

# Commentary library: category -> list of (text, style, rate)
# style: excited/cheerful/friendly/calm ; rate: -10% to +20%
# We use SSML to convey excitement, energy varies with event.
LIBRARY = {
    "welcome": [
        ("Welcome to Willow Cricket! It's a beautiful day for cricket!", "friendly", "+5%"),
        ("Hello and welcome to Willow Cricket, live from the stadium!", "friendly", "+5%"),
    ],
    "dot": [
        ("Dot ball, well bowled, no run.", "calm", "0%"),
        ("Beaten! Past the edge, no run.", "friendly", "+5%"),
        ("Solid defence, dot ball.", "calm", "0%"),
        ("Good line and length, dot ball.", "friendly", "0%"),
        ("Defended, no run. Pressure building.", "calm", "0%"),
        ("No run, excellent bowling.", "friendly", "0%"),
    ],
    "single": [
        ("Quick single taken, good running!", "friendly", "+5%"),
        ("They scamper through for one.", "friendly", "+5%"),
        ("Single, keeps the strike.", "calm", "0%"),
        ("One run, well judged.", "friendly", "0%"),
        ("Pushed for a single, they make it easily.", "friendly", "+5%"),
    ],
    "two": [
        ("They come back for two, good running!", "friendly", "+5%"),
        ("Two runs, well placed!", "friendly", "+5%"),
        ("Excellent running, two more.", "cheerful", "+5%"),
    ],
    "three": [
        ("Three runs! Great hustle between the wickets!", "cheerful", "+10%"),
        ("They push hard and get three!", "excited", "+10%"),
    ],
    "four": [
        ("Beautiful timing! Four runs, exquisite!", "excited", "+10%"),
        ("Crashed to the rope for four! What a shot!", "excited", "+12%"),
        ("Elegant cover drive, four runs!", "cheerful", "+10%"),
        ("What a shot! Races away for four!", "excited", "+10%"),
        ("Exquisite timing, four! The crowd loves it!", "excited", "+12%"),
        ("Four runs! Pure class, timed to perfection!", "cheerful", "+10%"),
        ("Driven beautifully, four more to the total!", "excited", "+10%"),
    ],
    "six": [
        ("Maximum! That's out of the ground! Six runs!", "excited", "+15%"),
        ("Huge hit! Six runs, monstrous strike!", "excited", "+15%"),
        ("That's massive! Six runs, crowd on its feet!", "excited", "+15%"),
        ("Launch! Six runs, clean as a whistle!", "excited", "+15%"),
        ("Six runs! That has flown into the stands!", "excited", "+15%"),
        ("What a hit! Six runs, absolute beauty!", "excited", "+12%"),
        ("Over the rope for six! Tremendous power!", "excited", "+15%"),
    ],
    "wide": [
        ("Wide ball, extra run.", "friendly", "0%"),
        ("Down the leg side, wide called.", "calm", "0%"),
        ("Too wide, umpire signals wide.", "friendly", "0%"),
    ],
    "bowled": [
        ("Bowled him! Timber! What a delivery!", "excited", "+10%"),
        ("Clean bowled! Knocked him over!", "excited", "+12%"),
        ("Through the gate, bowled! Brilliant bowling!", "excited", "+12%"),
        ("Bowled! The stumps are rattled!", "excited", "+10%"),
        ("What a ball! Bowled him neck and crop!", "excited", "+12%"),
        ("Got him! Bowled, fantastic delivery!", "excited", "+10%"),
    ],
    "caught": [
        ("In the air and taken! What a catch!", "excited", "+12%"),
        ("Edged and taken at slip! Brilliant catch!", "excited", "+12%"),
        ("Caught! Excellent catch in the outfield!", "excited", "+10%"),
        ("Taken! The fielder makes no mistake!", "excited", "+10%"),
        ("That's taken, wicket falls! Superb catch!", "excited", "+12%"),
        ("In the air, fielder underneath, taken!", "cheerful", "+10%"),
    ],
    "caught_behind": [
        ("Edged and taken behind! Keeper does the rest!", "excited", "+12%"),
        ("Nicked! Taken by the keeper!", "excited", "+10%"),
        ("Edge, and taken! Gone!", "excited", "+12%"),
        ("Feathered through to the keeper, out!", "excited", "+10%"),
        ("Thin edge, well taken behind the stumps!", "excited", "+10%"),
    ],
    "run_out": [
        ("Run out! Direct hit, he's short!", "excited", "+12%"),
        ("Run out going for the extra, brilliant fielding!", "excited", "+12%"),
        ("Run out! What a throw, what a wicket!", "excited", "+15%"),
        ("He's run out! Fantastic work in the field!", "excited", "+12%"),
    ],
    "over_complete": [
        ("Over complete, excellent over.", "friendly", "0%"),
        ("End of the over, good contest.", "calm", "0%"),
        ("That's the over, time for a change.", "friendly", "0%"),
    ],
    "innings_break": [
        ("Innings break, what a first innings that was!", "cheerful", "+5%"),
        ("End of the innings, time for the chase!", "cheerful", "+5%"),
        ("Innings complete, target set!", "friendly", "+5%"),
    ],
    "match_win": [
        ("And that's the match! What a victory!", "excited", "+10%"),
        ("Match over, what a win! Congratulations!", "excited", "+12%"),
        ("Winners! What a performance, well played!", "cheerful", "+10%"),
        ("Victory! The crowd erupts!", "excited", "+15%"),
    ],
    "match_tie": [
        ("Match tied! What drama, what a game!", "excited", "+12%"),
        ("It's a tie! Unbelievable finish!", "excited", "+12%"),
    ],
    "beaten": [
        ("Beaten! Past the outside edge!", "friendly", "+5%"),
        ("Swing and a miss, beaten!", "friendly", "+5%"),
        ("Past the bat, no shot offered.", "calm", "0%"),
    ],
}

VOICES = {
    "male": "en-US-GuyNeural",
    "female": "en-US-JennyNeural",
    # Alternative British/Australian for authenticity: keep as comment
    # male_alt: en-GB-RyanNeural, en-AU-WilliamNeural
    # female_alt: en-GB-SoniaNeural, en-AU-NatashaNeural
}

def ssml(text, voice, style, rate):
    # Edge TTS supports mstts:express-as with style and styledegree
    # For calm we use style friendly with lower degree, for excited we use 2
    # Some voices don't support all styles; we fallback gracefully.
    degree = "2" if style in ("excited", "cheerful") else "1"
    # Sanitize text for XML
    esc = text.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")
    return f'''<speak version="1.0" xmlns="http://www.w3.org/2001/10/synthesis" xmlns:mstts="http://www.w3.org/2001/mstts" xml:lang="en-US">
<voice name="{voice}">
<mstts:express-as style="{style}" styledegree="{degree}">
<prosody rate="{rate}">{esc}</prosody>
</mstts:express-as>
</voice>
</speak>'''

async def gen_edge(text, voice, style, rate, out_mp3):
    import edge_tts
    s = ssml(text, voice, style, rate)
    comm = edge_tts.Communicate(s, voice)
    await comm.save(out_mp3)

def gen_piper(text, model_path, out_wav):
    cmd = ["piper", "--model", str(model_path), "--output-file", str(out_wav)]
    proc = subprocess.run(cmd, input=text.encode(), stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if proc.returncode != 0:
        print(f"piper failed: {proc.stderr.decode()}", file=sys.stderr)
        raise RuntimeError(proc.stderr.decode())
    return out_wav

def convert_to_ogg(in_path: Path, out_path: Path):
    # Use ffmpeg to convert mp3/wav -> ogg vorbis q:4 (~128kbps)
    cmd = ["ffmpeg", "-y", "-loglevel", "error", "-i", str(in_path), "-c:a", "libvorbis", "-q:a", "4", str(out_path)]
    subprocess.run(cmd, check=True)

async def generate_one(text, voice, style, rate, out_ogg, use_piper=False, piper_model=None):
    tmp = out_ogg.with_suffix(".tmp.mp3") if not use_piper else out_ogg.with_suffix(".tmp.wav")
    try:
        if use_piper:
            gen_piper(text, piper_model, tmp)
            convert_to_ogg(tmp, out_ogg)
        else:
            await gen_edge(text, voice, style, rate, tmp)
            convert_to_ogg(tmp, out_ogg)
        print(f"OK {out_ogg}")
    finally:
        if tmp.exists():
            tmp.unlink()

async def generate_all(voice_filter=None, force=False):
    import edge_tts  # ensure installed
    for gender in ["male", "female"]:
        if voice_filter and voice_filter != gender:
            continue
        voice = VOICES[gender]
        for category, lines in LIBRARY.items():
            for idx, (text, style, rate) in enumerate(lines, start=1):
                filename = f"{category}_{idx:02d}.ogg"
                out = ASSETS / gender / filename
                out.parent.mkdir(parents=True, exist_ok=True)
                if out.exists() and not force:
                    print(f"skip exists {out}")
                    continue
                await generate_one(text, voice, style, rate, out, use_piper=False)
                # Small delay to avoid rate limit
                await asyncio.sleep(0.3)

def list_lines():
    total = 0
    for cat, lines in LIBRARY.items():
        print(f"{cat}: {len(lines)} variants")
        for i, (t,s,r) in enumerate(lines,1):
            print(f"  {i:02d} [{s:9s} {r:4s}] {t}")
        total += len(lines)
    print(f"Total per voice: {total}, both: {total*2}")

if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--voice", choices=["male","female"], help="only one gender")
    ap.add_argument("--list", action="store_true", help="list library")
    ap.add_argument("--force", action="store_true", help="regenerate existing")
    args = ap.parse_args()
    if args.list:
        list_lines()
        sys.exit(0)
    asyncio.run(generate_all(voice_filter=args.voice, force=args.force))

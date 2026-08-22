//! High-quality audio engine for Willow Cricket — inspired by Big Ant Studios Cricket 26.
//! Provides tiered bat cracks, distinct wicket/catch/four/six reactions, pitch bounce,
//! layered crowd ambience and background music, plus Kenney CC0 UI feedback.
//! All synthesis is 44.1 kHz 16-bit mono WAV and runs without external files, but
//! menu UI also plays embedded CC0 OGG (Kenney) when available — see assets/audio/ATTRIBUTION.md.

use bevy::audio::{AudioPlayer, AudioSource, PlaybackSettings, Volume, GlobalVolume};
use bevy::prelude::*;

// ---------------------------------------------------------------------------
// Resources
// ---------------------------------------------------------------------------

#[derive(Resource)]
pub struct SfxHandles {
    pub bat_light: Handle<AudioSource>,
    pub bat_heavy: Handle<AudioSource>,
    pub bat_edge: Handle<AudioSource>,
    pub wicket: Handle<AudioSource>,
    pub catch: Handle<AudioSource>,
    pub bounce: Handle<AudioSource>,
    pub cheer_four: Handle<AudioSource>,
    pub cheer_six: Handle<AudioSource>,
    pub ambient: Handle<AudioSource>,
    // fallback UI tones (used only if embedded OGG not yet ready)
    pub ui_fallback: Handle<AudioSource>,
}

// CC0 UI sounds loaded from embedded OGG — see assets/audio/ui/
#[derive(Resource)]
pub struct UiHandles {
    pub nav: Handle<AudioSource>,
    pub confirm: Handle<AudioSource>,
    pub back: Handle<AudioSource>,
    pub tick: Handle<AudioSource>,
    pub error: Handle<AudioSource>,
    pub bong: Handle<AudioSource>,
}

#[derive(Resource)]
pub struct MusicHandles {
    pub menu: Handle<AudioSource>,
    pub match_loop: Handle<AudioSource>,
}

#[derive(Resource, Clone)]
pub struct AudioSettings {
    pub master: f32,
    pub sfx: f32,
    pub music: f32,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self { master: 0.85, sfx: 0.90, music: 0.70 }
    }
}

#[derive(Component)]
struct AmbientTag;
#[derive(Component)]
struct MusicMenuTag;
#[derive(Component)]
struct MusicMatchTag;

// ---------------------------------------------------------------------------
// WAV helpers
// ---------------------------------------------------------------------------

const SR: u32 = 44100;

fn wav_bytes(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let n = samples.len() as u32;
    let byte_rate = sample_rate * 2;
    let data_size = n * 2;
    let mut out = Vec::with_capacity(44 + data_size as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_size).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_size.to_le_bytes());
    for &s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

#[inline]
fn env_adsr(t: f32, a: f32, d: f32, s: f32, r: f32, dur: f32, sus_level: f32) -> f32 {
    if t < a {
        (t / a).clamp(0.0, 1.0)
    } else if t < a + d {
        1.0 - ((t - a) / d).clamp(0.0, 1.0) * (1.0 - sus_level)
    } else if t < dur - r {
        sus_level * s
    } else if t < dur {
        sus_level * s * ((dur - t) / r).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

// cheap deterministic noise via LCG hash
#[inline]
fn hash_noise(i: usize, seed: u32) -> f32 {
    let h = i.wrapping_mul(1103515245).wrapping_add(seed as usize) >> 16 & 0x7FFF;
    h as f32 / 16383.5 * 2.0 - 1.0
}

// ---------------------------------------------------------------------------
// High-quality synthesis (44.1 kHz)
// ---------------------------------------------------------------------------

fn gen_bat_light() -> Vec<u8> {
    let dur = 0.065f32;
    let n = (SR as f32 * dur) as usize;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / SR as f32;
        let env = (-t * 65.0).exp();
        let noise = hash_noise(i, 0x1234) * 0.28 * env;
        let thump = (std::f32::consts::TAU * 180.0 * t).sin() * (-t * 28.0).exp() * 0.35;
        let crack = (std::f32::consts::TAU * 2200.0 * t).sin() * (-t * 140.0).exp() * 0.22;
        let body = (std::f32::consts::TAU * 620.0 * t).sin() * (-t * 55.0).exp() * 0.15;
        out.push((noise + thump + crack + body).clamp(-1.0, 1.0) * 0.88);
    }
    wav_bytes(&out, SR)
}

fn gen_bat_heavy() -> Vec<u8> {
    let dur = 0.095f32;
    let n = (SR as f32 * dur) as usize;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / SR as f32;
        let env = (-t * 42.0).exp();
        let noise = hash_noise(i, 0x9E37) * 0.32 * env;
        let thump = (std::f32::consts::TAU * 110.0 * t).sin() * (-t * 18.0).exp() * 0.52;
        let crack = (std::f32::consts::TAU * 1650.0 * t).sin() * (-t * 95.0).exp() * 0.28;
        let click = (std::f32::consts::TAU * 3400.0 * t).sin() * (-t * 180.0).exp() * 0.12;
        let ring = (std::f32::consts::TAU * 720.0 * t).sin() * (-t * 35.0).exp() * 0.14;
        out.push((noise + thump + crack + click + ring).clamp(-1.0, 1.0) * 0.92);
    }
    wav_bytes(&out, SR)
}

fn gen_bat_edge() -> Vec<u8> {
    let dur = 0.055f32;
    let n = (SR as f32 * dur) as usize;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / SR as f32;
        let env = (-t * 75.0).exp();
        let tick = (std::f32::consts::TAU * 2800.0 * t).sin() * env * 0.45;
        let thud = (std::f32::consts::TAU * 260.0 * t).sin() * (-t * 45.0).exp() * 0.18;
        let grit = hash_noise(i, 0x777) * 0.18 * env;
        out.push((tick + thud + grit).clamp(-1.0, 1.0) * 0.85);
    }
    wav_bytes(&out, SR)
}

fn gen_wicket() -> Vec<u8> {
    let dur = 0.78f32;
    let n = (SR as f32 * dur) as usize;
    let mut out = vec![0.0f32; n];
    // three woody knocks with different resonant frequencies
    let knocks = [(0.00f32, 540.0, 0.62), (0.11, 780.0, 0.58), (0.24, 460.0, 0.50)];
    for &(off, freq, amp) in &knocks {
        for i in 0..n {
            let t = i as f32 / SR as f32 - off;
            if !(0.0..0.22).contains(&t) {
                continue;
            }
            let env = (-t * 22.0).exp();
            let tone = (std::f32::consts::TAU * freq * t).sin() * env * amp;
            let harm = (std::f32::consts::TAU * freq * 1.52 * t).sin() * env * amp * 0.22;
            let click = if t < 0.005 { (1.0 - t / 0.005) * 0.45 } else { 0.0 };
            out[i] = (out[i] + tone + harm + click).clamp(-1.0, 1.0);
        }
    }
    // add subtle rattle tail
    for i in 0..n {
        let t = i as f32 / SR as f32;
        if t > 0.38 && t < 0.72 {
            let r = hash_noise(i, 0xABCD) * 0.06 * (-(t - 0.38) * 8.0).exp();
            out[i] = (out[i] + r).clamp(-1.0, 1.0);
        }
    }
    for s in &mut out { *s *= 0.88; }
    wav_bytes(&out, SR)
}

fn gen_catch() -> Vec<u8> {
    let dur = 0.42f32;
    let n = (SR as f32 * dur) as usize;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / SR as f32;
        let env = (-t * 18.0).exp();
        let thud = (std::f32::consts::TAU * 95.0 * t).sin() * env * 0.45;
        let slap = hash_noise(i, 0xC0FFEE) * 0.12 * (-t * 55.0).exp();
        let leather = (std::f32::consts::TAU * 420.0 * t).sin() * (-t * 30.0).exp() * 0.18;
        out.push((thud + slap + leather).clamp(-1.0, 1.0) * 0.90);
    }
    wav_bytes(&out, SR)
}

fn gen_bounce() -> Vec<u8> {
    let dur = 0.18f32;
    let n = (SR as f32 * dur) as usize;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / SR as f32;
        let env = (-t * 26.0).exp();
        let thump = (std::f32::consts::TAU * 90.0 * t).sin() * env * 0.55;
        let grit = hash_noise(i, 0xBEEF) * 0.10 * (-t * 70.0).exp();
        let wood = (std::f32::consts::TAU * 180.0 * t).sin() * (-t * 40.0).exp() * 0.18;
        out.push((thump + grit + wood).clamp(-1.0, 1.0) * 0.82);
    }
    wav_bytes(&out, SR)
}

fn gen_cheer_four() -> Vec<u8> {
    let dur = 0.95f32;
    let n = (SR as f32 * dur) as usize;
    let mut buf = [0.0f32; 6];
    let mut bi = 0;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / SR as f32;
        let env = if t < 0.10 { t / 0.10 } else if t > dur - 0.25 { ((dur - t)/0.25).max(0.0) } else { 1.0 };
        let white = hash_noise(i, 0x4444);
        buf[bi] = white; bi = (bi+1)%6;
        let avg = buf.iter().sum::<f32>()/6.0; // gentle low-pass
        // slightly bright
        let swell = 0.90 + 0.10*(t*4.0).sin().abs();
        let wobble = (t*2.6).sin()*0.06 + 1.0;
        out.push(avg * env * swell * wobble * 0.62);
    }
    wav_bytes(&out, SR)
}

fn gen_cheer_six() -> Vec<u8> {
    let dur = 2.10f32;
    let n = (SR as f32 * dur) as usize;
    let mut buf = [0.0f32; 8];
    let mut bi = 0;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / SR as f32;
        let env = if t < 0.14 { t/0.14 } else if t > dur - 0.35 { ((dur - t)/0.35).max(0.0) } else { 1.0 };
        // rising swell
        let rise = (t*0.9).min(1.0)*0.3 + 0.70 + 0.12*(t*2.0).sin().abs();
        let white = hash_noise(i, 0x5555);
        buf[bi]=white; bi=(bi+1)%8;
        let avg = buf.iter().sum::<f32>()/8.0;
        let wobble = (t*1.8).sin()*0.09 + 1.0;
        // add subtle horn-like overtone sweep 180->260 Hz
        let horn_freq = 180.0 + t*38.0;
        let horn = (std::f32::consts::TAU * horn_freq * t).sin() * env * rise * 0.08 * (t*1.2).min(1.0);
        out.push((avg * env * rise * wobble * 0.58 + horn).clamp(-1.0,1.0));
    }
    wav_bytes(&out, SR)
}

fn gen_ambient() -> Vec<u8> {
    let dur = 8.0f32;
    let n = (SR as f32 * dur) as usize;
    let mut out = Vec::with_capacity(n);
    let mut b = 0.0f32;
    for i in 0..n {
        let white = hash_noise(i, 0x9999);
        b += white * 0.015;
        b = (b * 0.987).clamp(-1.0, 1.0);
        let t = i as f32 / SR as f32;
        let fade = (t/0.5).min(1.0) * ((dur - t)/0.5).min(1.0);
        // add occasional distant shout (every ~2.5s)
        let shout_phase = (t*0.4).sin() * 0.5 + 0.5;
        let shout = if shout_phase > 0.92 { hash_noise(i, 0x1111)*0.08*fade } else { 0.0 };
        out.push((b*0.13 + shout) * fade);
    }
    wav_bytes(&out, SR)
}

fn gen_ui_fallback() -> Vec<u8> {
    let dur = 0.08f32;
    let n = (SR as f32 * dur) as usize;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / SR as f32;
        let env = if t < 0.012 { t/0.012 } else { ((dur - t)/0.02).clamp(0.0,1.0) };
        let tone = (std::f32::consts::TAU * 780.0 * t).sin() * env * 0.40;
        out.push(tone);
    }
    wav_bytes(&out, SR)
}

// Procedural menu music: 32s loop, 96 BPM, C-G-Am-F
fn gen_menu_music() -> Vec<u8> {
    let dur = 32.0f32;
    let n = (SR as f32 * dur) as usize;
    let mut out = Vec::with_capacity(n);
    // Chord roots (Hz): C3, G3, A3, F3 and their major/minor triads
    let chords: [[f32; 3]; 4] = [
        [130.81, 164.81, 196.00], // C (C-E-G)
        [98.00, 123.47, 146.83],  // G (G-B-D) approx
        [110.00, 130.81, 164.81], // Am (A-C-E)
        [87.31, 110.00, 130.81],  // F (F-A-C) shifted
    ];
    let chord_dur = 8.0f32; // each chord 8s => 32s total (very slow pavilion feel)
    for i in 0..n {
        let t = i as f32 / SR as f32;
        let chord_idx = ((t / chord_dur) as usize) % chords.len();
        let chord = chords[chord_idx];
        let ct = t % chord_dur;
        let chord_env = (ct/0.6).min(1.0) * ((chord_dur - ct)/0.6).min(1.0) * 0.85 + 0.15;
        // pad: sum of chord tones as soft sines + gentle triangle via harmonic
        let mut pad = 0.0f32;
        for &f in &chord {
            pad += (std::f32::consts::TAU * f * t).sin() * 0.18;
            pad += (std::f32::consts::TAU * f * 2.0 * t).sin() * 0.07; // octave harmonic
            pad += (std::f32::consts::TAU * f * 0.5 * t).sin() * 0.10; // bass octave
        }
        pad *= chord_env * 0.55;
        // subtle bass sine on root
        let bass = (std::f32::consts::TAU * chords[chord_idx][0]*0.5 * t).sin() * 0.22 * chord_env;
        // soft kick every 0.625s (96 BPM)
        let beat = 60.0/96.0;
        let beat_phase = t % beat;
        let kick_env = (-beat_phase * 28.0).exp();
        let kick = (std::f32::consts::TAU * 55.0 * beat_phase).sin() * kick_env * 0.18 * if beat_phase < 0.08 {1.0} else {0.0};
        let hi = if beat_phase < 0.02 { hash_noise(i, 0x2222)*0.06*(-beat_phase*120.0).exp() } else {0.0};
        // gentle moving filter: tremolo 0.3 Hz
        let trem = 0.88 + 0.12 * (t* 0.6 * std::f32::consts::TAU).sin();
        let sample = (pad*0.9 + bass*0.5 + kick + hi) * trem * 0.28;
        // master fade for loopability
        let fade = (t/0.8).min(1.0) * ((dur - t)/0.8).min(1.0);
        out.push((sample * fade).clamp(-1.0, 1.0));
    }
    wav_bytes(&out, SR)
}

fn gen_match_music() -> Vec<u8> {
    // Shorter, tenser drone for in-match tension (12s loop) — low volume bed
    let dur = 12.0f32;
    let n = (SR as f32 * dur) as usize;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / SR as f32;
        let drone = (std::f32::consts::TAU * 55.0 * t).sin() * 0.18
                  + (std::f32::consts::TAU * 110.0 * t).sin() * 0.08
                  + (std::f32::consts::TAU * 82.41 * t).sin() * 0.07;
        let swell = 0.80 + 0.20 * (t*0.35).sin();
        let noise = hash_noise(i, 0x3333) * 0.015;
        let fade = (t/0.6).min(1.0) * ((dur - t)/0.6).min(1.0);
        out.push(((drone*swell + noise)*0.18 * fade).clamp(-1.0,1.0));
    }
    wav_bytes(&out, SR)
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

pub struct AudioPlugin;

impl Plugin for AudioPlugin {
    fn build(&self, app: &mut App) {
        // Embed Kenney CC0 UI sounds (keep binary self-contained)
        bevy::asset::embedded_asset!(app, "../../assets/audio/ui/confirm.ogg");
        bevy::asset::embedded_asset!(app, "../../assets/audio/ui/back.ogg");
        bevy::asset::embedded_asset!(app, "../../assets/audio/ui/nav.ogg");
        bevy::asset::embedded_asset!(app, "../../assets/audio/ui/tick.ogg");
        bevy::asset::embedded_asset!(app, "../../assets/audio/ui/error.ogg");
        bevy::asset::embedded_asset!(app, "../../assets/audio/ui/bong.ogg");
        bevy::asset::embedded_asset!(app, "../../assets/audio/ui/click1.ogg");
        bevy::asset::embedded_asset!(app, "../../assets/audio/ui/rollover.ogg");

        app.init_resource::<AudioSettings>()
            .add_systems(Startup, setup_audio)
            .add_systems(
                Update,
                (
                    bat_crack_on_hit,
                    bounce_sfx,
                    sfx_on_result,
                    ui_feedback,
                    ambient_control,
                    music_control,
                    apply_volumes,
                ),
            );
    }
}

fn setup_audio(
    mut commands: Commands,
    mut assets: ResMut<Assets<AudioSource>>,
    asset_server: Res<AssetServer>,
    mut global_vol: ResMut<GlobalVolume>,
) {
    // Procedural SFX
    let h_bat_light = assets.add(AudioSource { bytes: gen_bat_light().into() });
    let h_bat_heavy = assets.add(AudioSource { bytes: gen_bat_heavy().into() });
    let h_bat_edge = assets.add(AudioSource { bytes: gen_bat_edge().into() });
    let h_wicket = assets.add(AudioSource { bytes: gen_wicket().into() });
    let h_catch = assets.add(AudioSource { bytes: gen_catch().into() });
    let h_bounce = assets.add(AudioSource { bytes: gen_bounce().into() });
    let h_four = assets.add(AudioSource { bytes: gen_cheer_four().into() });
    let h_six = assets.add(AudioSource { bytes: gen_cheer_six().into() });
    let h_ambient = assets.add(AudioSource { bytes: gen_ambient().into() });
    let h_fallback = assets.add(AudioSource { bytes: gen_ui_fallback().into() });

    let h_menu = assets.add(AudioSource { bytes: gen_menu_music().into() });
    let h_match = assets.add(AudioSource { bytes: gen_match_music().into() });

    commands.insert_resource(SfxHandles {
        bat_light: h_bat_light,
        bat_heavy: h_bat_heavy,
        bat_edge: h_bat_edge,
        wicket: h_wicket,
        catch: h_catch,
        bounce: h_bounce,
        cheer_four: h_four,
        cheer_six: h_six,
        ambient: h_ambient.clone(),
        ui_fallback: h_fallback,
    });
    commands.insert_resource(MusicHandles {
        menu: h_menu.clone(),
        match_loop: h_match.clone(),
    });

    // CC0 UI OGG handles — load from assets/ (works both with filesystem and embedded)
    // Files are also embedded via embedded_asset! above for self-contained release builds.
    let ui = UiHandles {
        confirm: asset_server.load("audio/ui/confirm.ogg"),
        back: asset_server.load("audio/ui/back.ogg"),
        nav: asset_server.load("audio/ui/nav.ogg"),
        tick: asset_server.load("audio/ui/tick.ogg"),
        error: asset_server.load("audio/ui/error.ogg"),
        bong: asset_server.load("audio/ui/bong.ogg"),
    };
    commands.insert_resource(ui);

    global_vol.volume = Volume::Linear(0.85);

    // Ambient crowd murmur (always looping, volume ducked per state)
    commands.spawn((
        AmbientTag,
        AudioPlayer(h_ambient),
        PlaybackSettings {
            mode: bevy::audio::PlaybackMode::Loop,
            volume: Volume::Linear(0.14),
            ..default()
        },
    ));
    // Menu music loop
    commands.spawn((
        MusicMenuTag,
        AudioPlayer(h_menu),
        PlaybackSettings {
            mode: bevy::audio::PlaybackMode::Loop,
            volume: Volume::Linear(0.22),
            ..default()
        },
    ));
    // Match ambient music loop (tenser, very low)
    commands.spawn((
        MusicMatchTag,
        AudioPlayer(h_match),
        PlaybackSettings {
            mode: bevy::audio::PlaybackMode::Loop,
            volume: Volume::Linear(0.0),
            ..default()
        },
    ));
}

fn apply_volumes(
    settings: Res<AudioSettings>,
    mut global: ResMut<GlobalVolume>,
) {
    if settings.is_changed() {
        global.volume = Volume::Linear(settings.master.clamp(0.0, 1.0));
    }
}

fn ambient_control(
    state: Res<State<crate::state::AppState>>,
    settings: Res<AudioSettings>,
    mut q: Query<&mut PlaybackSettings, With<AmbientTag>>,
) {
    let target = if *state.get() == crate::state::AppState::Menu { 0.08 } else { 0.18 } * settings.sfx;
    for mut s in &mut q {
        s.volume = Volume::Linear(target);
    }
}

fn music_control(
    state: Res<State<crate::state::AppState>>,
    settings: Res<AudioSettings>,
    mut menu_q: Query<&mut PlaybackSettings, (With<MusicMenuTag>, Without<MusicMatchTag>)>,
    mut match_q: Query<&mut PlaybackSettings, (With<MusicMatchTag>, Without<MusicMenuTag>)>,
) {
    let is_menu = *state.get() == crate::state::AppState::Menu;
    let mvol = settings.music.clamp(0.0, 1.0);
    for mut s in &mut menu_q {
        let target = if is_menu { 0.26 } else { 0.0 } * mvol;
        s.volume = Volume::Linear(target);
    }
    for mut s in &mut match_q {
        let target = if is_menu { 0.0 } else { 0.06 } * mvol;
        s.volume = Volume::Linear(target);
    }
}

fn play_once(
    commands: &mut Commands,
    handle: Handle<AudioSource>,
    vol: f32,
    pitch: f32,
) {
    commands.spawn((
        AudioPlayer(handle),
        PlaybackSettings {
            mode: bevy::audio::PlaybackMode::Despawn,
            volume: Volume::Linear(vol.clamp(0.0, 1.0)),
            speed: pitch,
            ..default()
        },
    ));
}

// Tiered bat crack the instant the ball is struck
fn bat_crack_on_hit(
    mut commands: Commands,
    ball_q: Query<&crate::game::ball::BallState>,
    attempt: Res<crate::game::ShotAttempt>,
    handles: Option<Res<SfxHandles>>,
    settings: Res<AudioSettings>,
    mut was_struck: Local<bool>,
) {
    let Some(h) = handles else { return };
    let Ok(bs) = ball_q.single() else { return };
    let now_struck = bs.struck && !bs.dead;
    if now_struck && !*was_struck {
        // Determine tier from timing offset
        let tier_heavy = if let Some(off) = attempt.offset {
            off.abs() < 0.07
        } else {
            bs.vel.length() > 28.0
        };
        let is_edge = if let Some(off) = attempt.offset { off.abs() >= 0.19 } else { false };
        let (handle, pitch, vol) = if is_edge {
            (h.bat_edge.clone(), 1.0, settings.sfx * 0.85)
        } else if tier_heavy {
            let p = 0.94 + (bs.vel.length()/45.0).clamp(0.0, 0.18);
            (h.bat_heavy.clone(), p, settings.sfx)
        } else {
            let p = 0.98 + (bs.vel.length()/50.0).clamp(0.0, 0.14);
            (h.bat_light.clone(), p, settings.sfx * 0.92)
        };
        play_once(&mut commands, handle, vol*0.95, pitch);
    }
    *was_struck = now_struck;
}

// Pitch bounce thud (first bounce before being struck)
fn bounce_sfx(
    mut commands: Commands,
    mut ball_q: Query<(&crate::game::ball::BallState, &crate::game::ball::BallFlags)>,
    handles: Option<Res<SfxHandles>>,
    settings: Res<AudioSettings>,
) {
    let Some(h) = handles else { return };
    let Ok((bs, flags)) = ball_q.single_mut() else { return };
    if flags.just_bounced && !bs.struck && !bs.dead {
        // softer bounce, slight pitch variation by speed
        let v = bs.vel.length() / 25.0;
        let pitch = (0.92 + v*0.12).clamp(0.85, 1.15);
        play_once(&mut commands, h.bounce.clone(), settings.sfx*0.45, pitch);
    }
}

// Crowd + wicket/catch/four/six when result banner appears
fn sfx_on_result(
    mut commands: Commands,
    phase: Res<crate::game::Phase>,
    handles: Option<Res<SfxHandles>>,
    settings: Res<AudioSettings>,
    mut last_text: Local<String>,
) {
    let Some(h) = handles else { return };
    let crate::game::PhaseEnum::ResultPause { text, .. } = &phase.0 else {
        if !last_text.is_empty() { *last_text = String::new(); }
        return;
    };
    if text == &*last_text { return; }
    *last_text = text.clone();
    let upper = text.to_uppercase();

    if upper.contains("BOWLED") || upper.contains("RUN OUT") {
        play_once(&mut commands, h.wicket.clone(), settings.sfx, 1.0);
        // add a short cheer tail 0.25s later via second cheer
        play_once(&mut commands, h.cheer_four.clone(), settings.sfx*0.55, 1.05);
    } else if upper.contains("CAUGHT") || upper.contains("TAKEN") {
        play_once(&mut commands, h.catch.clone(), settings.sfx*0.85, 1.0);
        play_once(&mut commands, h.wicket.clone(), settings.sfx*0.35, 1.1);
        play_once(&mut commands, h.cheer_four.clone(), settings.sfx*0.6, 1.0);
    } else if upper.contains("SIX") || upper.contains("MAXIMUM") {
        play_once(&mut commands, h.cheer_six.clone(), settings.sfx, 1.0);
    } else if upper.contains("FOUR") {
        play_once(&mut commands, h.cheer_four.clone(), settings.sfx, 1.02);
    } else if upper.contains("WIDE") {
        play_once(&mut commands, h.ui_fallback.clone(), settings.sfx*0.45, 1.25);
    } else if upper.contains("BEATEN") || upper.contains("EDGE") {
        // edge already had bat_edge; add subtle gasp via short cheer
        play_once(&mut commands, h.cheer_four.clone(), settings.sfx*0.25, 1.15);
    }
}

// Menu UI feedback with distinct Kenney sounds
fn ui_feedback(
    mut commands: Commands,
    input: Res<crate::input::PlayerInput>,
    handles: Option<Res<UiHandles>>,
    fallback: Option<Res<SfxHandles>>,
    settings: Res<AudioSettings>,
    state: Res<State<crate::state::AppState>>,
) {
    if *state.get() != crate::state::AppState::Menu {
        return;
    }
    let sfx = settings.sfx;
    // Prefer embedded CC0 handles, fall back to procedural
    let (h_confirm, h_back, h_nav) = if let Some(ui) = handles.as_deref() {
        (ui.confirm.clone(), ui.back.clone(), ui.nav.clone())
    } else if let Some(fb) = fallback.as_deref() {
        (fb.ui_fallback.clone(), fb.ui_fallback.clone(), fb.ui_fallback.clone())
    } else { return; };

    if input.pressed(crate::input::Action::Confirm) {
        play_once(&mut commands, h_confirm, sfx*0.70, 1.0);
    } else if input.pressed(crate::input::Action::Cancel) {
        play_once(&mut commands, h_back, sfx*0.65, 1.0);
    } else if input.pressed(crate::input::Action::Next) || input.pressed(crate::input::Action::Prev) {
        // slight pitch variation to avoid monotony
        let pitch = if input.pressed(crate::input::Action::Next) { 1.0 } else { 0.96 };
        play_once(&mut commands, h_nav, sfx*0.45, pitch);
    } else if input.pressed(crate::input::Action::Left) || input.pressed(crate::input::Action::Right) {
        // slider adjust tick
        let pitch = if input.pressed(crate::input::Action::Right) { 1.08 } else { 0.92 };
        play_once(&mut commands, h_nav, sfx*0.35, pitch);
    }
}

//! Procedural audio engine: bat cracks, stump clatters and crowd cheers
//! synthesized as tiny WAV assets so no external files are needed.

use bevy::audio::{AudioPlayer, AudioSource, PlaybackSettings, Volume, GlobalVolume};
use bevy::prelude::*;

#[derive(Resource)]
pub struct SfxHandles {
    pub bat: Handle<AudioSource>,
    pub cheer: Handle<AudioSource>,
    pub wicket: Handle<AudioSource>,
    pub blip: Handle<AudioSource>,
    pub ambient: Handle<AudioSource>,
}

#[derive(Resource, Clone)]
pub struct AudioSettings {
    pub master: f32,
    pub sfx: f32,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self { master: 0.85, sfx: 0.9 }
    }
}

#[derive(Component)]
struct AmbientTag;

// ---- WAV generation helpers ----

fn wav_bytes(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let n = samples.len() as u32;
    let byte_rate = sample_rate * 2; // mono 16-bit
    let data_size = n * 2;
    let mut out = Vec::with_capacity(44 + data_size as usize);
    // RIFF header
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_size).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // PCM fmt size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_size.to_le_bytes());
    for &s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

fn gen_bat_crack() -> Vec<u8> {
    let sr = 22050u32;
    let dur = 0.075f32;
    let n = (sr as f32 * dur) as usize;
    let mut samples = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / sr as f32;
        // pseudo noise via LCG hash
        let hash = (i.wrapping_mul(1103515245) >> 16) & 0x7FFF;
        let noise = (hash as f32 / 16383.5 - 1.0) * 0.35;
        let env = (-t * 58.0).exp();
        let crack = noise * env;
        // low thump
        let thump = (std::f32::consts::TAU * 145.0 * t).sin() * (-t * 22.0).exp() * 0.42;
        // higher click
        let click = (std::f32::consts::TAU * 1800.0 * t).sin() * (-t * 120.0).exp() * 0.18;
        samples.push((crack + thump + click).clamp(-1.0, 1.0) * 0.92);
    }
    wav_bytes(&samples, sr)
}

fn gen_cheer(bright: bool) -> Vec<u8> {
    let sr = 22050u32;
    let dur = if bright { 1.1f32 } else { 0.75f32 };
    let n = (sr as f32 * dur) as usize;
    let mut samples = Vec::with_capacity(n);
    // bandpassed noise via moving average
    let mut buf = [0.0f32; 4];
    let mut bi = 0usize;
    for i in 0..n {
        let t = i as f32 / sr as f32;
        // envelope: attack 0.08, sustain, release 0.3
        let env = if t < 0.08 {
            t / 0.08
        } else if t > dur - 0.28 {
            ((dur - t) / 0.28).max(0.0)
        } else {
            1.0
        };
        let swell = if bright {
            0.7 + 0.3 * (t * 3.2).sin().abs()
        } else {
            0.85
        };
        let hash = (i.wrapping_mul(1664525).wrapping_add(1013904223) >> 16) & 0x7FFF;
        let white = hash as f32 / 16383.5 * 2.0 - 1.0;
        buf[bi] = white;
        bi = (bi + 1) % 4;
        let avg = (buf[0] + buf[1] + buf[2] + buf[3]) * 0.25;
        // slight pitch wobble
        let wobble = (t * 2.1).sin() * 0.08 + 1.0;
        samples.push(avg * env * swell * wobble * 0.55);
    }
    wav_bytes(&samples, sr)
}

fn gen_wicket() -> Vec<u8> {
    let sr = 22050u32;
    let dur = 0.68f32;
    let n = (sr as f32 * dur) as usize;
    let mut samples = vec![0.0f32; n];
    let knocks = [(0.0f32, 680.0), (0.11, 920.0), (0.23, 540.0)];
    for &(offset, freq) in &knocks {
        for i in 0..n {
            let t = i as f32 / sr as f32 - offset;
            if !(0.0..0.18).contains(&t) {
                continue;
            }
            let env = (-t * 26.0).exp();
            let tone = (std::f32::consts::TAU * freq * t).sin() * env * 0.55;
            // add woody click at onset
            let click = if t < 0.006 { (1.0 - t / 0.006) * 0.4 } else { 0.0 };
            samples[i] = (samples[i] + tone + click).clamp(-1.0, 1.0);
        }
    }
    // normalize a bit
    for s in &mut samples {
        *s *= 0.9;
    }
    wav_bytes(&samples, sr)
}

fn gen_blip() -> Vec<u8> {
    let sr = 22050u32;
    let dur = 0.09f32;
    let n = (sr as f32 * dur) as usize;
    let mut samples = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / sr as f32;
        let env = if t < 0.015 {
            t / 0.015
        } else {
            ((dur - t) / 0.02).clamp(0.0, 1.0)
        };
        let tone = (std::f32::consts::TAU * 880.0 * t).sin() * env * 0.45;
        samples.push(tone);
    }
    wav_bytes(&samples, sr)
}

fn gen_ambient() -> Vec<u8> {
    let sr = 22050u32;
    let dur = 6.0f32;
    let n = (sr as f32 * dur) as usize;
    let mut samples = Vec::with_capacity(n);
    // Brown-ish murmur: random walk
    let mut b = 0.0f32;
    for i in 0..n {
        let hash = (i.wrapping_mul(1103515245).wrapping_add(12345) >> 16) & 0x7FFF;
        let white = hash as f32 / 16383.5 * 2.0 - 1.0;
        b += white * 0.018;
        b = (b * 0.985).clamp(-1.0, 1.0);
        // very low volume, loopable-ish (fade in/out 0.3s)
        let t = i as f32 / sr as f32;
        let fade = (t / 0.3).min(1.0) * ((dur - t) / 0.3).min(1.0);
        samples.push(b * 0.14 * fade);
    }
    wav_bytes(&samples, sr)
}

// ---- Plugin + systems ----

pub struct AudioPlugin;

impl Plugin for AudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AudioSettings>()
            .add_systems(Startup, setup_sfx)
            .add_systems(
                Update,
                (
                    bat_crack_on_hit,
                    sfx_on_result,
                    menu_blip,
                    ambient_control,
                    apply_volumes,
                ),
            );
    }
}

fn setup_sfx(
    mut commands: Commands,
    mut assets: ResMut<Assets<AudioSource>>,
    mut global_vol: ResMut<GlobalVolume>,
) {
    let h_bat = assets.add(AudioSource {
        bytes: gen_bat_crack().into(),
    });
    let h_cheer = assets.add(AudioSource {
        bytes: gen_cheer(true).into(),
    });
    let h_wicket = assets.add(AudioSource {
        bytes: gen_wicket().into(),
    });
    let h_blip = assets.add(AudioSource {
        bytes: gen_blip().into(),
    });
    let h_ambient = assets.add(AudioSource {
        bytes: gen_ambient().into(),
    });
    commands.insert_resource(SfxHandles {
        bat: h_bat,
        cheer: h_cheer,
        wicket: h_wicket,
        blip: h_blip,
        ambient: h_ambient.clone(),
    });
    // Global master volume
    global_vol.volume = Volume::Linear(0.85);
    // Spawn ambient loop (low, constant murmur)
    commands.spawn((
        AmbientTag,
        AudioPlayer(h_ambient),
        PlaybackSettings {
            mode: bevy::audio::PlaybackMode::Loop,
            volume: Volume::Linear(0.18),
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
    // In menus keep ambient very low; in match bring it up a touch.
    let target = if *state.get() == crate::state::AppState::Menu {
        0.10
    } else {
        0.20
    } * settings.sfx;
    for mut s in &mut q {
        s.volume = Volume::Linear(target);
    }
}

fn play_once(
    commands: &mut Commands,
    handle: Handle<AudioSource>,
    sfx_vol: f32,
    pitch: f32,
) {
    commands.spawn((
        AudioPlayer(handle),
        PlaybackSettings {
            mode: bevy::audio::PlaybackMode::Despawn,
            volume: Volume::Linear((sfx_vol * 0.9).clamp(0.0, 1.0)),
            speed: pitch,
            ..default()
        },
    ));
}

// Bat crack the instant the ball is struck
fn bat_crack_on_hit(
    mut commands: Commands,
    ball_q: Query<&crate::game::ball::BallState>,
    handles: Option<Res<SfxHandles>>,
    settings: Res<AudioSettings>,
    mut was_struck: Local<bool>,
) {
    let Some(handles) = handles else { return };
    let Ok(bs) = ball_q.single() else { return };
    let now_struck = bs.struck && !bs.dead;
    if now_struck && !*was_struck {
        let pitch = 0.92 + (bs.vel.length() / 45.0).clamp(0.0, 0.22);
        play_once(&mut commands, handles.bat.clone(), settings.sfx, pitch);
    }
    *was_struck = now_struck;
}

// Cheers / wicket clatter when the result banner appears
fn sfx_on_result(
    mut commands: Commands,
    phase: Res<crate::game::Phase>,
    handles: Option<Res<SfxHandles>>,
    settings: Res<AudioSettings>,
    mut last_text: Local<String>,
) {
    let Some(handles) = handles else { return };
    let crate::game::PhaseEnum::ResultPause { text, .. } = &phase.0 else {
        if !last_text.is_empty() {
            *last_text = String::new();
        }
        return;
    };
    if text == &*last_text {
        return;
    }
    *last_text = text.clone();
    let upper = text.to_uppercase();
    if upper.contains("BOWLED")
        || upper.contains("CAUGHT")
        || upper.contains("TAKEN")
        || upper.contains("RUN OUT")
    {
        play_once(&mut commands, handles.wicket.clone(), settings.sfx, 1.0);
        // follow with a cheer a moment later is handled by the same banner;
        // the wicket sample already has crowd in it, so single play is enough.
    } else if upper.contains("SIX") || upper.contains("MAXIMUM") {
        play_once(&mut commands, handles.cheer.clone(), settings.sfx * 1.0, 1.02);
    } else if upper.contains("FOUR") {
        play_once(&mut commands, handles.cheer.clone(), settings.sfx * 0.75, 1.08);
    } else if upper.contains("WIDE") {
        // soft blip for extras
        play_once(&mut commands, handles.blip.clone(), settings.sfx * 0.5, 1.2);
    }
}

fn menu_blip(
    mut commands: Commands,
    input: Res<crate::input::PlayerInput>,
    handles: Option<Res<SfxHandles>>,
    settings: Res<AudioSettings>,
    state: Res<State<crate::state::AppState>>,
) {
    let Some(handles) = handles else { return };
    if *state.get() != crate::state::AppState::Menu {
        return;
    }
    if input.pressed(crate::input::Action::Confirm)
        || input.pressed(crate::input::Action::Cancel)
        || input.pressed(crate::input::Action::Next)
        || input.pressed(crate::input::Action::Prev)
    {
        play_once(&mut commands, handles.blip.clone(), settings.sfx * 0.6, 1.0);
    }
}

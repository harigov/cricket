//! Top-level application states.

use bevy::prelude::*;

#[derive(States, Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum AppState {
    /// Menus: main menu, setup wizard, controls, tournament bracket.
    #[default]
    Menu,
    /// Live match simulation.
    InMatch,
}

/// True while the player has opened the in-match pause overlay (Esc).
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq)]
pub struct MatchPaused(pub bool);

/// Gameplay systems (physics, AI, timers) freeze while paused; rendering continues.
pub fn not_paused(paused: Option<Res<MatchPaused>>) -> bool {
    !paused.map(|p| p.0).unwrap_or(false)
}

/// Fired when the match scene must be torn down and rebuilt
/// (innings change).
#[derive(Message)]
pub struct RebuildScene;

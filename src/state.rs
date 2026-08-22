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

/// Fired when the match scene must be torn down and rebuilt
/// (innings change).
#[derive(Message)]
pub struct RebuildScene;

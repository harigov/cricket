//! Top-level application states.

use bevy::prelude::*;

#[derive(States, Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum AppState {
    #[default]
    MainMenu,
    /// Team/format/stadium selection before a match.
    Setup,
    /// Live match simulation.
    InMatch,
    /// Between-innings screen (part of InMatch phases, kept for future).
    InningsBreak,
    /// Post-match result display.
    MatchResult,
    /// Tournament bracket overview.
    Tournament,
    ControlsHelp,
}

/// Fired when the match scene must be torn down and rebuilt
/// (innings change, new tournament match, etc.).
#[derive(Message)]
pub struct RebuildScene;

/// Fired to tear down the scene when leaving the match entirely.
#[derive(Message)]
pub struct TeardownScene;

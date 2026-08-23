//! Surroundings rendered outside the stadium bowl.
//!
//! Every ground sits in a themed world (see [`StadiumEnvironment`]): a downtown
//! skyline, an alpine valley, an island coast, and so on. [`spawn_environment`]
//! populates the annulus between the outer edge of the bowl and the ground disc
//! that fades into the sky.

use bevy::prelude::*;

use crate::core::stadiums::StadiumEnvironment;
use crate::render::stadium::StadiumBuildCtx;

/// Marker for every entity belonging to the themed surroundings.
#[derive(Component)]
pub struct EnvironmentProp;

/// Populate the world outside the bowl for `ctx.stadium.environment`.
pub(crate) fn spawn_environment(
    _p: &mut ChildSpawnerCommands,
    ctx: &mut StadiumBuildCtx<'_>,
    _spawn_count: &mut usize,
) {
    match ctx.stadium.environment {
        StadiumEnvironment::Metropolis
        | StadiumEnvironment::Alpine
        | StadiumEnvironment::Coastal
        | StadiumEnvironment::Parkland
        | StadiumEnvironment::Desert => {}
    }
}

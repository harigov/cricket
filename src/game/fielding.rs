//! Fielder entities, placement and chase behaviour.

use crate::core::geometry::{self, FieldPos};
use crate::game::ball::CricketBall;
use bevy::prelude::*;

#[derive(Component)]
pub struct Fielder {
    pub slot: usize,
    pub is_keeper: bool,
    pub label: &'static str,
}

/// Movement brain for a fielder.
#[derive(Component)]
pub enum Brain {
    /// Standing at the assigned post.
    AtPost,
    /// Chasing the ball's projected position.
    Chase,
    /// Walking back to post after collecting.
    Return,
}

pub const FIELDER_SPEED: f32 = 8.2;
const KEEPER_SPEED: f32 = 9.0;

/// Spawn the fielding side (keeper + 10 fielders). The bowler figure is
/// managed separately by the match flow.
#[allow(clippy::too_many_arguments)]
pub fn spawn_field_side(
    commands: &mut Commands,
    asset_server: &AssetServer,
    layout: &[FieldPos],
    shirt: Color,
    trousers: Color,
) -> Vec<Entity> {
    let mut out = Vec::new();
    for (slot, fp) in layout.iter().enumerate() {
        let pos = fp.world_pos(geometry::BATSMAN_POS);
        let facing = {
            // Face the striker.
            let d = geometry::BATSMAN_POS - pos;
            d.y.atan2(d.x).to_degrees() - 90.0
        };
        let is_keeper = slot == 0;
        let e = crate::render::player::spawn_figure(
            commands,
            asset_server,
            Vec3::new(pos.x, 0.0, pos.y),
            facing,
            shirt,
            trousers,
            if is_keeper {
                crate::render::player::FigureKind::Keeper
            } else {
                crate::render::player::FigureKind::Fielder(slot)
            },
        );
        commands.entity(e).insert((
            Fielder { slot, is_keeper, label: fp.name },
            Brain::AtPost,
        ));
        out.push(e);
    }
    out
}

/// Snap everyone back to their posts (start of delivery).
pub fn reset_brains(mut brains: Query<&mut Brain>) {
    for mut b in &mut brains {
        if !matches!(*b, Brain::AtPost) {
            *b = Brain::AtPost;
        }
    }
}

/// Move chasing fielders toward the live ball, predicting its path.
pub fn chase_system(
    time: Res<Time>,
    ball_q: Query<&crate::game::ball::BallState, With<CricketBall>>,
    mut fielders: Query<(&Fielder, &Brain, &mut Transform)>,
) {
    let Ok(ball) = ball_q.single() else { return };
    if ball.dead && ball.vel.length_squared() < 0.01 {
        return;
    }
    let dt = time.delta_secs();
    for (f, brain, mut tf) in &mut fielders {
        if !matches!(brain, Brain::Chase) {
            continue;
        }
        let speed = if f.is_keeper { KEEPER_SPEED } else { FIELDER_SPEED };
        // Predict where the ball will be when we get there (simple lead)
        let to_ball = Vec2::new(ball.pos.x - tf.translation.x, ball.pos.z - tf.translation.z);
        let dist = to_ball.length();
        if dist < 0.05 {
            continue;
        }
        let t_intercept = (dist / speed).clamp(0.0, 0.65);
        // Only predict horizontal motion; vertical is irrelevant for ground chase
        let pred = Vec2::new(
            ball.pos.x + ball.vel.x * t_intercept * 0.55,
            ball.pos.z + ball.vel.z * t_intercept * 0.55,
        );
        let to_pred = Vec2::new(pred.x - tf.translation.x, pred.y - tf.translation.z);
        let d2 = to_pred.length();
        if d2 < 0.05 {
            continue;
        }
        let step = (speed * dt * 1.4).min(d2);
        let dir = to_pred / d2;
        tf.translation.x += dir.x * step;
        tf.translation.z += dir.y * step;
        tf.rotation = Quat::from_rotation_y(crate::render::player::yaw_to_face(dir));
    }
}

/// Pitch surface behaviour. Affects bounce height, seam movement and spin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PitchType {
    /// Extra bounce and seam for fast bowlers.
    Green,
    /// True, even bounce; best for batting.
    Hard,
    /// Low bounce, big turn for spinners.
    Dusty,
    /// Wearing: variable bounce as the match goes on (approximated).
    Dry,
}

impl PitchType {
    pub fn label(&self) -> &'static str {
        match self {
            PitchType::Green => "Green top",
            PitchType::Hard => "Hard & true",
            PitchType::Dusty => "Dusty turner",
            PitchType::Dry => "Dry & cracking",
        }
    }

    /// Multiplier on vertical restitution when the ball bounces.
    pub fn bounce_mul(&self) -> f32 {
        match self {
            PitchType::Green => 1.12,
            PitchType::Hard => 1.0,
            PitchType::Dusty => 0.82,
            PitchType::Dry => 0.9,
        }
    }

    /// Multiplier on lateral deviation off the pitch.
    pub fn turn_mul(&self) -> f32 {
        match self {
            PitchType::Green => 0.8,
            PitchType::Hard => 0.6,
            PitchType::Dusty => 1.5,
            PitchType::Dry => 1.25,
        }
    }

    /// Multiplier on pace retained through the bounce (slower pitches grip).
    pub fn grip_mul(&self) -> f32 {
        match self {
            PitchType::Green => 0.95,
            PitchType::Hard => 1.0,
            PitchType::Dusty => 0.85,
            PitchType::Dry => 0.9,
        }
    }
}

/// The world the stadium sits in. Drives the surroundings, sky and ground tint
/// rendered outside the bowl (see `crate::render::environment`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StadiumEnvironment {
    /// Dense downtown: towers and skyscrapers crowd the skyline.
    Metropolis,
    /// Snow-capped peaks ringing an alpine valley.
    Alpine,
    /// Island ground with ocean, beach and palms on the seaward side.
    Coastal,
    /// Leafy suburban county ground: houses, hedgerows and mature trees.
    Parkland,
    /// Arid plateau: dust haze, scrub and distant mesas.
    Desert,
}

impl StadiumEnvironment {
    /// Every theme, so renderers and tests can cover the set exhaustively.
    pub const ALL: [StadiumEnvironment; 5] = [
        StadiumEnvironment::Metropolis,
        StadiumEnvironment::Alpine,
        StadiumEnvironment::Coastal,
        StadiumEnvironment::Parkland,
        StadiumEnvironment::Desert,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            StadiumEnvironment::Metropolis => "City centre",
            StadiumEnvironment::Alpine => "Mountain valley",
            StadiumEnvironment::Coastal => "Island coast",
            StadiumEnvironment::Parkland => "Country parkland",
            StadiumEnvironment::Desert => "Desert plateau",
        }
    }
}

/// A playable stadium.
#[derive(Clone, Debug)]
pub struct Stadium {
    pub name: String,
    pub city: String,
    /// Boundary radius in metres.
    pub boundary_radius: f32,
    pub pitch: PitchType,
    /// Stand/roof tint used by the renderer.
    pub stand_color: bevy::color::Color,
    pub outfield_color: bevy::color::Color,
    /// Surroundings theme rendered outside the bowl.
    pub environment: StadiumEnvironment,
}

impl Stadium {
    pub fn boundary_radius(&self) -> f32 {
        self.boundary_radius
    }
}

/// Built-in stadium catalogue.
///
/// Each ground is deliberately paired with a different `StadiumEnvironment` so the
/// tournament never plays two matches against the same skyline.
pub fn builtin_stadiums() -> Vec<Stadium> {
    vec![
        Stadium {
            name: "Harbour Oval".into(),
            city: "Port Kemble".into(),
            boundary_radius: 62.0,
            pitch: PitchType::Hard,
            stand_color: bevy::color::Color::srgb_u8(0x3E, 0x4A, 0x5A),
            outfield_color: bevy::color::Color::srgb_u8(0x2F, 0x7D, 0x32),
            environment: StadiumEnvironment::Coastal,
        },
        Stadium {
            name: "Rose Bowl Gardens".into(),
            city: "Alderton".into(),
            boundary_radius: 68.0,
            pitch: PitchType::Green,
            stand_color: bevy::color::Color::srgb_u8(0x6B, 0x4A, 0x2F),
            outfield_color: bevy::color::Color::srgb_u8(0x35, 0x82, 0x36),
            environment: StadiumEnvironment::Parkland,
        },
        Stadium {
            name: "Fortress Arena".into(),
            city: "Qasimabad".into(),
            boundary_radius: 66.0,
            pitch: PitchType::Dusty,
            stand_color: bevy::color::Color::srgb_u8(0x2E, 0x7D, 0x6B),
            outfield_color: bevy::color::Color::srgb_u8(0x3B, 0x83, 0x38),
            environment: StadiumEnvironment::Desert,
        },
        Stadium {
            name: "Highveld Dome".into(),
            city: "New Meridian".into(),
            boundary_radius: 60.0,
            pitch: PitchType::Dry,
            stand_color: bevy::color::Color::srgb_u8(0x55, 0x55, 0x66),
            outfield_color: bevy::color::Color::srgb_u8(0x44, 0x8C, 0x33),
            environment: StadiumEnvironment::Metropolis,
        },
        Stadium {
            name: "Meridian Sky Park".into(),
            city: "Calverton".into(),
            boundary_radius: 64.0,
            pitch: PitchType::Hard,
            stand_color: bevy::color::Color::srgb_u8(0x46, 0x4F, 0x63),
            outfield_color: bevy::color::Color::srgb_u8(0x31, 0x80, 0x35),
            environment: StadiumEnvironment::Metropolis,
        },
        Stadium {
            name: "Glacier Field".into(),
            city: "Thornevale".into(),
            boundary_radius: 65.0,
            pitch: PitchType::Green,
            stand_color: bevy::color::Color::srgb_u8(0x5C, 0x66, 0x72),
            outfield_color: bevy::color::Color::srgb_u8(0x33, 0x85, 0x3B),
            environment: StadiumEnvironment::Alpine,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_environment_theme_is_playable() {
        let stadiums = builtin_stadiums();
        for theme in StadiumEnvironment::ALL {
            assert!(
                stadiums.iter().any(|s| s.environment == theme),
                "no stadium uses {theme:?}"
            );
        }
    }
}

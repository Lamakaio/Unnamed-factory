//! The world's parameters (`docs/evolution.md` §2): its age, star, orbit, atmosphere, ocean,
//! surface and whether there is life. They set what the climate computes (`climate()`), and
//! later what the generator builds and the game draws. Earth's by default; presets for other
//! states (`World::virgin`).

use crate::climate::ClimateSettings;

/// The world's parameters.
#[derive(Clone, Debug)]
pub struct World {
    /// Age of the planet and its star (Gyr).
    pub age: f32,
    pub star: Star,
    pub orbit: Orbit,
    pub atmosphere: Atmosphere,
    pub ocean: Ocean,
    pub albedo: Albedo,
    /// Life on land (vegetation). Until the life parameters are designed: none or today's.
    pub life: bool,
}

/// The star.
#[derive(Clone, Copy, Debug)]
pub struct Star {
    /// Surface temperature (K): the light's colour (5772 for the Sun), and how long the star
    /// lives (cooler stars are smaller and brighten far more slowly).
    pub temperature: f32,
    /// Sunlight on the planet (share of Earth's today) when the star is as far through its
    /// life as the Sun is today; it brightens with age (`Star::irradiance`).
    pub brightness: f32,
}

/// The planet's orbit and spin.
#[derive(Clone, Copy, Debug)]
pub struct Orbit {
    /// Year length (Earth years).
    pub year: f32,
    /// Eccentricity (0: circular; Earth 0.0167, Mars 0.093).
    pub eccentricity: f32,
    /// Where the planet is in its seasons at its closest to the star: the star's longitude
    /// (degrees: 0 northern spring equinox, 90 northern summer solstice, 270 northern winter
    /// solstice; Earth ~283, early January).
    pub perihelion: f32,
    /// Axial tilt (degrees).
    pub tilt: f32,
    /// Day length (hours).
    pub day: f32,
}

/// The atmosphere's gases and haze.
#[derive(Clone, Copy, Debug)]
pub struct Atmosphere {
    /// Carbon dioxide and methane (ppm).
    pub co2: f32,
    pub ch4: f32,
    /// Oxygen (share of the air; Earth 0.21).
    pub o2: f32,
    /// Dust and aerosols (0 clear .. 1 very hazy: volcanic ash, desert dust).
    pub dust: f32,
}

/// The ocean.
#[derive(Clone, Copy, Debug)]
pub struct Ocean {
    /// Strength of the ocean currents' warming and cooling of the coasts (1: Earth's).
    pub currents: f32,
}

/// Surface albedos (share of the light reflected) of the ground types.
#[derive(Clone, Copy, Debug)]
pub struct Albedo {
    pub sea: f32,
    /// Bare rock and regolith.
    pub rock: f32,
    /// Dry ground: sand and desert pavement.
    pub desert: f32,
    pub plants: f32,
    /// Snow and ice.
    pub ice: f32,
}

impl Default for World {
    /// Earth today.
    fn default() -> Self {
        Self {
            age: SUN_AGE,
            star: Star {
                temperature: 5772.,
                brightness: 1.,
            },
            orbit: Orbit {
                year: 1.,
                eccentricity: 0.0167,
                perihelion: 283.,
                tilt: 23.44,
                day: 24.,
            },
            atmosphere: Atmosphere {
                co2: 280.,
                ch4: 0.7,
                o2: 0.21,
                dust: 0.1,
            },
            ocean: Ocean { currents: 1. },
            albedo: Albedo {
                sea: 0.06,
                rock: 0.2,
                desert: 0.35,
                plants: 0.15,
                ice: 0.6,
            },
            life: true,
        }
    }
}

/// The Sun's age (Gyr).
const SUN_AGE: f32 = 4.57;
/// The Sun's lifetime on the main sequence (Gyr).
const SUN_LIFETIME: f32 = 10.;
/// How much fainter a star is at its birth than at the Sun's present point in its life
/// (Gough 1981: L(t) = L☉ / (1 + 2/5 (1 − t/t☉))).
const YOUNG_FAINTNESS: f32 = 0.4;
/// Earth's sunlight (W/m²).
const SOLAR_CONSTANT: f32 = 1361.;
/// CO₂'s forcing beyond its logarithm (W/m² per ln² of the ratio to today's).
const HIGH_CO2: f32 = 0.39;
/// Today's CO₂ and CH₄ (ppm) for the forcing.
const REFERENCE_CO2: f32 = 280.;
const REFERENCE_CH4: f32 = 0.7;
/// Organic haze forms when CH₄/CO₂ exceeds about 0.1 (Archean Earth): it reflects sunlight
/// (anti-greenhouse) and tints the sky orange. Ratio over which it thickens, and the
/// albedo it adds when thick.
const HAZE_RATIO: (f32, f32) = (0.1, 0.3);
const HAZE_ALBEDO: f32 = 0.08;
/// Albedo added by dust and aerosols at `dust` 1.
const DUST_ALBEDO: f32 = 0.06;

impl World {
    /// A young planet without life: a fainter star, CO₂-rich air (a few tenths of a bar,
    /// as estimates for the young Earth: enough to keep the seas from freezing), no oxygen,
    /// bare ground.
    pub fn virgin() -> Self {
        let mut world = Self {
            age: 0.5,
            ..Self::default()
        };
        world.atmosphere = Atmosphere {
            co2: 300_000.,
            ch4: 0.,
            o2: 0.,
            dust: 0.2,
        };
        world.life = false;
        world
    }

    /// A preset by name (`earth`, `virgin`).
    pub fn preset(name: &str) -> Option<Self> {
        match name {
            "earth" => Some(Self::default()),
            "virgin" => Some(Self::virgin()),
            _ => None,
        }
    }

    /// A world from a description: a preset, then parameters changed, separated by commas
    /// (`virgin,day=240,eccentricity=0.2`; parameter names as in `World::set`).
    pub fn parse(description: &str) -> Result<Self, String> {
        let mut parts = description.split(',').map(str::trim);
        let name = parts.next().unwrap_or("earth");
        let mut world = Self::preset(name).ok_or(format!("no world preset {name}"))?;
        for part in parts {
            let (key, value) = part
                .split_once('=')
                .ok_or(format!("{part}: expected name=value"))?;
            let value: f32 = value.parse().map_err(|_| format!("{part}: not a number"))?;
            world.set(key, value)?;
        }
        Ok(world)
    }

    /// Sets a parameter by name (the fields' names; `life` 0 or 1).
    pub fn set(&mut self, key: &str, value: f32) -> Result<(), String> {
        let field = match key {
            "age" => &mut self.age,
            "star_temperature" => &mut self.star.temperature,
            "brightness" => &mut self.star.brightness,
            "year" => &mut self.orbit.year,
            "eccentricity" => &mut self.orbit.eccentricity,
            "perihelion" => &mut self.orbit.perihelion,
            "tilt" => &mut self.orbit.tilt,
            "day" => &mut self.orbit.day,
            "co2" => &mut self.atmosphere.co2,
            "ch4" => &mut self.atmosphere.ch4,
            "o2" => &mut self.atmosphere.o2,
            "dust" => &mut self.atmosphere.dust,
            "currents" => &mut self.ocean.currents,
            "albedo_sea" => &mut self.albedo.sea,
            "albedo_rock" => &mut self.albedo.rock,
            "albedo_desert" => &mut self.albedo.desert,
            "albedo_plants" => &mut self.albedo.plants,
            "albedo_ice" => &mut self.albedo.ice,
            "life" => {
                self.life = value > 0.5;
                return Ok(());
            }
            _ => return Err(format!("no world parameter {key}")),
        };
        *field = value;
        Ok(())
    }

    /// The climate's parameters.
    pub fn climate(&self) -> ClimateSettings {
        let atmosphere = &self.atmosphere;
        ClimateSettings {
            tilt: self.orbit.tilt,
            solar: SOLAR_CONSTANT * self.star.irradiance(self.age),
            greenhouse: self.greenhouse(),
            year: self.orbit.year,
            eccentricity: self.orbit.eccentricity,
            perihelion: self.orbit.perihelion,
            day: self.orbit.day,
            haze: self.haze() * HAZE_ALBEDO + atmosphere.dust * DUST_ALBEDO,
            albedo: self.albedo,
            life: self.life,
            currents: self.ocean.currents,
            n: crate::climate::GRID_CELLS,
            meters_per_unit: crate::climate::METERS_PER_UNIT,
        }
    }

    /// Greenhouse forcing of CO₂ and CH₄ against Earth's pre-industrial air (W/m²): the
    /// simplified expressions of Myhre et al. 1998, with CO₂'s growing faster than its
    /// logarithm at high levels (fitted to the 38 W/m² at 50 000 ppm of Byrne & Goldblatt
    /// 2014; beyond, as for an early CO₂-rich atmosphere, only indicative).
    pub fn greenhouse(&self) -> f32 {
        let x = (self.atmosphere.co2.max(1.) / REFERENCE_CO2).ln();
        let co2 = 5.35 * x + HIGH_CO2 * x.max(0.).powi(2);
        let ppb = |ppm: f32| (ppm * 1000.).max(0.).sqrt();
        let ch4 = 0.036 * (ppb(self.atmosphere.ch4) - ppb(REFERENCE_CH4));
        co2 + ch4
    }

    /// How thick the organic haze is (0..1).
    pub fn haze(&self) -> f32 {
        let ratio = self.atmosphere.ch4 / self.atmosphere.co2.max(1.);
        crate::noise::smoothstep(HAZE_RATIO.0, HAZE_RATIO.1, ratio)
    }
}

impl Star {
    /// Main-sequence lifetime (Gyr): with mass roughly as temperature², lifetime as
    /// mass^-2.5.
    pub fn lifetime(&self) -> f32 {
        let mass = (self.temperature / 5772.).powi(2);
        SUN_LIFETIME * mass.powf(-2.5)
    }

    /// Sunlight on the planet at an age (share of Earth's today): Gough's brightening,
    /// stretched over the star's lifetime.
    pub fn irradiance(&self, age: f32) -> f32 {
        let scale = self.lifetime() / SUN_LIFETIME;
        // (Only for the main sequence: past about twice the Sun's age the formula runs away.)
        let through =
            |t: f32| 1. / (1. + YOUNG_FAINTNESS * (1. - (t / (SUN_AGE * scale)).min(2.2)));
        self.brightness * through(age) / through(SUN_AGE * scale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn earth_today() {
        let world = World::default();
        assert!((world.star.irradiance(world.age) - 1.).abs() < 1e-4);
        assert!(world.greenhouse().abs() < 1e-3);
        // The young Sun: about 70 % of today's light.
        let young = world.star.irradiance(0.);
        assert!((0.69..0.73).contains(&young), "{young}");
    }
}

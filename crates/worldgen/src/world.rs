//! The world's parameters (`docs/evolution.md` §2): its age, star, orbit, atmosphere, ocean,
//! surface and whether there is life. They set what the climate computes (`climate()`), and
//! later what the generator builds and the game draws. Earth's by default; presets for other
//! states (`World::virgin`, `World::oxidised`).

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
    pub geology: Geology,
    pub ground: Ground,
    /// The moons: tides, the night sky (and, later, a steady tilt).
    pub moons: Vec<Moon>,
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
    /// Salinity (‰; Earth 35): saltier seas freeze colder.
    pub salinity: f32,
    /// Dissolved iron (0..1): seas without oxygen keep their iron, and look green to brown.
    pub iron: f32,
    /// Amount of water (share of Earth's): sets how much of the planet is land, from a
    /// desert world with small seas to an ocean world.
    pub water: f32,
}

/// What shapes the planet's rock (the age does too: `World::interior_heat`,
/// `World::continental_growth`).
#[derive(Clone, Copy, Debug)]
pub struct Geology {
    /// How gathered the continents are: 0 scattered (as the noise lays them), 1 one
    /// supercontinent.
    pub assembly: f32,
    /// Impact rate (share of the Solar System's at the same age): craters.
    pub impacts: f32,
}

/// A moon.
#[derive(Clone, Copy, Debug)]
pub struct Moon {
    /// Mass and distance (Earth's Moon's: 1).
    pub mass: f32,
    pub distance: f32,
}

/// The ground.
#[derive(Clone, Copy, Debug)]
pub struct Ground {
    /// Soil depth (1: Earth's): thin soil leaves rock showing and creeps less down slopes.
    pub soil: f32,
    /// Erosion rate (1: Earth's): rivers cut faster, craters wear away sooner.
    pub erosion: f32,
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
            ocean: Ocean {
                currents: 1.,
                salinity: 35.,
                iron: 0.,
                water: 1.,
            },
            geology: Geology {
                assembly: 0.,
                impacts: 1.,
            },
            ground: Ground {
                soil: 1.,
                erosion: 1.,
            },
            moons: vec![Moon {
                mass: 1.,
                distance: 1.,
            }],
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
/// Interior heat: its e-folding time (Gyr).
const HEAT_DECAY: f32 = 3.;
/// Continental growth: its e-folding time (Gyr).
const CONTINENT_GROWTH: f32 = 1.2;
/// How land share answers the amount of water (see `World::land_fraction`).
const WATER_LAND: f32 = 0.6;
/// The Sun's tide against the Moon's, on Earth.
const SOLAR_TIDE: f32 = 0.46;
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
        world.ocean.iron = 0.8;
        // (Regolith only: no soil without life.)
        world.ground.soil = 0.25;
        world.life = false;
        world
    }

    /// After the first oxygen: a little in the air (rusting the ground red), the sea's iron
    /// gone, CO₂ lower, the star brighter; still no life on land.
    pub fn oxidised() -> Self {
        let mut world = Self::virgin();
        world.age = 2.5;
        world.atmosphere.co2 = 10_000.;
        world.atmosphere.o2 = 0.02;
        world.atmosphere.dust = 0.15;
        world.ocean.iron = 0.1;
        world
    }

    /// A preset by name (`earth`, `virgin`, `oxidised`).
    pub fn preset(name: &str) -> Option<Self> {
        match name {
            "earth" => Some(Self::default()),
            "virgin" => Some(Self::virgin()),
            "oxidised" => Some(Self::oxidised()),
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
            "salinity" => &mut self.ocean.salinity,
            "iron" => &mut self.ocean.iron,
            "water" => &mut self.ocean.water,
            "assembly" => &mut self.geology.assembly,
            "impacts" => &mut self.geology.impacts,
            "soil" => &mut self.ground.soil,
            "erosion" => &mut self.ground.erosion,
            // (The moons: how many (copies of the first, each further out), and the first's
            // mass and distance.)
            "moons" => {
                let first = self.moons.first().copied().unwrap_or(Moon {
                    mass: 1.,
                    distance: 1.,
                });
                self.moons = (0..value.max(0.).round() as usize)
                    .map(|k| Moon {
                        distance: first.distance * 1.6f32.powi(k as i32),
                        ..first
                    })
                    .collect();
                return Ok(());
            }
            "moon_mass" | "moon_distance" => {
                if self.moons.is_empty() {
                    return Err("no moon: set moons first".into());
                }
                let moon = &mut self.moons[0];
                if key == "moon_mass" {
                    moon.mass = value;
                } else {
                    moon.distance = value;
                }
                return Ok(());
            }
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

    /// How oxidised the ground is (0..1): rusty red once there is oxygen in the air (red
    /// beds appeared on Earth with its first oxygen, around 1 % of today's), grey to beige
    /// before.
    pub fn oxidation(&self) -> f32 {
        crate::noise::smoothstep(0.0005, 0.01, self.atmosphere.o2)
    }

    /// How active the interior is against Earth's today (volcanoes): radiogenic heat and the
    /// heat left from formation decay over billions of years (a young planet's mantle is
    /// several times hotter in heat flow; softened here: volcanoes don't scale with it).
    pub fn interior_heat(&self) -> f32 {
        ((SUN_AGE - self.age) / HEAT_DECAY)
            .exp()
            .sqrt()
            .clamp(0.3, 2.5)
    }

    /// Continental crust grown so far against Earth's today: continents grow fast early
    /// and slowly later (about two thirds of today's by 3 Gyr ago: Dhuime et al. 2012).
    pub fn continental_growth(&self) -> f32 {
        let grown = |age: f32| 1. - (-age / CONTINENT_GROWTH).exp();
        (grown(self.age) / grown(SUN_AGE)).clamp(0.15, 1.15)
    }

    /// Share of the planet that is land, for a planet that would have `earth_land` with
    /// Earth's water and continents: less with smaller continents, more with less water
    /// (the sea draining off the shelves and into the basins), less with more.
    pub fn land_fraction(&self, earth_land: f32) -> f32 {
        let land = earth_land * self.continental_growth();
        (1. - (1. - land) * self.ocean.water.max(0.).powf(WATER_LAND)).clamp(0.02, 0.95)
    }

    /// The impact rate at an age (craters per unit time against the Solar System's today):
    /// the lunar chronology's decaying early bombardment over a constant rate (Neukum et al.
    /// 2001: N(1 km) = 5.44e-14 (e^6.93T − 1) + 8.38e-4 T, T in Gyr before today).
    pub fn impact_flux(&self, age: f32) -> f32 {
        let before = (SUN_AGE - age).max(0.);
        self.geology.impacts * (1. + 6.93 * 5.44e-14 / 8.38e-4 * (6.93 * before).exp())
    }

    /// Tide-raising force against Earth's (1): each moon's mass / distance³, and the star's
    /// (on Earth about 0.46 of the Moon's; its mass / distance³ goes as 1 / year² by Kepler's
    /// third law).
    pub fn tidal_forcing(&self) -> f32 {
        let moons: f32 = self
            .moons
            .iter()
            .map(|m| m.mass / m.distance.max(0.05).powi(3))
            .sum();
        (moons + SOLAR_TIDE / self.orbit.year.max(0.01).powi(2)) / (1. + SOLAR_TIDE)
    }

    /// Freezing point of the sea (°C): about −0.054 °C per ‰ of salt.
    pub fn sea_freezing(&self) -> f32 {
        -0.054 * self.ocean.salinity
    }

    /// How thick the organic haze is (0..1).
    pub fn haze(&self) -> f32 {
        let ratio = self.atmosphere.ch4 / self.atmosphere.co2.max(1.);
        crate::noise::smoothstep(HAZE_RATIO.0, HAZE_RATIO.1, ratio)
    }
}

impl Star {
    /// The colour of its light (linear RGB, the Sun's white): from its temperature, with
    /// Tanner Helland's fit of black-body colours (sRGB), divided by the Sun's.
    pub fn color(&self) -> [f32; 3] {
        let black_body = |kelvin: f32| {
            let t = kelvin.clamp(1000., 40000.) / 100.;
            let r = if t <= 66. {
                255.
            } else {
                329.699 * (t - 60.).powf(-0.133_205)
            };
            let g = if t <= 66. {
                99.4708 * t.ln() - 161.12
            } else {
                288.122 * (t - 60.).powf(-0.075_515)
            };
            let b = if t >= 66. {
                255.
            } else if t <= 19. {
                0.
            } else {
                138.518 * (t - 10.).ln() - 305.045
            };
            [r, g, b].map(|c| (c.clamp(0., 255.) / 255.).powf(2.2))
        };
        let (star, sun) = (black_body(self.temperature), black_body(5772.));
        let ratio = [0, 1, 2].map(|k| star[k] / sun[k].max(1e-3));
        let brightest = ratio.iter().copied().fold(0., f32::max);
        ratio.map(|c| c / brightest)
    }

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
        // Earth's own geology unchanged.
        assert!((world.interior_heat() - 1.).abs() < 1e-5);
        assert!((world.continental_growth() - 1.).abs() < 1e-5);
        assert!((world.land_fraction(0.3) - 0.3).abs() < 1e-5);
        assert!((world.impact_flux(world.age) - 1.).abs() < 1e-3);
        assert!((world.tidal_forcing() - 1.).abs() < 1e-5);
        // The late heavy bombardment: hundreds of times today's rate 4 Gyr ago.
        let early = world.impact_flux(0.57);
        assert!((100.0..2000.0).contains(&early), "{early}");
    }
}

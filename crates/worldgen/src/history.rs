//! The world over deep time (`docs/evolution.md` E3, the part without life): the world's
//! parameters evolving from an age to another, with the events on the way.
//!
//! - **Climate**: a one-dimensional energy balance by latitude (Budyko 1969, Sellers 1969):
//!   sunlight by latitude for the tilt, outgoing longwave `A + B T` lowered by the greenhouse,
//!   heat carried toward the mean, ice albedo below freezing. Its ice-albedo feedback gives
//!   ice ages and snowballs, with hysteresis: a frozen planet stays frozen until the
//!   greenhouse is far stronger than what froze it.
//! - **Carbon**: volcanoes outgas CO₂ (more while the interior is hot); silicate weathering
//!   draws it down, faster when warm and with more CO₂ (Walker, Hays and Kasting 1981: the
//!   thermostat that keeps a planet temperate as its star brightens), on ice-free land, less
//!   on a supercontinent (dry interior), slower on bare rock than under plants. Under a
//!   snowball weathering stops and the volcanoes' CO₂ builds up until the ice melts.
//! - **Events**: large igneous provinces (pulses of outgassing), big impacts, snowballs and
//!   thaws.
//!
//! Life (oxygen, methane, plants speeding weathering) plugs in later: today `World::life`
//! only sets the weathering's pace.

use crate::{
    noise::{hash, smoothstep},
    world::World,
};

/// The world at a moment.
#[derive(Clone, Copy, Debug)]
pub struct Snapshot {
    /// Age (Gyr), CO₂ (ppm), global mean temperature (°C), the ice line (latitude in degrees
    /// down to which the ground freezes: 90 no ice, 0 all frozen).
    pub age: f32,
    pub co2: f32,
    pub temperature: f32,
    pub ice_line: f32,
    /// Outgassing and weathering (Earth's today: 1).
    pub outgassing: f32,
    pub weathering: f32,
}

/// What happened (ages in Gyr).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// Ice reached the tropics: the whole planet froze.
    Snowball { age: f32 },
    /// The ice withdrew from the tropics.
    Thaw { age: f32 },
    /// A large igneous province: outgassing this many times the usual, for `duration` (Gyr).
    Eruption {
        age: f32,
        strength: f32,
        duration: f32,
    },
    /// A big impact, its crater's radius (km).
    Impact { age: f32, radius: f32 },
}

impl Event {
    pub fn age(&self) -> f32 {
        match *self {
            Event::Snowball { age }
            | Event::Thaw { age }
            | Event::Eruption { age, .. }
            | Event::Impact { age, .. } => age,
        }
    }
}

/// A history: the world through time, the events, and the world at the end.
pub struct History {
    pub snapshots: Vec<Snapshot>,
    pub events: Vec<Event>,
    pub world: World,
}

/// Latitude bands of the energy balance (equal areas: uniform in the sine of latitude).
const BANDS: usize = 90;
/// Outgoing longwave `A + B T` (W/m², T in °C; North et al. 1981), and the heat carried
/// toward the global mean (W/m²/K: Budyko's transport), calibrated so that Earth today comes
/// out at about 15 °C with its ice line near 70°.
const OLR_A: f32 = 203.3;
const OLR_B: f32 = 2.09;
const TRANSPORT: f32 = 3.8;
/// Albedo of the air and clouds, and the share of the ground's seen through them (as the
/// climate's), and the band temperatures (°C) over which the ground freezes over.
const ATMOSPHERE_ALBEDO: f32 = 0.24;
const SURFACE_SEEN: f32 = 0.6;
const FREEZING: (f32, f32) = (-2., -12.);
/// Earth's land share (the weathering's reference).
const EARTH_LAND: f32 = 0.3;
/// Weathering: its dependence on CO₂ (power) and temperature (per K: an activation energy of
/// about 60 kJ/mol), its pace on bare ground against under plants.
const WEATHERING_CO2: f32 = 0.3;
const WEATHERING_TEMPERATURE: f32 = 0.09;
const BARE_WEATHERING: f32 = 0.25;
/// Earth's mean temperature today (°C) and pre-industrial CO₂ (ppm): weathering balances
/// outgassing there.
const EARTH_TEMPERATURE: f32 = 15.;
const EARTH_CO2: f32 = 280.;
/// CO₂ change per Myr per unit of imbalance (ppm; the ocean buffers it: a snowball takes
/// some tens of Myr to build up enough to thaw).
const CARBON_RATE: f32 = 4000.;
/// Interior heat's effect on outgassing (power of `World::interior_heat`, which is softened).
const OUTGASSING_HEAT: f32 = 2.;
/// Large igneous provinces: one per this many Myr at Earth's interior heat, strength and
/// duration (Myr) ranges.
const ERUPTION_INTERVAL: f32 = 40.;
const ERUPTION_STRENGTH: (f32, f32) = (2., 6.);
const ERUPTION_DURATION: (f32, f32) = (0.5, 2.);
/// Impacts making craters over 50 km: one per this many Myr at today's rate.
const BIG_IMPACT_INTERVAL: f32 = 30.;
/// Ice line (degrees) under which the planet snowballs, and over which it has thawed.
const SNOWBALL_LINE: f32 = 25.;
const THAW_LINE: f32 = 40.;
/// Time step (Myr) and the most CO₂ may change in one (as a share).
const STEP: f32 = 0.1;
const MAX_CHANGE: f32 = 0.05;
/// A snapshot every this many Myr.
const SNAPSHOT: f32 = 10.;

/// The energy balance's state: band temperatures (°C).
#[derive(Clone)]
struct Bands {
    t: [f32; BANDS],
}

/// Sunlight by band (share of the mean) for a tilt (degrees): annual means, the second
/// Legendre polynomial's coefficient as a function of obliquity (North 1975).
fn insolation(tilt: f32) -> [f32; BANDS] {
    let s = tilt.to_radians().sin();
    let s2 = 5. / 16. * (3. * s * s - 2.);
    std::array::from_fn(|k| {
        let x = (k as f32 + 0.5) / BANDS as f32;
        1. + s2 * (3. * x * x - 1.) / 2.
    })
}

impl Bands {
    fn earth() -> Self {
        Self {
            t: std::array::from_fn(|k| {
                let x = (k as f32 + 0.5) / BANDS as f32;
                28. - 45. * x * x
            }),
        }
    }

    /// Settle the temperatures for a world and CO₂ (from the current ones: the ice
    /// remembers).
    fn settle(&mut self, world: &World, co2: f32) {
        let mut air = world.clone();
        air.atmosphere.co2 = co2;
        let climate = air.climate();
        let q = climate.solar / 4.;
        let s = insolation(world.orbit.tilt);
        let land = world.land_fraction(EARTH_LAND);
        let ground = if world.life {
            world.albedo.plants * 0.7 + world.albedo.desert * 0.3
        } else {
            world.albedo.rock * 0.6 + world.albedo.desert * 0.4
        };
        let warm = land * ground + (1. - land) * world.albedo.sea;
        let reflect = |t: f32| {
            let ice = smoothstep(FREEZING.0, FREEZING.1, t);
            ATMOSPHERE_ALBEDO
                + climate.haze
                + SURFACE_SEEN * (warm + (world.albedo.ice - warm) * ice)
        };
        let a = OLR_A - climate.greenhouse;
        for _ in 0..200 {
            let mean = self.t.iter().sum::<f32>() / BANDS as f32;
            let mut moved = 0f32;
            for k in 0..BANDS {
                let target = (q * s[k] * (1. - reflect(self.t[k])) - a + TRANSPORT * mean)
                    / (OLR_B + TRANSPORT);
                // (Relaxed: the ice edge would otherwise flip back and forth.)
                let next = self.t[k] + (target - self.t[k]) * 0.5;
                moved = moved.max((next - self.t[k]).abs());
                self.t[k] = next;
            }
            if moved < 0.01 {
                break;
            }
        }
    }

    fn mean(&self) -> f32 {
        self.t.iter().sum::<f32>() / BANDS as f32
    }

    /// The ice line (degrees of latitude).
    fn ice_line(&self) -> f32 {
        let frozen = self.t.iter().position(|&t| t < FREEZING.0).unwrap_or(BANDS);
        let x = frozen as f32 / BANDS as f32;
        x.asin().to_degrees()
    }

    /// The share of the surface free of ice.
    fn ice_free(&self) -> f32 {
        self.t.iter().filter(|&&t| t >= FREEZING.0).count() as f32 / BANDS as f32
    }
}

/// Evolve `world` from its age to `to` (Gyr).
pub fn evolve(world: &World, to: f32, seed: u64) -> History {
    let seed = seed as u32 ^ 0x415E;
    let random = |a: u32, b: u32| hash(seed, a, b) as f32 / u32::MAX as f32;
    let mut world = world.clone();
    let mut co2 = world.atmosphere.co2.max(1.);
    let mut bands = Bands::earth();
    bands.settle(&world, co2);
    // Earth's ice-free share today, for the weathering's reference.
    let earth_free = {
        let mut earth = Bands::earth();
        let earth_world = World::default();
        earth.settle(&earth_world, EARTH_CO2);
        earth.ice_free()
    };
    let mut snapshots = Vec::new();
    let mut events = Vec::new();
    let mut frozen = bands.ice_line() < SNOWBALL_LINE;
    if frozen {
        events.push(Event::Snowball { age: world.age });
    }
    let mut eruptions: Vec<(f32, f32, f32)> = Vec::new();
    let (mut since_snapshot, mut step_index) = (f32::MAX, 0u32);
    let mut weathering = 1.;
    while world.age < to {
        let age = world.age;
        let myr = age * 1000.;
        // Events: eruptions and big impacts, as random arrivals per step.
        let heat = world.interior_heat().powf(OUTGASSING_HEAT);
        if random(step_index, 1) < STEP / ERUPTION_INTERVAL * heat.sqrt() {
            let strength = ERUPTION_STRENGTH.0
                + (ERUPTION_STRENGTH.1 - ERUPTION_STRENGTH.0) * random(step_index, 2);
            let duration = ERUPTION_DURATION.0
                + (ERUPTION_DURATION.1 - ERUPTION_DURATION.0) * random(step_index, 3);
            eruptions.push((myr, strength, duration));
            events.push(Event::Eruption {
                age,
                strength,
                duration: duration / 1000.,
            });
        }
        if random(step_index, 4) < STEP / BIG_IMPACT_INTERVAL * world.impact_flux(age) {
            // (Sizes: a power law above 25 km of radius.)
            let radius = 25. * random(step_index, 5).max(1e-3).powf(-0.5);
            events.push(Event::Impact {
                age,
                radius: radius.min(500.),
            });
        }
        eruptions.retain(|&(start, _, duration)| myr < start + duration);
        let pulse: f32 = eruptions.iter().map(|&(_, s, _)| s - 1.).sum();
        let outgassing = heat * (1. + pulse);
        // Weathering at the current state.
        let land = world.land_fraction(EARTH_LAND) / EARTH_LAND;
        let supercontinent = 1. - 0.3 * world.geology.assembly;
        let pace = if world.life { 1. } else { BARE_WEATHERING };
        weathering = (co2 / EARTH_CO2).powf(WEATHERING_CO2)
            * (WEATHERING_TEMPERATURE * (bands.mean() - EARTH_TEMPERATURE)).exp()
            * (bands.ice_free() / earth_free)
            * land.sqrt()
            * supercontinent
            * pace;
        // CO₂ toward the balance, by steps no larger than `MAX_CHANGE` of it.
        let change = (CARBON_RATE * (outgassing - weathering) * STEP)
            .clamp(-MAX_CHANGE * co2, MAX_CHANGE * co2);
        co2 = (co2 + change).clamp(10., 5e6);
        bands.settle(&world, co2);
        let line = bands.ice_line();
        if !frozen && line < SNOWBALL_LINE {
            frozen = true;
            events.push(Event::Snowball { age });
        } else if frozen && line > THAW_LINE {
            frozen = false;
            events.push(Event::Thaw { age });
        }
        since_snapshot += STEP;
        if since_snapshot >= SNAPSHOT {
            since_snapshot = 0.;
            snapshots.push(Snapshot {
                age,
                co2,
                temperature: bands.mean(),
                ice_line: line,
                outgassing,
                weathering,
            });
        }
        world.age += STEP / 1000.;
        step_index += 1;
    }
    world.age = to.max(world.age.min(to));
    world.atmosphere.co2 = co2;
    snapshots.push(Snapshot {
        age: world.age,
        co2,
        temperature: bands.mean(),
        ice_line: bands.ice_line(),
        outgassing: world.interior_heat().powf(OUTGASSING_HEAT),
        weathering,
    });
    History {
        snapshots,
        events,
        world,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn earth_balance() {
        let mut bands = Bands::earth();
        bands.settle(&World::default(), EARTH_CO2);
        let (t, line) = (bands.mean(), bands.ice_line());
        assert!((12.0..18.0).contains(&t), "{t}");
        assert!((60.0..80.0).contains(&line), "{line}");
    }

    #[test]
    fn snowball_hysteresis() {
        // Frozen, the planet stays frozen at CO₂ that keeps a warm one warm.
        let world = World::default();
        let mut warm = Bands::earth();
        warm.settle(&world, EARTH_CO2);
        let mut cold = Bands { t: [-40.; BANDS] };
        cold.settle(&world, EARTH_CO2);
        assert!(warm.ice_line() > 50.);
        assert!(cold.ice_line() < SNOWBALL_LINE, "{}", cold.ice_line());
    }
}

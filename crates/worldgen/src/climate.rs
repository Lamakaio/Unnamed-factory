//! The planet's climate (milestone 3 of `docs/living-world.md`): a reduced model on a coarse
//! cube-sphere, computed as if the planet were Earth-sized (latitudes, seasons and lapse rates
//! as on Earth), month by month over a year. The goal is Earth-like *patterns* that look right
//! to anyone (rainforests at the equator, deserts around 20–30° and in rain shadows, mild wet
//! west coasts, monsoons, tundra toward the poles), not weather forecasting.
//!
//! - Temperature: an energy balance per cell (Budyko–Sellers): monthly insolation from
//!   latitude and axial tilt, outgoing longwave `A + B T`, albedo (ice when cold), heat
//!   capacity (ocean large, land small: seasons and continentality emerge), heat spread
//!   between neighbours, and a rule-based ocean current anomaly (warm water along western
//!   ocean shores, cold along eastern ones in the subtropics). Altitude cools by the lapse rate.
//! - Wind: the thermal equator (ITCZ) per longitude; pressure belts placed around it (low at
//!   the ITCZ, highs near 30°, lows near 60°, polar highs), plus warm land lowering pressure
//!   (monsoons); wind down the pressure gradient, turned by Coriolis (trades, westerlies,
//!   polar easterlies). The terrain slows it (friction over land), blocks the part blowing up
//!   mountain slopes and turns some of it along them, and high ground is windier. The year's
//!   prevailing wind (mean vector) and strength (mean speed, storms included: their winds
//!   spread with the temperature gradient) are kept: they set the swell.
//! - Moisture: evaporation from the sea (and, less, from wet land), carried by the wind
//!   (semi-Lagrangian), raining out where air rises over mountains or converges, or is
//!   saturated; sinking air (diverging winds) stays dry. Rain shadows emerge.
//! - Köppen–Geiger classes from the monthly temperatures and precipitation.

use std::f32::consts::{PI, TAU};

use bevy_math::{Vec2, Vec3};

use crate::{
    grid::Grid,
    noise::smoothstep,
    par,
    planet::Planet,
    sphere::SphereGrid,
    world::{Albedo, World},
};

/// Climate parameters (Earth's by default; `World::climate` sets them from the world's).
#[derive(Clone, Debug)]
pub struct ClimateSettings {
    /// Cells along a face edge of the climate grid.
    pub n: usize,
    /// Axial tilt (degrees) and solar constant (W/m², at the mean distance).
    pub tilt: f32,
    pub solar: f32,
    /// Greenhouse strength: lowers the outgoing longwave (W/m²; 0 for Earth).
    pub greenhouse: f32,
    /// Year length (Earth years), eccentricity, the star's longitude at perihelion (degrees
    /// from the northern spring equinox) and day length (hours): see `world::Orbit`.
    pub year: f32,
    pub eccentricity: f32,
    pub perihelion: f32,
    pub day: f32,
    /// Albedo added by haze and dust in the air.
    pub haze: f32,
    /// Surface albedos, and whether plants cover the land.
    pub albedo: Albedo,
    pub life: bool,
    /// Strength of the ocean current anomalies (1: Earth's).
    pub currents: f32,
    /// Meters of altitude per world unit of height (the planet's relief is exaggerated).
    pub meters_per_unit: f32,
}

impl Default for ClimateSettings {
    /// Earth's.
    fn default() -> Self {
        World::default().climate()
    }
}

/// Cells along a face edge of the climate grid, and meters of altitude per world unit, by
/// default.
pub(crate) const GRID_CELLS: usize = 48;
pub(crate) const METERS_PER_UNIT: f32 = 100.;

/// Earth's radius (m), for the transport scales.
const EARTH_RADIUS: f32 = 6.371e6;
/// Outgoing longwave `A + B T` (W/m², T in °C; North et al. 1981).
const OLR_A: f32 = 203.3;
const OLR_B: f32 = 2.09;
/// Heat capacities (J/m²/K): ocean mixed layer, land.
const OCEAN_CAPACITY: f32 = 2.0e8;
const LAND_CAPACITY: f32 = 8.0e6;
/// Heat transport: the diffusion of North's energy balance model (W/m²/K, on the unit
/// sphere), shared between side neighbours `spacing` radians apart as `D / spacing²`.
const DIFFUSION: f32 = 0.4;
/// Share of the heat transport lost between land cells.
const LAND_DECOUPLING: f32 = 0.75;
/// Gauss–Seidel sweeps per step of the (implicit) heat transport.
const TRANSPORT_SWEEPS: usize = 12;
/// Surface wind speed (m/s) per unit of pressure gradient (per radian; the belts span ±1),
/// and its share left at the poles (polar highs are weak: the polar easterlies are light).
const WIND_SCALE: f32 = 1.8;
const POLAR_WIND: f32 = 0.55;
/// Storm winds (m/s, their spread about the mean) per unit of north–south temperature
/// gradient (K per radian): storms grow where the temperature changes fast with latitude
/// (baroclinicity),
/// the mid-latitudes, so their winds there are strong but unsteady.
const STORMINESS: f32 = 0.22;
/// Moisture travels with the lower troposphere's wind, faster than the surface's.
const ALOFT: f32 = 1.67;
/// Surface wind speed left over land by friction (over the sea: all of it). The wind aloft,
/// which carries the moisture, doesn't feel it.
const LAND_FRICTION: f32 = 0.55;
/// Mountain slopes (m/m, at the climate grid's scale) over which the wind blowing up them is
/// blocked, the share blocked, and the share of that turned along the slope instead.
const BLOCKING_SLOPE: (f32, f32) = (0.002, 0.012);
const BLOCKED: f32 = 0.7;
const DEFLECTED: f32 = 0.5;
/// Extra wind speed on high ground (share, reached at this altitude in m).
const ALTITUDE_GAIN: (f32, f32) = (0.3, 3000.);
/// Lapse rate (K/m).
const LAPSE: f32 = 0.0065;
/// Strength of the ocean current anomalies (W/m²).
const CURRENTS: f32 = 35.;
/// Energy balance steps per month, and Earth years run before the recorded one.
const STEPS_PER_MONTH: usize = 4;
const SPIN_UP_YEARS: usize = 7;
/// The northern spring equinox (share of the year from 1 January: around 21 March).
const SPRING: f32 = 0.25 - 10. / 365.25;
/// Planetary albedo: what the air and clouds reflect, and the share of the surface's albedo
/// seen through them (Earth: about 0.3 in all).
const ATMOSPHERE_ALBEDO: f32 = 0.24;
const SURFACE_SEEN: f32 = 0.6;
/// Land is desert (bright ground) below this annual precipitation (mm), not above this.
const DESERT_RAIN: (f32, f32) = (200., 500.);

/// A month's climate of a cell.
#[derive(Clone, Copy, Debug, Default)]
pub struct Month {
    /// Mean temperature at sea level (°C; subtract the lapse rate × altitude).
    pub temperature: f32,
    /// Precipitation (mm).
    pub precipitation: f32,
}

/// The climate: twelve months per cell of a coarse grid.
pub struct PlanetClimate {
    pub settings: ClimateSettings,
    pub grid: SphereGrid,
    pub months: Vec<[Month; 12]>,
    /// Mean land height (world units) and land share of each cell.
    pub elevation: Vec<f32>,
    pub land: Vec<f32>,
    /// January and July winds (tangent vectors, m/s), for previews.
    pub wind: [Vec<Vec3>; 2],
    /// The year's prevailing wind (mean of the monthly winds: tangent vectors, m/s) and
    /// strength (mean of the monthly speeds, m/s). Their ratio is how steady the wind is.
    pub prevailing: Vec<Vec3>,
    pub strength: Vec<f32>,
}

/// Latitude (radians) of a unit direction.
fn latitude(d: Vec3) -> f32 {
    d.y.clamp(-1., 1.).asin()
}

/// Longitude (radians) of a unit direction.
fn longitude(d: Vec3) -> f32 {
    d.z.atan2(d.x)
}

/// East and north unit tangents at a direction (north falls back near the poles).
fn frame(d: Vec3) -> (Vec3, Vec3) {
    let north = (Vec3::Y - d * d.y)
        .try_normalize()
        .unwrap_or(Vec3::X.reject_from(d).normalize());
    (d.cross(north), north)
}

/// Daily mean insolation (W/m²) at a latitude for a solar declination.
fn insolation(solar: f32, latitude: f32, declination: f32) -> f32 {
    let x = (-latitude.tan() * declination.tan()).clamp(-1., 1.);
    let h0 = x.acos();
    solar / PI
        * (h0 * latitude.sin() * declination.sin() + latitude.cos() * declination.cos() * h0.sin())
}

/// Saturation water (mm of precipitable water) at a temperature (°C), Clausius–Clapeyron-like.
fn saturation(t: f32) -> f32 {
    50. * (0.065 * (t - 25.)).exp()
}

pub fn simulate(planet: &Planet, settings: &ClimateSettings) -> PlanetClimate {
    let grid = SphereGrid::new(settings.n);
    let cells = grid.len();
    let directions: Vec<Vec3> = (0..cells).map(|i| grid.direction(i)).collect();
    let latitudes: Vec<f32> = directions.iter().map(|&d| latitude(d)).collect();
    let longitudes: Vec<f32> = directions.iter().map(|&d| longitude(d)).collect();

    // Land share and mean land height of each climate cell.
    let (mut land, mut height, mut total) =
        (vec![0f32; cells], vec![0f32; cells], vec![0f32; cells]);
    for i in 0..planet.grid.len() {
        let c = grid.cell_at(planet.grid.direction(i));
        total[c] += 1.;
        if !planet.sea[i] {
            land[c] += 1.;
            height[c] += planet.heights[i].max(0.);
        }
    }
    let elevation: Vec<f32> = (0..cells)
        .map(|c| {
            if land[c] > 0. {
                height[c] / land[c]
            } else {
                0.
            }
        })
        .collect();
    let land: Vec<f32> = (0..cells).map(|c| land[c] / total[c].max(1.)).collect();
    let altitude: Vec<f32> = elevation
        .iter()
        .map(|e| e * settings.meters_per_unit)
        .collect();
    let side: Vec<Vec<usize>> = (0..cells)
        .map(|i| {
            grid.neighbours(i)
                .filter(|&(_, d)| d < 1.2)
                .map(|(j, _)| j)
                .collect()
        })
        .collect();
    let currents = current_anomaly(&grid, &land, &latitudes, &longitudes);

    // Energy balance: sea-level temperatures, stepped through the years. (Run again once the
    // rain is known: deserts are brighter.)
    let energy = EnergyBalance {
        settings,
        latitudes: &latitudes,
        land: &land,
        altitude: &altitude,
        side: &side,
        currents: &currents,
    };
    let mut temperature: Vec<f32> = latitudes
        .iter()
        .map(|&l| 15. - 30. * l.sin().powi(2))
        .collect();
    // (Spun up for as many Earth years whatever the year's length: the ocean's heat takes
    // years to settle.)
    let spin_up = (SPIN_UP_YEARS as f32 / settings.year).ceil().clamp(2., 60.) as usize;
    let mut monthly_t = energy.run(&mut temperature, &vec![0.; cells], spin_up);

    // Winds and rain, month by month.
    let mut months = vec![[Month::default(); 12]; cells];
    let mut wind_out = [vec![Vec3::ZERO; cells], vec![Vec3::ZERO; cells]];
    let (mut prevailing, mut strength) = (vec![Vec3::ZERO; cells], vec![0f32; cells]);
    let mut land_wetness = vec![0.5f32; cells];
    let mut annual = vec![0f32; cells];
    for pass in 0..2 {
        if pass == 1 {
            let dryness: Vec<f32> = annual
                .iter()
                .map(|&p| smoothstep(DESERT_RAIN.1, DESERT_RAIN.0, p))
                .collect();
            monthly_t = energy.run(&mut temperature, &dryness, spin_up.div_ceil(2));
        }
        for month in 0..12 {
            let t: Vec<f32> = (0..cells).map(|i| monthly_t[i][month]).collect();
            let wind = winds(
                &grid,
                &directions,
                &latitudes,
                &longitudes,
                &t,
                &land,
                &altitude,
                settings.day,
            );
            let aloft: Vec<Vec3> = wind.iter().map(|w| w.1 * ALOFT).collect();
            let wind: Vec<Vec3> = wind.iter().map(|w| w.0).collect();
            let precipitation = rain(
                &grid,
                &directions,
                &aloft,
                &t,
                &land,
                &land_wetness,
                &altitude,
            );
            if pass == 0 {
                for (a, p) in annual.iter_mut().zip(&precipitation) {
                    *a += p;
                }
            }
            if pass == 1 {
                for i in 0..cells {
                    months[i][month] = Month {
                        temperature: t[i],
                        precipitation: precipitation[i],
                    };
                }
                for i in 0..cells {
                    let storms = STORMINESS
                        * gradient(&grid, &directions, &t, i).y.abs()
                        * (1. - (1. - LAND_FRICTION) * land[i]);
                    prevailing[i] += wind[i] / 12.;
                    strength[i] += (wind[i].length_squared() + storms * storms).sqrt() / 12.;
                }
                if month == 0 {
                    wind_out[0] = wind.clone();
                } else if month == 6 {
                    wind_out[1] = wind.clone();
                }
            }
            // Wet land evaporates more next time round (rain recycling inland).
            for i in 0..cells {
                land_wetness[i] = (land_wetness[i] * 0.7 + 0.3 * (precipitation[i] / 100.).min(1.))
                    .clamp(0.05, 1.);
            }
        }
    }
    PlanetClimate {
        settings: settings.clone(),
        grid,
        months,
        elevation,
        land,
        wind: wind_out,
        prevailing,
        strength,
    }
}

/// Where the star is in the sky over the year: its declination (radians) and the planet's
/// distance (share of the mean) at a time of year (0..1 from 1 January), on a Kepler orbit.
fn orbit(settings: &ClimateSettings, t: f32) -> (f32, f32) {
    let e = settings.eccentricity.clamp(0., 0.9);
    let perihelion = settings.perihelion.to_radians();
    // Mean anomaly (zero at perihelion): on a circular orbit, the star's longitude goes
    // round evenly from the northern spring equinox.
    let mean = TAU * (t - SPRING) - perihelion;
    // Kepler's equation by Newton's method.
    let mut eccentric = mean;
    for _ in 0..6 {
        eccentric -= (eccentric - e * eccentric.sin() - mean) / (1. - e * eccentric.cos());
    }
    let anomaly = 2.
        * ((1. + e).sqrt() * (eccentric / 2.).sin())
            .atan2((1. - e).sqrt() * (eccentric / 2.).cos());
    let longitude = perihelion + anomaly;
    (
        settings.tilt.to_radians() * longitude.sin(),
        1. - e * eccentric.cos(),
    )
}

/// The energy balance of the climate grid.
struct EnergyBalance<'a> {
    settings: &'a ClimateSettings,
    latitudes: &'a [f32],
    land: &'a [f32],
    /// Mean land altitude (m).
    altitude: &'a [f32],
    /// Side neighbours.
    side: &'a [Vec<usize>],
    /// Ocean current heating (W/m²).
    currents: &'a [f32],
}

impl EnergyBalance<'_> {
    /// Steps the sea-level temperatures through `years` years from `temperature` (left at the
    /// end of the last), with the land `dryness` (0..1: deserts are brighter); returns the
    /// last year's monthly means.
    fn run(&self, temperature: &mut [f32], dryness: &[f32], years: usize) -> Vec<[f32; 12]> {
        let settings = self.settings;
        let (land, side) = (self.land, self.side);
        let cells = land.len();
        let capacity: Vec<f32> = land
            .iter()
            .map(|&l| l * LAND_CAPACITY + (1. - l) * OCEAN_CAPACITY)
            .collect();
        let dt = settings.year * 365.25 * 86400. / (12 * STEPS_PER_MONTH) as f32;
        let spacing = std::f32::consts::FRAC_PI_2 / settings.n as f32;
        // (Heat spreads less far on fast-spinning planets: their eddies are smaller.)
        let spin = 24. / settings.day.max(1.);
        let coupling = DIFFUSION / spin.cbrt() / (spacing * spacing);
        let albedo = &settings.albedo;
        let ground = if settings.life {
            albedo.plants
        } else {
            albedo.rock
        };
        let mut monthly_t = vec![[0f32; 12]; cells];
        for year in 0..years {
            for month in 0..12 {
                for step in 0..STEPS_PER_MONTH {
                    let t = (month as f32 + (step as f32 + 0.5) / STEPS_PER_MONTH as f32) / 12.;
                    let (declination, distance) = orbit(settings, t);
                    let solar = settings.solar / (distance * distance);
                    // Local energy balance (explicit), then the heat transport (implicit: it
                    // is far stiffer than the local terms).
                    let previous = temperature.to_vec();
                    let local = par::map(cells, |i| {
                        let surface_t = previous[i] - LAPSE * self.altitude[i];
                        let on_land = ground + (albedo.desert - ground) * dryness[i];
                        let mut surface = land[i] * on_land + (1. - land[i]) * albedo.sea;
                        // (Sea ice covers less of a cold sea than snow does land.)
                        let ice = smoothstep(-5., -15., surface_t) * (0.6 + 0.4 * land[i]);
                        surface += (albedo.ice - surface) * ice;
                        let reflected = ATMOSPHERE_ALBEDO + settings.haze + SURFACE_SEEN * surface;
                        let absorbed =
                            insolation(solar, self.latitudes[i], declination) * (1. - reflected);
                        let emitted = OLR_A - settings.greenhouse + OLR_B * previous[i];
                        let current = self.currents[i] * settings.currents * (1. - land[i]);
                        previous[i] + dt * (absorbed - emitted + current) / capacity[i]
                    });
                    // (C/dt + Σ K) T_i = C/dt T_local + Σ K T_j, by Gauss–Seidel sweeps.
                    // (Weaker over land: the air over continents heats and cools with them,
                    // so their interiors get hot summers and cold winters.)
                    for _ in 0..TRANSPORT_SWEEPS {
                        for i in 0..cells {
                            let c = capacity[i] / dt;
                            let (mut sum, mut weight) = (0., 0.);
                            for &j in &side[i] {
                                let k =
                                    coupling * (1. - LAND_DECOUPLING * (land[i] + land[j]) / 2.);
                                sum += k * temperature[j];
                                weight += k;
                            }
                            temperature[i] = (c * local[i] + sum) / (c + weight);
                        }
                    }
                    if year + 1 == years {
                        for (months, t) in monthly_t.iter_mut().zip(temperature.iter()) {
                            months[month] += t / STEPS_PER_MONTH as f32;
                        }
                    }
                }
            }
        }
        monthly_t
    }
}

/// Ocean current heating (W/m²): warm along the western side of ocean basins (east coasts of
/// continents) in the subtropics and mid-latitudes, cold along their eastern side (west coasts)
/// in the subtropics. Distances to land are measured along the latitude circle.
fn current_anomaly(
    grid: &SphereGrid,
    land: &[f32],
    latitudes: &[f32],
    longitudes: &[f32],
) -> Vec<f32> {
    let step = 1.5f32.to_radians();
    let range = 45f32.to_radians();
    par::map(grid.len(), |i| {
        if land[i] > 0.5 {
            return 0.;
        }
        let (lat, lon) = (latitudes[i], longitudes[i]);
        let at = |lon: f32| {
            let d = Vec3::new(lat.cos() * lon.cos(), lat.sin(), lat.cos() * lon.sin());
            land[grid.cell_at(d)] > 0.5
        };
        let distance = |sign: f32| {
            let mut d = step;
            while d < range {
                if at(lon + sign * d) {
                    return d;
                }
                d += step;
            }
            range
        };
        let (west, east) = (distance(-1.), distance(1.));
        let scale = 12f32.to_radians();
        let abs_lat = lat.abs().to_degrees();
        let warm =
            (-west / scale).exp() * smoothstep(10., 22., abs_lat) * smoothstep(55., 40., abs_lat);
        let cold =
            (-east / scale).exp() * smoothstep(8., 18., abs_lat) * smoothstep(45., 30., abs_lat);
        CURRENTS * (warm - cold)
    })
}

/// Least-squares gradient of `values` at cell `i` (per radian, in the east/north frame).
fn gradient(grid: &SphereGrid, directions: &[Vec3], values: &[f32], i: usize) -> Vec2 {
    let (east, north) = frame(directions[i]);
    let (mut ee, mut en, mut nn, mut eh, mut nh) = (0., 0., 0., 0., 0.);
    for (j, _) in grid.neighbours(i) {
        let t = directions[j] - directions[i];
        let (e, n) = (t.dot(east), t.dot(north));
        let dv = values[j] - values[i];
        ee += e * e;
        en += e * n;
        nn += n * n;
        eh += e * dv;
        nh += n * dv;
    }
    let det = ee * nn - en * en;
    if det.abs() < 1e-12 {
        return Vec2::ZERO;
    }
    Vec2::new((nn * eh - en * nh) / det, (ee * nh - en * eh) / det)
}

/// Winds (tangent vectors, m/s) for a month's temperatures: at the surface, and aloft (the
/// same without the land's friction).
fn winds(
    grid: &SphereGrid,
    directions: &[Vec3],
    latitudes: &[f32],
    longitudes: &[f32],
    t: &[f32],
    land: &[f32],
    altitude: &[f32],
    day: f32,
) -> Vec<(Vec3, Vec3)> {
    let cells = grid.len();
    // Circulation cells per hemisphere (Earth's three: Hadley, Ferrel, polar): fewer and
    // wider on slowly spinning planets, more and narrower on fast ones (the Hadley cell's
    // width goes roughly as rotation^-1/3 in idealised models: Hill et al. 2019, Kaspi &
    // Showman 2015).
    let spin = 24. / day.max(1.);
    let circulation_cells = (3. * spin.cbrt()).clamp(1., 8.);
    // Coriolis turning of the surface wind off the pressure gradient (70° on Earth): less on
    // slow planets.
    let turning = (70f32.to_radians().tan() * spin).atan();
    // The thermal equator per longitude band: the warmest latitude within 35° of the equator
    // (zonal means per 2° band), smoothed along the longitudes.
    const BANDS: usize = 72;
    const ROWS: usize = 36;
    let mut sum = vec![[0f32; ROWS]; BANDS];
    let mut count = vec![[0f32; ROWS]; BANDS];
    let band = |lon: f32| (((lon + PI) / TAU * BANDS as f32) as usize).min(BANDS - 1);
    for i in 0..cells {
        let lat = latitudes[i].to_degrees();
        if lat.abs() < 35. {
            let row = (((lat + 35.) / 70. * ROWS as f32) as usize).min(ROWS - 1);
            sum[band(longitudes[i])][row] += t[i];
            count[band(longitudes[i])][row] += 1.;
        }
    }
    let raw: Vec<f32> = (0..BANDS)
        .map(|b| {
            let (mut best, mut best_t) = (0., f32::MIN);
            for row in 0..ROWS {
                if count[b][row] > 0. {
                    let mean = sum[b][row] / count[b][row];
                    if mean > best_t {
                        best_t = mean;
                        best = -35. + (row as f32 + 0.5) * 70. / ROWS as f32;
                    }
                }
            }
            best
        })
        .collect();
    let itcz: Vec<f32> = (0..BANDS)
        .map(|b| {
            (-4..=4)
                .map(|k| raw[(b as i64 + k).rem_euclid(BANDS as i64) as usize])
                .sum::<f32>()
                / 9.
        })
        .collect();
    // Zonal mean temperatures (per degree of latitude), for the land–sea pressure anomaly.
    let mut zonal = [(0f32, 0f32); 181];
    for i in 0..cells {
        let row = (latitudes[i].to_degrees() + 90.).round() as usize;
        zonal[row].0 += t[i];
        zonal[row].1 += 1.;
    }
    let zonal_mean = |lat: f32| {
        let row = (lat.to_degrees() + 90.).round() as usize;
        let (s, c) = zonal[row.saturating_sub(3)..(row + 4).min(181)]
            .iter()
            .fold((0., 0.), |(s, c), &(t, n)| (s + t, c + n));
        s / f32::max(c, 1.)
    };
    // Pressure: belts relative to the thermal equator, lowered over warm land.
    let pressure: Vec<f32> = (0..cells)
        .map(|i| {
            let lat = latitudes[i].to_degrees();
            let shift = itcz[band(longitudes[i])];
            let s = if lat >= shift {
                (lat - shift) / (90. - shift) * 90.
            } else {
                (lat - shift) / (90. + shift) * 90.
            };
            let belts = -(2. * circulation_cells * s.to_radians()).cos();
            let thermal = (t[i] - zonal_mean(latitudes[i])) * land[i];
            belts - 0.06 * thermal
        })
        .collect();
    par::map(cells, |i| {
        let g = gradient(grid, directions, &pressure, i);
        let down = -g;
        // Coriolis turns the flow to the right in the north, left in the south (along the
        // isobars, friction keeping some flow toward low pressure).
        let lat = latitudes[i];
        let turn = -lat.signum() * turning * smoothstep(0., 12f32.to_radians(), lat.abs());
        let polar =
            1. - (1. - POLAR_WIND) * smoothstep(60f32.to_radians(), 75f32.to_radians(), lat.abs());
        let mut flow = Vec2::from_angle(turn).rotate(down) * WIND_SCALE * polar;
        // Mountains: the wind blowing up a steep slope is partly blocked, and some of it turns
        // along the slope (going round the range).
        let slope = gradient(grid, directions, altitude, i) / EARTH_RADIUS;
        let steepness = slope.length();
        if steepness > 1e-6 {
            let uphill = slope / steepness;
            let up = flow.dot(uphill).max(0.);
            let blocked = up * BLOCKED * smoothstep(BLOCKING_SLOPE.0, BLOCKING_SLOPE.1, steepness);
            let along = uphill.perp() * flow.dot(uphill.perp()).signum();
            flow += -uphill * blocked + along * blocked * DEFLECTED;
        }
        // High ground is windier.
        flow *= 1. + ALTITUDE_GAIN.0 * smoothstep(0., ALTITUDE_GAIN.1, altitude[i]);
        let (east, north) = frame(directions[i]);
        let aloft = east * flow.x + north * flow.y;
        // Friction over land.
        (aloft * (1. - (1. - LAND_FRICTION) * land[i]), aloft)
    })
}

/// A month's precipitation (mm) from the winds: moisture evaporated, carried, rained out.
fn rain(
    grid: &SphereGrid,
    directions: &[Vec3],
    wind: &[Vec3],
    t: &[f32],
    land: &[f32],
    land_wetness: &[f32],
    altitude: &[f32],
) -> Vec<f32> {
    let cells = grid.len();
    // Uplift by terrain (wind up the slope, m/s) and convergence (1/s).
    let uplift: Vec<f32> = par::map(cells, |i| {
        let g = gradient(grid, directions, altitude, i) / EARTH_RADIUS;
        let (east, north) = frame(directions[i]);
        let w = Vec2::new(wind[i].dot(east), wind[i].dot(north));
        w.dot(g).max(0.)
    });
    let divergence: Vec<f32> = par::map(cells, |i| {
        let (mut sum, mut count) = (0f32, 0f32);
        for (j, _) in grid.neighbours(i) {
            let t = directions[j] - directions[i];
            let length = t.length() * EARTH_RADIUS;
            sum += (wind[j] - wind[i]).dot(t / t.length()) / length;
            count += 1.;
        }
        2. * sum / count.max(1.)
    });
    // Semi-Lagrangian transport in steps of `step` seconds.
    let step = 0.25 * 86400.;
    let steps = 150;
    let day_fraction = step / 86400.;
    let evaporation: Vec<f32> = (0..cells)
        .map(|i| {
            let sea = 5. * (0.07 * (t[i] - 25.)).exp().min(1.6);
            let wet_land = 2.5
                * land_wetness[i]
                * (0.06 * (t[i] - 20.)).exp().min(1.5)
                * smoothstep(-5., 5., t[i]);
            (sea * (1. - land[i]) + wet_land * land[i]) * day_fraction
        })
        .collect();
    let upstream: Vec<usize> = par::map(cells, |i| {
        let back = directions[i] - wind[i] * step / EARTH_RADIUS;
        grid.cell_at(back.normalize())
    });
    let mut q = vec![10f32; cells];
    let mut total = vec![0f32; cells];
    let recorded = steps / 2;
    for k in 0..steps {
        let previous = q.clone();
        let rained: Vec<(f32, f32)> = par::map(cells, |i| {
            // (A little mixing with the neighbours spreads the moisture plumes.)
            let carried = previous[upstream[i]] * 0.85
                + 0.15 * grid.neighbours(i).map(|(j, _)| previous[j]).sum::<f32>()
                    / grid.neighbours(i).count() as f32;
            let water = carried + evaporation[i];
            let saturated = (water - saturation(t[i] - LAPSE * altitude[i])).max(0.) * 0.5;
            let rate = (0.02 + 150. * uplift[i] + 4.0e4 * (-divergence[i]).max(0.))
                * (1. - 0.97 * smoothstep(0., 2.0e-6, divergence[i]))
                * day_fraction;
            let p = (water * rate.min(0.9) + saturated).min(water);
            (water - p, p)
        });
        for i in 0..cells {
            q[i] = rained[i].0;
            if k >= steps - recorded {
                total[i] += rained[i].1;
            }
        }
    }
    // Scale the recorded days to a month.
    let days = recorded as f32 * day_fraction;
    total.iter().map(|p| p * 30.4 / days).collect()
}

/// Köppen–Geiger climate classes (the main ones).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Koppen {
    /// Tropical: rainforest, monsoon, savanna.
    Af,
    Am,
    Aw,
    /// Arid: hot / cold desert, hot / cold steppe.
    BWh,
    BWk,
    BSh,
    BSk,
    /// Temperate: dry summer (Mediterranean), dry winter, no dry season; hot / warm / cold
    /// summers folded in.
    Csa,
    Csb,
    Cwa,
    Cfa,
    Cfb,
    Cfc,
    /// Continental: hot / warm summer, subarctic.
    Dfa,
    Dfb,
    Dfc,
    Dw,
    /// Polar: tundra, ice cap.
    ET,
    EF,
}

impl Koppen {
    pub const ALL: [Koppen; 19] = [
        Koppen::Af,
        Koppen::Am,
        Koppen::Aw,
        Koppen::BWh,
        Koppen::BWk,
        Koppen::BSh,
        Koppen::BSk,
        Koppen::Csa,
        Koppen::Csb,
        Koppen::Cwa,
        Koppen::Cfa,
        Koppen::Cfb,
        Koppen::Cfc,
        Koppen::Dfa,
        Koppen::Dfb,
        Koppen::Dfc,
        Koppen::Dw,
        Koppen::ET,
        Koppen::EF,
    ];

    /// The class's index in [`Koppen::ALL`].
    pub fn id(self) -> u8 {
        Koppen::ALL.iter().position(|&k| k == self).unwrap() as u8
    }

    /// The class of an index in [`Koppen::ALL`] (clamped).
    pub fn from_id(id: u8) -> Koppen {
        Koppen::ALL[(id as usize).min(Koppen::ALL.len() - 1)]
    }

    /// Classify twelve months (temperatures in °C at the place's altitude, precipitation in mm);
    /// `north`: the place is in the northern hemisphere (summer is April–September).
    pub fn classify(t: [f32; 12], p: [f32; 12], north: bool) -> Koppen {
        let t_min = t.iter().copied().fold(f32::MAX, f32::min);
        let t_max = t.iter().copied().fold(f32::MIN, f32::max);
        let t_mean = t.iter().sum::<f32>() / 12.;
        let p_year: f32 = p.iter().sum();
        let summer = |m: usize| (3..9).contains(&m) == north;
        let p_summer: f32 = (0..12).filter(|&m| summer(m)).map(|m| p[m]).sum();
        let summer_months = || (0..12).filter(|&m| summer(m)).map(|m| p[m]);
        let winter_months = || (0..12).filter(|&m| !summer(m)).map(|m| p[m]);
        let p_driest = p.iter().copied().fold(f32::MAX, f32::min);
        let driest_summer = summer_months().fold(f32::MAX, f32::min);
        let wettest_summer = summer_months().fold(0., f32::max);
        let driest_winter = winter_months().fold(f32::MAX, f32::min);
        let wettest_winter = winter_months().fold(0., f32::max);
        if t_max < 10. {
            return if t_max > 0. { Koppen::ET } else { Koppen::EF };
        }
        let share = p_summer / p_year.max(1e-3);
        let threshold = 20. * t_mean
            + if share >= 0.7 {
                280.
            } else if share >= 0.3 {
                140.
            } else {
                0.
            };
        if p_year < threshold {
            let hot = t_mean >= 18.;
            return match (p_year < threshold / 2., hot) {
                (true, true) => Koppen::BWh,
                (true, false) => Koppen::BWk,
                (false, true) => Koppen::BSh,
                (false, false) => Koppen::BSk,
            };
        }
        if t_min >= 18. {
            return if p_driest >= 60. {
                Koppen::Af
            } else if p_driest >= 100. - p_year / 25. {
                Koppen::Am
            } else {
                Koppen::Aw
            };
        }
        let dry_summer = driest_summer < 40. && driest_summer < wettest_winter / 3.;
        let dry_winter = driest_winter < wettest_summer / 10.;
        let warm_months = t.iter().filter(|&&m| m >= 10.).count();
        if t_min > 0. {
            if dry_summer {
                return if t_max >= 22. {
                    Koppen::Csa
                } else {
                    Koppen::Csb
                };
            }
            if dry_winter {
                return Koppen::Cwa;
            }
            return if t_max >= 22. {
                Koppen::Cfa
            } else if warm_months >= 4 {
                Koppen::Cfb
            } else {
                Koppen::Cfc
            };
        }
        if dry_winter {
            return Koppen::Dw;
        }
        if t_max >= 22. {
            Koppen::Dfa
        } else if warm_months >= 4 {
            Koppen::Dfb
        } else {
            Koppen::Dfc
        }
    }

    /// Map colour (sRGB, 0..1), close to the usual Köppen map colours.
    pub fn map_color(self) -> [f32; 3] {
        let rgb = |r: u8, g: u8, b: u8| [r as f32 / 255., g as f32 / 255., b as f32 / 255.];
        match self {
            Koppen::Af => rgb(0, 0, 255),
            Koppen::Am => rgb(0, 120, 255),
            Koppen::Aw => rgb(70, 170, 250),
            Koppen::BWh => rgb(255, 0, 0),
            Koppen::BWk => rgb(255, 150, 150),
            Koppen::BSh => rgb(245, 165, 0),
            Koppen::BSk => rgb(255, 220, 100),
            Koppen::Csa => rgb(255, 255, 0),
            Koppen::Csb => rgb(200, 200, 0),
            Koppen::Cwa => rgb(150, 255, 150),
            Koppen::Cfa => rgb(200, 255, 80),
            Koppen::Cfb => rgb(100, 255, 80),
            Koppen::Cfc => rgb(50, 200, 0),
            Koppen::Dfa => rgb(0, 255, 255),
            Koppen::Dfb => rgb(55, 200, 255),
            Koppen::Dfc => rgb(0, 125, 125),
            Koppen::Dw => rgb(170, 175, 255),
            Koppen::ET => rgb(178, 178, 178),
            Koppen::EF => rgb(102, 102, 102),
        }
    }

    /// Ground material weights for the terrain shader: (dryness of the grass, forest,
    /// desert, snow, cold: conifers and alpine grass), 0..1.
    pub fn ground_weights(self) -> [f32; 5] {
        match self {
            Koppen::Af | Koppen::Am => [0.0, 1.0, 0.0, 0.0, 0.0],
            Koppen::Aw => [0.6, 0.25, 0.0, 0.0, 0.0],
            Koppen::BWh => [1.0, 0.0, 1.0, 0.0, 0.0],
            Koppen::BWk => [1.0, 0.0, 0.8, 0.0, 0.3],
            Koppen::BSh => [0.85, 0.05, 0.25, 0.0, 0.0],
            Koppen::BSk => [0.8, 0.05, 0.15, 0.0, 0.3],
            Koppen::Csa | Koppen::Csb => [0.6, 0.3, 0.0, 0.0, 0.0],
            Koppen::Cwa | Koppen::Cfa => [0.15, 0.55, 0.0, 0.0, 0.0],
            Koppen::Cfb | Koppen::Cfc => [0.05, 0.45, 0.0, 0.0, 0.15],
            Koppen::Dfa | Koppen::Dfb => [0.1, 0.6, 0.0, 0.0, 0.45],
            Koppen::Dw => [0.3, 0.45, 0.0, 0.0, 0.5],
            Koppen::Dfc => [0.1, 0.75, 0.0, 0.1, 0.9],
            Koppen::ET => [0.25, 0.0, 0.0, 0.25, 1.0],
            Koppen::EF => [0.0, 0.0, 0.0, 1.0, 1.0],
        }
    }

    /// Ground colour of the landscape (sRGB, 0..1, toon-friendly).
    pub fn ground_color(self) -> [f32; 3] {
        match self {
            Koppen::Af | Koppen::Am => [0.18, 0.42, 0.16],
            Koppen::Aw => [0.55, 0.62, 0.28],
            Koppen::BWh => [0.88, 0.74, 0.50],
            Koppen::BWk => [0.76, 0.68, 0.52],
            Koppen::BSh => [0.74, 0.66, 0.38],
            Koppen::BSk => [0.66, 0.64, 0.42],
            Koppen::Csa | Koppen::Csb => [0.58, 0.60, 0.32],
            Koppen::Cwa | Koppen::Cfa => [0.36, 0.58, 0.26],
            Koppen::Cfb | Koppen::Cfc => [0.42, 0.64, 0.30],
            Koppen::Dfa | Koppen::Dfb | Koppen::Dw => [0.32, 0.52, 0.26],
            Koppen::Dfc => [0.22, 0.38, 0.26],
            Koppen::ET => [0.56, 0.58, 0.50],
            Koppen::EF => [0.93, 0.95, 0.98],
        }
    }
}

impl PlanetClimate {
    /// The twelve months of the climate cell nearest a direction, with temperatures at an
    /// altitude (world units above sea level).
    pub fn months_at(&self, direction: Vec3, height: f32) -> ([f32; 12], [f32; 12]) {
        // Bilinear between the four nearest cells, across the face's edges (corners beyond it
        // are the neighbouring face's cells there: no seam along the cube's edges).
        let (face, p) = self.grid.face_coords(direction);
        let n = self.grid.n() as i64;
        let (x0, z0) = (p.x.floor(), p.y.floor());
        let (fx, fz) = (p.x - x0, p.y - z0);
        let cell = |x: f32, z: f32| {
            let (xi, zi) = (x as i64, z as i64);
            if (0..n).contains(&xi) && (0..n).contains(&zi) {
                self.grid.index(face, xi as usize, zi as usize)
            } else {
                self.grid
                    .cell_at(self.grid.direction_at(face, Vec2::new(x, z)))
            }
        };
        let corners = [
            (cell(x0, z0), (1. - fx) * (1. - fz)),
            (cell(x0 + 1., z0), fx * (1. - fz)),
            (cell(x0, z0 + 1.), (1. - fx) * fz),
            (cell(x0 + 1., z0 + 1.), fx * fz),
        ];
        let drop = LAPSE * height.max(0.) * self.settings.meters_per_unit;
        let blend = |f: &dyn Fn(&Month) -> f32, k: usize| {
            corners
                .iter()
                .map(|&(c, w)| f(&self.months[c][k]) * w)
                .sum::<f32>()
        };
        (
            std::array::from_fn(|k| blend(&|m| m.temperature, k) - drop),
            std::array::from_fn(|k| blend(&|m| m.precipitation, k)),
        )
    }

    /// The prevailing winds and strengths, to sample anywhere (see [`WindField`]).
    pub fn wind_field(&self) -> WindField {
        WindField {
            grid: SphereGrid::new(self.grid.n()),
            prevailing: self.prevailing.clone(),
            strength: self.strength.clone(),
        }
    }

    /// The Köppen class at a direction and altitude (world units).
    pub fn koppen_at(&self, direction: Vec3, height: f32) -> Koppen {
        let (t, p) = self.months_at(direction, height);
        Koppen::classify(t, p, direction.y >= 0.)
    }
}

/// The year's prevailing wind and wind strength of the climate cells (see `PlanetClimate`).
pub struct WindField {
    grid: SphereGrid,
    prevailing: Vec<Vec3>,
    strength: Vec<f32>,
}

impl WindField {
    /// The prevailing wind (tangent vector, m/s) and wind strength (mean speed, m/s) in a
    /// direction, bilinear between the climate cells.
    pub fn at(&self, direction: Vec3) -> (Vec3, f32) {
        let (face, p) = self.grid.face_coords(direction);
        let last = (self.grid.n() - 1) as f32;
        let p = p.clamp(Vec2::ZERO, Vec2::splat(last));
        let (x0, z0) = (p.x.floor(), p.y.floor());
        let (fx, fz) = (p.x - x0, p.y - z0);
        let (x0, z0) = (x0 as usize, z0 as usize);
        let (x1, z1) = ((x0 + 1).min(last as usize), (z0 + 1).min(last as usize));
        let (mut wind, mut strength) = (Vec3::ZERO, 0.);
        for (c, w) in [
            (self.grid.index(face, x0, z0), (1. - fx) * (1. - fz)),
            (self.grid.index(face, x1, z0), fx * (1. - fz)),
            (self.grid.index(face, x0, z1), (1. - fx) * fz),
            (self.grid.index(face, x1, z1), fx * fz),
        ] {
            wind += self.prevailing[c] * w;
            strength += self.strength[c] * w;
        }
        (wind.reject_from(direction), strength)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orbit_seasons() {
        let earth = ClimateSettings {
            eccentricity: 0.,
            ..ClimateSettings::default()
        };
        // A circular orbit: the declination of the former formula (solstices around 21 June
        // and 21 December), at the mean distance.
        for k in 0..48 {
            let t = k as f32 / 48.;
            let (declination, distance) = orbit(&earth, t);
            let former = -earth.tilt.to_radians() * (TAU * (t + 10. / 365.25)).cos();
            assert!(
                (declination - former).abs() < 1e-4,
                "{t}: {declination} {former}"
            );
            assert!((distance - 1.).abs() < 1e-5);
        }
        // An eccentric one: closest (1 - e) at perihelion, the north's summer solstice here.
        let eccentric = ClimateSettings {
            eccentricity: 0.3,
            perihelion: 90.,
            ..earth
        };
        let (closest, at) = (0..365)
            .map(|d| d as f32 / 365.)
            .map(|t| (orbit(&eccentric, t).1, t))
            .fold((f32::MAX, 0.), |a, b| if b.0 < a.0 { b } else { a });
        assert!((closest - 0.7).abs() < 1e-3, "{closest}");
        let declination = orbit(&eccentric, at).0;
        assert!((declination - eccentric.tilt.to_radians()).abs() < 0.01);
    }
}

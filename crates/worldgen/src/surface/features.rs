//! Per-cell fields of the surface's features, from the planet and its climate: river banks
//! (riparian strips), karst, desert types (sand seas, badlands), sea and lake ice.

use crate::{
    climate::PlanetClimate,
    grid::Grid,
    noise::{Noise, smoothstep},
    planet::Planet,
};

/// Badlands: soft rocks (shale, sandstone) in dry climates (deserts and steppes), on slopes
/// steeper than `BADLANDS_SLOPE`, in stretches.
const BADLANDS_SLOPE: (f32, f32) = (0.15, 0.4);

/// Sand seas: in desert climates (BWh, BWk) where a noise (wavelength `ERG_WAVELENGTH`, world
/// units) is high, on gentle ground (slope below `ERG_SLOPE`) below `ERG_HEIGHT`.
const ERG_WAVELENGTH: f32 = 90.;
const ERG_SLOPE: (f32, f32) = (0.12, 0.35);
const ERG_HEIGHT: (f32, f32) = (12., 20.);

/// River banks reach `RIPARIAN_REACH.0` plus `RIPARIAN_REACH.1` × the river's width (world
/// units) from its centre.
const RIPARIAN_REACH: (f32, f32) = (1.5, 1.2);

/// How near each cell is to a river (see [`PlanetSurface::riparian`]).
pub(super) fn riparian(planet: &Planet) -> Vec<f32> {
    let grid = &planet.grid;
    let radius = planet.radius();
    let mut near = vec![0f32; grid.len()];
    // (Banks are green with plants: none without life.)
    if !planet.settings.world.life {
        return near;
    }
    let mut seen = vec![0u32; grid.len()];
    let mut stamp = 0u32;
    for river in &planet.rivers {
        for point in &river.points {
            stamp += 1;
            let reach = RIPARIAN_REACH.0 + RIPARIAN_REACH.1 * point.width;
            let start = grid.cell_at(point.direction);
            seen[start] = stamp;
            let mut queue = std::collections::VecDeque::from([start]);
            while let Some(i) = queue.pop_front() {
                let r = grid.direction(i).distance(point.direction) * radius;
                if r > reach {
                    continue;
                }
                near[i] = near[i].max(smoothstep(reach, point.width * 0.5, r));
                for (j, _) in grid.neighbours(i) {
                    if seen[j] != stamp {
                        seen[j] = stamp;
                        queue.push_back(j);
                    }
                }
            }
        }
    }
    near
}

/// Tidal range (m) on open coasts at Earth's tidal forcing, and how much a wide shelf (cells
/// to deep water: `TIDE_SHELF`) raises it (the tide wave slows and piles up over shallow
/// shelves: northwest Europe, Patagonia).
const OPEN_TIDE: f32 = 0.8;
const SHELF_GAIN: f32 = 5.;
const TIDE_SHELF: (f32, f32) = (2., 14.);
/// Bays raise it more (the tide funnelled into a narrowing inlet: the Bay of Fundy, the
/// Severn): by up to `BAY_GAIN` where little of the surroundings (`BAY_REACH` cells) is sea.
const BAY_GAIN: f32 = 1.5;
const BAY_REACH: usize = 4;
/// Deep water starts below this (world units: the shelf's edge).
const DEEP_WATER: f32 = -2.;

/// Tidal range of each coastal cell (m; 0 away from the sea's shores): from the tidal forcing
/// of the moons and the star, raised along wide shelves.
pub(super) fn tides(planet: &Planet) -> Vec<f32> {
    let grid = &planet.grid;
    let cells = grid.len();
    let forcing = planet.settings.world.tidal_forcing();
    // Distance (cells) from deep water through the sea.
    let mut shelf = vec![u32::MAX; cells];
    let mut queue: std::collections::VecDeque<usize> = (0..cells)
        .filter(|&i| planet.sea[i] && planet.heights[i] < DEEP_WATER)
        .collect();
    for &i in &queue {
        shelf[i] = 0;
    }
    while let Some(i) = queue.pop_front() {
        for (j, _) in grid.neighbours(i) {
            if planet.sea[j] && shelf[j] == u32::MAX {
                shelf[j] = shelf[i] + 1;
                queue.push_back(j);
            }
        }
    }
    let open = grid.blur(
        &planet
            .sea
            .iter()
            .map(|&s| if s { 1. } else { 0. })
            .collect::<Vec<f32>>(),
        BAY_REACH,
    );
    let at_sea = |i: usize| {
        let width = shelf[i].min(1000) as f32;
        OPEN_TIDE
            * forcing
            * (1. + SHELF_GAIN * smoothstep(TIDE_SHELF.0, TIDE_SHELF.1, width))
            * (1. + BAY_GAIN * smoothstep(0.5, 0.25, open[i]))
    };
    let raw = crate::par::map(cells, |i| {
        if planet.sea[i] {
            // (Sea cells along the shore.)
            if grid.neighbours(i).any(|(j, _)| !planet.sea[j]) {
                at_sea(i)
            } else {
                0.
            }
        } else {
            // (Land cells along the shore: the sea's beside them.)
            grid.neighbours(i)
                .filter(|&(j, _)| planet.sea[j])
                .map(|(j, _)| at_sea(j))
                .fold(0., f32::max)
        }
    });
    grid.blur(&raw, 2)
}

/// Tidal flats: on gentle shores where the tide covers and uncovers the ground (heights below
/// the high tide, exaggerated as the relief is: `TIDAL_FLATS` × the half range in world
/// units), 0..1. Salt marsh with life, bare mud without.
const TIDAL_FLATS: f32 = 6.;

pub(super) fn tidal_flats(planet: &Planet, tide: &[f32]) -> Vec<f32> {
    let grid = &planet.grid;
    let raw = crate::par::map(grid.len(), |i| {
        if planet.sea[i] || tide[i] <= 0. {
            return 0.;
        }
        let high = tide[i] / 2. / crate::climate::METERS_PER_UNIT * TIDAL_FLATS;
        let slope = grid
            .neighbours(i)
            .map(|(j, d)| (planet.heights[j] - planet.heights[i]).abs() / d)
            .fold(0., f32::max);
        smoothstep(high, high * 0.5, planet.heights[i]) * smoothstep(0.15, 0.05, slope)
    });
    grid.blur(&raw, 1)
}

/// Sea ice: none where the warmest month at sea level reaches the first (°C), pack ice where
/// it stays below the second (for Earth's salinity: shifted with the freezing point).
const SEA_ICE_WARMEST: (f32, f32) = (4., 0.);
/// Earth's sea water freezing point (°C).
const EARTH_SEA_FREEZING: f32 = -1.9;
/// Without plants to pin it, sand spreads: the sand seas' noise threshold is lowered by this,
/// and steppes get them too.
const BARE_ERG: f32 = 0.3;

/// Karst of each cell: towers (positive) where limestone meets the humid tropics and
/// subtropics, sinkholes (negative) in other wet climates; none in dry or polar ones.
pub(super) fn karst(planet: &Planet, koppen: &[crate::climate::Koppen]) -> Vec<f32> {
    use crate::{climate::Koppen, geology::Rock};
    let raw: Vec<f32> = (0..planet.grid.len())
        .map(|i| {
            if planet.sea[i] || Rock::from_id(planet.rock[i]) != Rock::Limestone {
                return 0.;
            }
            match koppen[i] {
                Koppen::Af | Koppen::Am | Koppen::Aw | Koppen::Cwa | Koppen::Cfa => 1.,
                Koppen::Cfb
                | Koppen::Cfc
                | Koppen::Csa
                | Koppen::Csb
                | Koppen::Dfa
                | Koppen::Dfb
                | Koppen::Dw => -1.,
                _ => 0.,
            }
        })
        .collect();
    planet.grid.blur(&raw, 2)
}

/// Sand seas (ergs) and badlands of each cell (0..1), from the climate classes.
pub(super) fn deserts(planet: &Planet, koppen: &[crate::climate::Koppen]) -> (Vec<f32>, Vec<f32>) {
    use crate::{climate::Koppen, geology::Rock};
    let grid = &planet.grid;
    let radius = planet.radius();
    let noise = Noise::new(planet.settings.seed as u32 ^ 0xE6C5);
    let bare = !planet.settings.world.life;
    let threshold = if bare { -0.05 - BARE_ERG } else { -0.05 };
    let raw: Vec<(f32, f32)> = crate::par::map(grid.len(), |i| {
        let desert = matches!(koppen[i], Koppen::BWh | Koppen::BWk);
        let steppe = matches!(koppen[i], Koppen::BSh | Koppen::BSk);
        if planet.sea[i] || !desert && !steppe {
            return (0., 0.);
        }
        let slope = grid
            .neighbours(i)
            .map(|(j, d)| (planet.heights[j] - planet.heights[i]).abs() / d)
            .fold(0., f32::max);
        let world = grid.direction(i) * radius;
        let erg = if desert || bare && steppe {
            smoothstep(
                threshold,
                threshold + 0.3,
                noise.fbm3(world / ERG_WAVELENGTH, 3),
            ) * (1. - smoothstep(ERG_SLOPE.0, ERG_SLOPE.1, slope))
                * (1. - smoothstep(ERG_HEIGHT.0, ERG_HEIGHT.1, planet.heights[i]))
        } else {
            0.
        };
        let soft = matches!(Rock::from_id(planet.rock[i]), Rock::Shale | Rock::Sandstone);
        let badlands = if soft {
            smoothstep(BADLANDS_SLOPE.0, BADLANDS_SLOPE.1, slope)
                * smoothstep(-0.2, 0.2, noise.layer(1).fbm3(world / 50., 2))
        } else {
            0.
        };
        (erg, badlands)
    });
    // (Blurred wide: the climate classes' borders are coarse.)
    let erg = grid.blur(&raw.iter().map(|r| r.0).collect::<Vec<_>>(), 5);
    let badlands = grid.blur(&raw.iter().map(|r| r.1).collect::<Vec<_>>(), 1);
    (erg, badlands)
}

/// Sea and lake ice (see [`PlanetSurface::sea_ice`]): from the warmest month at the water's level.
pub(super) fn sea_ice(planet: &Planet, climate: &PlanetClimate) -> Vec<f32> {
    let colder = EARTH_SEA_FREEZING - planet.settings.world.sea_freezing();
    // (Lakes too: at their water's height. Fresh water: as Earth's sea.)
    let raw = crate::par::map(planet.grid.len(), |i| {
        let lake = planet.lake_depth[i] > 0.02;
        if !planet.sea[i] && !lake {
            return 0.;
        }
        let level = if lake {
            planet.heights[i] + planet.lake_depth[i]
        } else {
            0.
        };
        let (t, _) = climate.months_at(planet.grid.direction(i), level);
        let warmest = t.iter().copied().fold(f32::MIN, f32::max);
        let colder = if lake { 0. } else { colder };
        smoothstep(
            SEA_ICE_WARMEST.0 - colder,
            SEA_ICE_WARMEST.1 - colder,
            warmest,
        )
    });
    planet.grid.blur(&raw, 2)
}

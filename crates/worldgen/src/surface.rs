//! Sampling the planet's surface between its cells: each cube face as a 2D height grid,
//! padded with the neighbouring faces' cells (found by direction), interpolated bicubically
//! in the face's cell coordinates. The game meshes the planet face by face through it.

use std::collections::HashMap;

use bevy_math::{IVec3, Vec2, Vec3};

use crate::{
    detail::gullies,
    grid::Grid,
    noise::{Noise, hash, smoothstep},
    planet::Planet,
    climate::{PlanetClimate, WindField},
    rivers::PlanetRiver,
    springs::{PlanetSpring, Springs, find_springs},
    sphere::{FACES, SphereGrid},
};

/// Cells of padding around each face: the bicubic stencil reaches two cells out, and fine
/// detail near a face's edge is also evaluated on the neighbouring face, up to
/// [`EDGE_BLEND`] cells past its edge.
const PAD: usize = 14;
/// Fine detail: gully wavelength (cells), octaves and depth (world units).
const DETAIL_WAVELENGTH: f32 = 8.;
const DETAIL_OCTAVES: u32 = 3;
const DETAIL_AMPLITUDE: f32 = 1.2;
/// Dunes: wavelength (world units, at `DUNE_LATITUDE`), height (world units) and where along
/// a wavelength the crest is (the slip face beyond it).
const DUNE_WAVELENGTH: f32 = 5.;
const DUNE_LATITUDE: f32 = 0.45;
const DUNE_HEIGHT: f32 = 0.6;
const DUNE_CREST: f32 = 0.78;
/// River banks reach `RIPARIAN_REACH.0` plus `RIPARIAN_REACH.1` × the river's width (world
/// units) from its centre.
const RIPARIAN_REACH: (f32, f32) = (1.5, 1.2);

/// How near each cell is to a river (see [`PlanetSurface::riparian`]).
fn riparian(planet: &Planet) -> Vec<f32> {
    let grid = &planet.grid;
    let radius = planet.radius();
    let mut near = vec![0f32; grid.len()];
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

/// Sea ice: none where the warmest month at sea level reaches the first (°C), pack ice where
/// it stays below the second.
const SEA_ICE_WARMEST: (f32, f32) = (4., 0.);

/// Karst: features this far apart (world units), towers this high, sinkholes this deep (world
/// units) round this share of the features.
const KARST_SPACING: f32 = 4.5;
const KARST_TOWER: f32 = 2.6;
const KARST_SINKHOLE: f32 = 0.5;
const KARST_SINKHOLES: f32 = 0.35;

/// Karst of each cell: towers (positive) where limestone meets the humid tropics and
/// subtropics, sinkholes (negative) in other wet climates; none in dry or polar ones.
fn karst(planet: &Planet, koppen: &[crate::climate::Koppen]) -> Vec<f32> {
    use crate::{geology::Rock, climate::Koppen};
    let raw: Vec<f32> = (0..planet.grid.len())
        .map(|i| {
            if planet.sea[i] || Rock::from_id(planet.rock[i]) != Rock::Limestone {
                return 0.;
            }
            match koppen[i] {
                Koppen::Af | Koppen::Am | Koppen::Aw | Koppen::Cwa | Koppen::Cfa => 1.,
                Koppen::Cfb | Koppen::Cfc | Koppen::Csa | Koppen::Csb | Koppen::Dfa | Koppen::Dfb | Koppen::Dw => -1.,
                _ => 0.,
            }
        })
        .collect();
    planet.grid.blur(&raw, 2)
}

/// Badlands' gullies are this much deeper.
const BADLANDS_GULLIES: f32 = 1.5;
/// Sand seas: in desert climates (BWh, BWk) where a noise (wavelength `ERG_WAVELENGTH`, world
/// units) is high, on gentle ground (slope below `ERG_SLOPE`) below `ERG_HEIGHT`.
const ERG_WAVELENGTH: f32 = 90.;
const ERG_SLOPE: (f32, f32) = (0.12, 0.35);
const ERG_HEIGHT: (f32, f32) = (12., 20.);
/// Badlands: soft rocks (shale, sandstone) in dry climates (deserts and steppes), on slopes
/// steeper than `BADLANDS_SLOPE`, in stretches.
const BADLANDS_SLOPE: (f32, f32) = (0.15, 0.4);

/// Sand seas (ergs) and badlands of each cell (0..1), from the climate classes.
fn deserts(planet: &Planet, koppen: &[crate::climate::Koppen]) -> (Vec<f32>, Vec<f32>) {
    use crate::{geology::Rock, climate::Koppen};
    let grid = &planet.grid;
    let radius = planet.radius();
    let noise = Noise::new(planet.settings.seed as u32 ^ 0xE6C5);
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
        let erg = if desert {
            smoothstep(-0.05, 0.25, noise.fbm3(world / ERG_WAVELENGTH, 3))
                * (1. - smoothstep(ERG_SLOPE.0, ERG_SLOPE.1, slope))
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
/// Near a face's edge, fine detail blends with the neighbouring face's over this many cells
/// (each face's gullies follow its own cell axes).
const EDGE_BLEND: f32 = 8.;

/// A per-cell field of the planet, sampled continuously face by face.
#[derive(Clone)]
pub struct FaceField {
    n: usize,
    /// Each face's values with [`PAD`] cells of padding, `(n + 2 PAD)²`, row-major.
    faces: Vec<Vec<f32>>,
}

impl FaceField {
    /// `values` per cell of the planet's grid.
    pub fn new(planet: &Planet, values: &[f32]) -> Self {
        let grid = &planet.grid;
        let n = grid.n();
        let side = n + 2 * PAD;
        let faces = (0..FACES)
            .map(|face| {
                (0..side * side)
                    .map(|k| {
                        let (x, z) = ((k % side) as i64 - PAD as i64, (k / side) as i64 - PAD as i64);
                        if x >= 0 && z >= 0 && (x as usize) < n && (z as usize) < n {
                            values[grid.index(face, x as usize, z as usize)]
                        } else {
                            // Off the face: the cell in that direction, on a neighbouring face.
                            let direction = grid.direction_at(face, Vec2::new(x as f32, z as f32));
                            values[grid.cell_at(direction)]
                        }
                    })
                    .collect()
            })
            .collect();
        Self { n, faces }
    }

    fn at(&self, face: usize, x: i64, z: i64) -> f32 {
        let side = (self.n + 2 * PAD) as i64;
        let (x, z) = ((x + PAD as i64).clamp(0, side - 1), (z + PAD as i64).clamp(0, side - 1));
        self.faces[face][(z * side + x) as usize]
    }

    /// Bicubic (Catmull-Rom) value at continuous cell coordinates of a face (cell centres at
    /// whole numbers).
    pub fn bicubic(&self, face: usize, p: Vec2) -> f32 {
        let cell = p.floor();
        let t = p - cell;
        let (x, z) = (cell.x as i64, cell.y as i64);
        let row = |dz: i64| {
            catmull_rom(
                [
                    self.at(face, x - 1, z + dz),
                    self.at(face, x, z + dz),
                    self.at(face, x + 1, z + dz),
                    self.at(face, x + 2, z + dz),
                ],
                t.x,
            )
        };
        catmull_rom([row(-1), row(0), row(1), row(2)], t.y)
    }

    /// The nearest cell's value at continuous cell coordinates of a face.
    pub fn nearest(&self, face: usize, p: Vec2) -> f32 {
        self.at(face, p.x.round() as i64, p.y.round() as i64)
    }

    /// Bilinear value at continuous cell coordinates of a face.
    pub fn bilinear(&self, face: usize, p: Vec2) -> f32 {
        let cell = p.floor();
        let t = p - cell;
        let (x, z) = (cell.x as i64, cell.y as i64);
        let bottom = self.at(face, x, z) + (self.at(face, x + 1, z) - self.at(face, x, z)) * t.x;
        let top =
            self.at(face, x, z + 1) + (self.at(face, x + 1, z + 1) - self.at(face, x, z + 1)) * t.x;
        bottom + (top - bottom) * t.y
    }
}

/// The planet's surface for rendering: heights (with fine detail: roughness and gullies, see
/// `detail`) and lake depths between cells.
#[derive(Clone)]
pub struct PlanetSurface {
    pub n: usize,
    /// Radius of the sea level (world units).
    pub radius: f32,
    pub heights: FaceField,
    pub lake_depth: FaceField,
    pub ruggedness: FaceField,
    pub calm: FaceField,
    /// River banks (0..1: 1 at the water, fading over `RIPARIAN_REACH`, wider along bigger
    /// rivers): green strips and gallery forests in dry country.
    pub riparian: FaceField,
    /// Ground colour of each cell (linear RGB), from its climate (Köppen class at its altitude);
    /// `None` without a climate.
    pub ground: Option<[FaceField; 3]>,
    /// The climate class of each cell at its height (`Koppen::id`; see
    /// [`PlanetSurface::koppen_at`]).
    pub koppen: Option<FaceField>,
    /// Ground material weights of each cell from its climate (see `Koppen::ground_weights`).
    pub weights: Option<[FaceField; 5]>,
    /// The rock of each cell (`geology::Rock` id; see [`PlanetSurface::rock_at`]), fresh lava
    /// (0..1, round active volcanoes) and the volcanoes.
    pub rock: FaceField,
    pub lava: FaceField,
    /// Geothermal fields (hot springs: 0..1, see `volcanoes::Built::geothermal`).
    pub geothermal: FaceField,
    pub volcanoes: Vec<crate::volcanoes::Volcano>,
    /// Coral reef (0..1, see `reefs`).
    pub reef: FaceField,
    /// Glacier ice (0..1, see `glaciers`; its thickness is in the heights).
    pub ice: FaceField,
    /// Sea and lake ice (0..1): pack ice where the warmest month at the water's level stays
    /// below `SEA_ICE_WARMEST.1`, thinning out to `SEA_ICE_WARMEST.0`; frozen lakes alike.
    pub sea_ice: FaceField,
    /// Deserts (see [`deserts`]): sand seas (ergs, with dunes in the heights) and badlands
    /// (0..1); the rest of a hot or cold desert is stony (reg).
    pub erg: FaceField,
    pub badlands: FaceField,
    /// Karst on limestone in wet climates (see [`karst`]): tower karst (positive, humid
    /// tropics and subtropics) or sinkholes (negative, elsewhere), in the heights.
    pub karst: FaceField,
    /// The ocean (1) and land (0), blurred a little: near 0.5 at the coast, small but not 0
    /// on land near it. Land below sea level (dry basins) is land.
    pub ocean: FaceField,
    /// Wetlands: marsh (positive, where the climate is wet enough) or salt flats (negative:
    /// closed flats and basins in dry climates), from `Planet::wetland` / `basins`.
    pub marsh: FaceField,
    /// The climate's prevailing winds (see [`PlanetSurface::wind_at`]).
    wind: Option<std::sync::Arc<WindField>>,
    /// The rivers, their water levels kept under the surface across their channel.
    pub rivers: Vec<PlanetRiver>,
    channels: Channels,
    /// River springs (ponds, hillside notches), carved like the channels.
    springs: Springs,
    grid: std::sync::Arc<SphereGrid>,
    seed: u32,
}

impl PlanetSurface {
    pub fn new(planet: &Planet, climate: Option<&PlanetClimate>) -> Self {
        let ground = climate.map(|climate| {
            let colours: Vec<[f32; 3]> = crate::par::map(planet.grid.len(), |i| {
                let [r, g, b] = climate
                    .koppen_at(planet.grid.direction(i), planet.heights[i])
                    .ground_color();
                [r, g, b].map(srgb_to_linear)
            });
            [0, 1, 2].map(|k| {
                FaceField::new(planet, &colours.iter().map(|c| c[k]).collect::<Vec<_>>())
            })
        });
        let koppen = climate.map(|climate| {
            crate::par::map(planet.grid.len(), |i| climate.koppen_at(planet.grid.direction(i), planet.heights[i]))
        });
        let weights = koppen.as_ref().map(|koppen| koppen.iter().map(|k| k.ground_weights()).collect::<Vec<_>>());
        let koppen_cells = koppen.clone();
        let koppen = koppen.map(|koppen| {
            FaceField::new(planet, &koppen.iter().map(|k| k.id() as f32).collect::<Vec<_>>())
        });
        // Marsh where wet ground meets a wet enough climate (not frozen), salt flats on closed
        // flats where it is dry.
        let marsh: Vec<f32> = (0..planet.grid.len())
            .map(|i| {
                let [dryness, _, _, snow, _] = weights.as_ref().map_or([0.3, 0., 0., 0., 0.], |w| w[i]);
                let moist = 1. - smoothstep(0.5, 0.85, dryness);
                let marsh = planet.wetland[i] * moist * (1. - snow);
                marsh - planet.basins[i] * (1. - moist)
            })
            .collect();
        let ocean: Vec<f32> = planet.sea.iter().map(|&s| if s { 1. } else { 0. }).collect();
        let ocean = planet.grid.blur(&ocean, 3);
        // Glaciers and ice caps: the ice on the ground (smooth: no gullies), no lakes or
        // marsh under it.
        let thickness =
            climate.map_or_else(|| vec![0.; planet.grid.len()], |climate| crate::glaciers::glaciers(planet, climate));
        let ice: Vec<f32> = thickness.iter().map(|&t| smoothstep(0.05, 0.25, t)).collect();
        let heights: Vec<f32> = planet.heights.iter().zip(&thickness).map(|(h, t)| h + t).collect();
        let lake_depth: Vec<f32> =
            planet.lake_depth.iter().zip(&ice).map(|(&d, &ice)| if ice > 0. { 0. } else { d }).collect();
        let calm: Vec<f32> = planet.calm.iter().zip(&ice).map(|(c, i)| c.max(*i)).collect();
        let (erg, badlands) = match &koppen_cells {
            Some(koppen) => deserts(planet, koppen),
            None => (vec![0.; planet.grid.len()], vec![0.; planet.grid.len()]),
        };
        let sea_ice: Vec<f32> = match climate {
            Some(climate) => {
                // (Lakes too: at their water's height.)
                let raw = crate::par::map(planet.grid.len(), |i| {
                    let lake = planet.lake_depth[i] > 0.02;
                    if !planet.sea[i] && !lake {
                        return 0.;
                    }
                    let level = if lake { planet.heights[i] + planet.lake_depth[i] } else { 0. };
                    let (t, _) = climate.months_at(planet.grid.direction(i), level);
                    let warmest = t.iter().copied().fold(f32::MIN, f32::max);
                    smoothstep(SEA_ICE_WARMEST.0, SEA_ICE_WARMEST.1, warmest)
                });
                planet.grid.blur(&raw, 2)
            }
            None => vec![0.; planet.grid.len()],
        };
        let karst_cells = match &koppen_cells {
            Some(koppen) => karst(planet, koppen),
            None => vec![0.; planet.grid.len()],
        };
        let marsh: Vec<f32> = marsh.iter().zip(&ice).map(|(m, i)| m * (1. - i)).collect();
        let weights = weights.map(|weights: Vec<[f32; 5]>| {
            [0, 1, 2, 3, 4].map(|k| {
                FaceField::new(planet, &weights.iter().map(|w| w[k]).collect::<Vec<_>>())
            })
        });
        let mut surface = Self {
            wind: climate.map(|climate| std::sync::Arc::new(climate.wind_field())),
            weights,
            koppen,
            ground,
            ocean: FaceField::new(planet, &ocean),
            rock: FaceField::new(planet, &planet.rock.iter().map(|&r| r as f32).collect::<Vec<_>>()),
            lava: FaceField::new(planet, &planet.lava),
            geothermal: FaceField::new(planet, &planet.geothermal),
            reef: FaceField::new(planet, &planet.reef),
            ice: FaceField::new(planet, &ice),
            sea_ice: FaceField::new(planet, &sea_ice),
            erg: FaceField::new(planet, &erg),
            badlands: FaceField::new(planet, &badlands),
            karst: FaceField::new(planet, &karst_cells),
            volcanoes: planet.volcanoes.clone(),
            marsh: FaceField::new(planet, &marsh),
            rivers: Vec::new(),
            channels: Channels::default(),
            springs: Springs::default(),
            n: planet.grid.n(),
            radius: planet.radius(),
            heights: FaceField::new(planet, &heights),
            lake_depth: FaceField::new(planet, &lake_depth),
            ruggedness: FaceField::new(planet, &planet.ruggedness),
            calm: FaceField::new(planet, &calm),
            riparian: FaceField::new(planet, &riparian(planet)),
            grid: std::sync::Arc::new(SphereGrid::new(planet.grid.n())),
            seed: planet.settings.seed as u32 ^ 0xD37A,
        };
        // River water levels: never above the ground across the channel (smoothing and
        // meanders move points over lower ground), never rising downstream.
        let mut rivers = planet.rivers.clone();
        // Rivers start at the glaciers' snouts (meltwater), not under the ice.
        for river in &mut rivers {
            let under = river.points.iter().take_while(|p| surface.ice_at(p.direction) > 0.5).count();
            let keep = river.points.len().saturating_sub(1);
            river.points.drain(..under.min(keep));
        }
        for river in &mut rivers {
            let mut previous = f32::MAX;
            let count = river.points.len();
            for k in 0..count {
                let point = river.points[k];
                let (before, after) =
                    (river.points[k.saturating_sub(1)], river.points[(k + 1).min(count - 1)]);
                let up = point.direction;
                let across = (after.direction - before.direction).cross(up).normalize_or_zero();
                let lowest = [-0.5f32, 0., 0.5]
                    .map(|side| {
                        let q = (up * surface.radius + across * side * point.width).normalize();
                        // (In a lake, its water: rivers run through lakes at their level.)
                        surface.uncarved_height(q) + surface.lake_depth_at(q)
                    })
                    .into_iter()
                    .fold(f32::MAX, f32::min);
                let level = point.level.min(lowest).min(previous);
                river.points[k].level = level;
                previous = level;
            }
        }
        // Rivers end where they reach the sea (the shaped coast may lie upstream of the cell
        // they drained into).
        for river in &mut rivers {
            let reached = river
                .points
                .iter()
                .position(|p| {
                    surface.uncarved_height(p.direction) < -SEA_REACHED && surface.ocean_at(p.direction) > 0.5
                });
            if let Some(k) = reached {
                river.points.truncate(k + 1);
                river.end = crate::RiverEnd::Sea;
            }
            // A distributary starting in the sea is a piece cut off its delta: none.
            if river.distributary && reached.is_some_and(|k| k < 2) {
                river.points.truncate(1);
            }
        }
        // (Rivers left with a single point stay: removing them would shift the indices other
        // rivers join by.)
        surface.channels = Channels::new(&rivers, surface.radius);
        let springs = find_springs(
            &rivers,
            surface.radius,
            |d| surface.uncarved_height(d),
            |d| surface.lake_depth_at(d),
            |d| surface.ocean_at(d),
        );
        surface.springs = Springs::new(springs, surface.radius);
        surface.rivers = rivers;
        surface
    }

    /// Depth of the lake water in a direction (0: no lake).
    pub fn lake_depth_at(&self, direction: Vec3) -> f32 {
        let (face, p) = self.face_coords(direction);
        self.lake_depth.bilinear(face, p)
    }

    /// How much of a river's channel or a lake covers a point (0..1): the beds.
    pub fn wetness(&self, face: usize, p: Vec2) -> f32 {
        let lake = smoothstep(0.02, 0.1, self.lake_depth.bilinear(face, p));
        let world = self.direction(face, p) * self.radius;
        lake.max(self.channels.wetness(world)).max(self.springs.carve(world, 0.).1)
    }

    /// Height in a direction, before the river channels are carved.
    pub fn uncarved_height(&self, direction: Vec3) -> f32 {
        let (face, p) = self.face_coords(direction);
        self.detailed_height(face, p, direction)
    }

    /// Height above sea level at continuous cell coordinates of a face: the cells' heights
    /// (bicubic), with fine detail on rugged land.
    pub fn height(&self, face: usize, p: Vec2) -> f32 {
        let direction = self.direction(face, p);
        let height = self.detailed_height(face, p, direction);
        let world = direction * self.radius;
        self.springs.carve(world, self.channels.carve(world, height)).0
    }

    /// The cells' heights with fine detail (no river channels).
    fn detailed_height(&self, face: usize, p: Vec2, direction: Vec3) -> f32 {
        let height = self.heights.bicubic(face, p);
        if height <= 0. {
            return height;
        }
        let calm = 1. - self.calm.bilinear(face, p).clamp(0., 1.);
        // Dunes on the sand seas.
        let erg = self.erg.bilinear(face, p);
        let height = if erg > 0.01 { height + self.dunes(direction) * erg * calm } else { height };
        // Karst: towers or sinkholes.
        let karst = self.karst.bilinear(face, p);
        let height = if karst.abs() > 0.01 { height + self.karst_relief(direction, karst) * calm } else { height };
        // Badlands: dense gullies.
        let badlands = self.badlands.bilinear(face, p);
        let strength = self.ruggedness.bilinear(face, p).max(badlands) * calm;
        if strength <= 0.01 {
            return height;
        }
        let noise = Noise::new(self.seed);
        // Small irregular bumps (3D noise: seamless across faces)...
        let world = direction * (2. * self.n as f32 / std::f32::consts::PI);
        let roughness = noise.fbm3(world / 5., 3) * 0.12;
        // ...and gullies on slopes, blended with the neighbouring face's near an edge.
        let edge = [p.x + 0.5, self.n as f32 - 0.5 - p.x, p.y + 0.5, self.n as f32 - 0.5 - p.y];
        let (nearest, distance) = edge
            .iter()
            .enumerate()
            .fold((0, f32::MAX), |best, (k, &d)| if d < best.1 { (k, d) } else { best });
        let mut gully = self.gullies_on(face, p, world);
        if distance < EDGE_BLEND {
            // The neighbouring face across the nearest edge: the direction just past it.
            let outward = [-Vec2::X, Vec2::X, -Vec2::Y, Vec2::Y][nearest];
            let beyond = self.direction(face, p + outward * (distance + 1.));
            let (other, _) = self.face_coords(beyond);
            if other != face {
                let q = self.grid.coords_on_face(other, direction);
                let own = 0.5 + 0.5 * smoothstep(0., EDGE_BLEND, distance);
                gully = gully * own + self.gullies_on(other, q, world) * (1. - own);
            }
        }
        height + (gully * (1. + BADLANDS_GULLIES * badlands) + roughness) * DETAIL_AMPLITUDE * strength
    }

    /// Karst relief (world units) in a direction: steep limestone towers with rounded tops
    /// (`karst` positive), or round sinkholes (negative), scattered on a jittered lattice
    /// (`KARST_SPACING`).
    fn karst_relief(&self, direction: Vec3, karst: f32) -> f32 {
        let q = direction * self.radius / KARST_SPACING;
        let cell = q.floor().as_ivec3();
        let seed = self.seed ^ 0x6A25;
        let random = |c: IVec3, k: u32| {
            hash(hash(seed ^ k, c.x as u32, c.y as u32), c.z as u32, k) as f32 / u32::MAX as f32
        };
        let mut relief = 0f32;
        for dz in -1..=1 {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let c = cell + IVec3::new(dx, dy, dz);
                    let feature = c.as_vec3() + Vec3::new(random(c, 1), random(c, 2), random(c, 3));
                    let d = feature.distance(q);
                    let size = random(c, 4);
                    if karst > 0. {
                        // Towers, of varied height and girth.
                        let radius = 0.28 + 0.14 * size;
                        let tower = smoothstep(radius, radius * 0.55, d).powf(0.6);
                        relief = relief.max(KARST_TOWER * (0.4 + 0.6 * size) * tower);
                    } else if size < KARST_SINKHOLES {
                        let bowl = smoothstep(0.35, 0., d);
                        relief = relief.min(-KARST_SINKHOLE * bowl * bowl);
                    }
                }
            }
        }
        relief * karst.abs()
    }

    /// Dune height (world units) in a direction: transverse dunes, crests running north–south
    /// (across the trade winds blowing from the east: gentle slopes facing east, steep slip
    /// faces west), sinuous and of varied size. A whole number of dunes round the planet keeps
    /// them seamless.
    fn dunes(&self, direction: Vec3) -> f32 {
        let count = (std::f32::consts::TAU * self.radius * DUNE_LATITUDE.cos() / DUNE_WAVELENGTH).round();
        let longitude = direction.z.atan2(direction.x);
        let world = direction * self.radius;
        let noise = Noise::new(self.seed ^ 0xD0E5);
        let warp = 1.2 * noise.fbm3(world / 14., 2) + 2. * noise.layer(1).fbm3(world / 40., 2);
        let along = (-longitude / std::f32::consts::TAU * count + warp).rem_euclid(1.);
        let profile = if along < DUNE_CREST {
            smoothstep(0., DUNE_CREST, along)
        } else {
            1. - smoothstep(DUNE_CREST, 1., along)
        };
        let size = 0.65 + 0.35 * noise.layer(2).fbm3(world / 25., 2);
        DUNE_HEIGHT * profile * size
    }

    /// Gullies (about -1..1, already masked by slope and patchiness) at cell coordinates `p` of
    /// `face`, following the slope of the cells' heights.
    fn gullies_on(&self, face: usize, p: Vec2, world: Vec3) -> f32 {
        let e = 0.5;
        let gradient = Vec2::new(
            self.heights.bicubic(face, p + Vec2::X * e) - self.heights.bicubic(face, p - Vec2::X * e),
            self.heights.bicubic(face, p + Vec2::Y * e) - self.heights.bicubic(face, p - Vec2::Y * e),
        ) / (2. * e);
        let slope_mask = smoothstep(0.12, 0.6, gradient.length());
        if slope_mask <= 0. {
            return 0.;
        }
        let noise = Noise::new(self.seed);
        let patchiness = smoothstep(-0.4, 0.5, noise.layer(1).fbm3(world / 45., 3));
        gullies(p, gradient, self.seed, DETAIL_WAVELENGTH, DETAIL_OCTAVES)
            * slope_mask
            * (0.3 + 0.7 * patchiness)
    }

    /// Unit direction of continuous cell coordinates of a face.
    pub fn direction(&self, face: usize, p: Vec2) -> Vec3 {
        self.grid.direction_at(face, p)
    }

    /// World position of the surface at continuous cell coordinates of a face (the planet's
    /// centre at the origin).
    pub fn position(&self, face: usize, p: Vec2) -> Vec3 {
        self.direction(face, p) * (self.radius + self.height(face, p))
    }

    /// Height above sea level in a direction from the planet's centre.
    pub fn height_at(&self, direction: Vec3) -> f32 {
        let (face, p) = self.face_coords(direction);
        self.height(face, p)
    }

    /// The face and cell coordinates of a direction.
    pub fn face_coords(&self, direction: Vec3) -> (usize, Vec2) {
        self.grid.face_coords(direction)
    }

    /// The wind at continuous cell coordinates of a face: the climate's prevailing wind
    /// (tangent vector, m/s) and strength (mean speed, m/s), stronger on ground standing above
    /// its surroundings (ridges, summits), weaker in hollows and valleys, and turned along
    /// steep slopes rather than up them. A light westerly without a climate.
    pub fn wind_at(&self, face: usize, p: Vec2) -> (Vec3, f32) {
        let direction = self.direction(face, p);
        let Some(field) = &self.wind else {
            let east = Vec3::Y.cross(direction).normalize_or_zero();
            return (east * 5., 5.);
        };
        let (mut wind, strength) = field.at(direction);
        let height = self.heights.bilinear(face, p);
        if height <= 0. {
            return (wind, strength);
        }
        // Exposure: height above the mean of a ring around.
        let ring = (0..8)
            .map(|k| {
                let q = p + Vec2::from_angle(k as f32 * std::f32::consts::FRAC_PI_4) * WIND_RING;
                self.heights.bilinear(face, q).max(0.)
            })
            .sum::<f32>()
            / 8.;
        let exposure = 1. + WIND_EXPOSURE.0 * ((height - ring) / WIND_EXPOSURE.1).tanh();
        // Steep slopes: the wind blowing up them turns along them.
        let tx = (self.direction(face, p + Vec2::X) - self.direction(face, p - Vec2::X)) * self.radius / 2.;
        let ty = (self.direction(face, p + Vec2::Y) - self.direction(face, p - Vec2::Y)) * self.radius / 2.;
        let dx = (self.heights.bilinear(face, p + Vec2::X) - self.heights.bilinear(face, p - Vec2::X)) / 2.;
        let dy = (self.heights.bilinear(face, p + Vec2::Y) - self.heights.bilinear(face, p - Vec2::Y)) / 2.;
        let slope = tx * (dx / tx.length_squared()) + ty * (dy / ty.length_squared());
        let steepness = slope.length();
        if steepness > 1e-4 {
            let uphill = slope / steepness;
            let up = wind.dot(uphill).max(0.);
            let turned = up * smoothstep(WIND_STEEP.0, WIND_STEEP.1, steepness);
            let along = direction.cross(uphill);
            wind += -uphill * turned + along * turned * wind.dot(along).signum();
        }
        (wind * exposure, strength * exposure)
    }

    /// The climate class at a point of a face (the nearest cell's), `None` without a climate.
    pub fn koppen_at(&self, face: usize, p: Vec2) -> Option<crate::climate::Koppen> {
        let id = self.koppen.as_ref()?.nearest(face, p);
        Some(crate::climate::Koppen::from_id(id.round() as u8))
    }

    /// Glacier ice in a direction (0..1, see [`PlanetSurface::ice`]).
    pub fn ice_at(&self, direction: Vec3) -> f32 {
        let (face, p) = self.face_coords(direction);
        self.ice.bilinear(face, p)
    }

    /// The ocean in a direction (see [`PlanetSurface::ocean`]): above 0.5 at sea.
    pub fn ocean_at(&self, direction: Vec3) -> f32 {
        let (face, p) = self.face_coords(direction);
        self.ocean.bilinear(face, p)
    }

    /// The rock at continuous cell coordinates of a face (the nearest cell's).
    pub fn rock_at(&self, face: usize, p: Vec2) -> crate::geology::Rock {
        crate::geology::Rock::from_id(self.rock.bilinear(face, p.round()).round() as u8)
    }

    /// The river springs (see `springs`).
    pub fn springs(&self) -> &[PlanetSpring] {
        &self.springs.springs
    }

    /// Seed of the surface's details (for things placed on it).
    pub fn seed(&self) -> u32 {
        self.seed
    }

    /// Number of cells (for checks).
    pub fn cells(&self) -> usize {
        self.grid.len()
    }
}

/// Depth of the sea floor where a river has reached the sea.
const SEA_REACHED: f32 = 0.05;
/// Bank width relative to the channel width (at least [`MIN_BANK`]), widened to keep the
/// bank's mean slope under [`MAX_BANK_SLOPE`], up to [`MAX_BANK`].
const BANK_PER_WIDTH: f32 = 1.;
const MIN_BANK: f32 = 0.8;
const MAX_BANK_SLOPE: f32 = 0.5;
const MAX_BANK: f32 = 6.;
/// Size of the lookup buckets of river segments (world units).
const BUCKET: f32 = 4.;

/// A straight piece of river between two points (world positions at sea level radius).
#[derive(Clone)]
struct Segment {
    a: Vec3,
    b: Vec3,
    level: (f32, f32),
    width: (f32, f32),
    depth: (f32, f32),
}

/// The river channels, carved into the surface: segments bucketed by position.
#[derive(Clone, Default)]
struct Channels {
    segments: Vec<Segment>,
    buckets: HashMap<IVec3, Vec<u32>>,
}

impl Channels {
    fn new(rivers: &[PlanetRiver], radius: f32) -> Self {
        let mut channels = Self::default();
        for river in rivers {
            for pair in river.points.windows(2) {
                let (p, q) = (pair[0], pair[1]);
                let segment = Segment {
                    a: p.direction * radius,
                    b: q.direction * radius,
                    level: (p.level, q.level),
                    // (Channels narrower than the terrain mesh shows come out as holes.)
                    width: (p.width.max(MIN_CARVED_WIDTH), q.width.max(MIN_CARVED_WIDTH)),
                    depth: (p.depth, q.depth),
                };
                let reach = reach(p.width.max(q.width));
                let (lo, hi) = (
                    ((segment.a.min(segment.b) - reach) / BUCKET).floor().as_ivec3(),
                    ((segment.a.max(segment.b) + reach) / BUCKET).floor().as_ivec3(),
                );
                let id = channels.segments.len() as u32;
                for x in lo.x..=hi.x {
                    for y in lo.y..=hi.y {
                        for z in lo.z..=hi.z {
                            channels.buckets.entry(IVec3::new(x, y, z)).or_default().push(id);
                        }
                    }
                }
                channels.segments.push(segment);
            }
        }
        channels
    }

    /// Carve the channels at `p` (a point at sea level radius) into `height`: a parabolic
    /// channel under the water line, then banks rising back to the terrain.
    fn carve(&self, p: Vec3, height: f32) -> f32 {
        let Some(ids) = self.buckets.get(&(p / BUCKET).floor().as_ivec3()) else {
            return height;
        };
        let base = height;
        let mut height = height;
        for &id in ids {
            let s = &self.segments[id as usize];
            let ab = s.b - s.a;
            let u = ((p - s.a).dot(ab) / ab.length_squared().max(1e-9)).clamp(0., 1.);
            let distance = p.distance(s.a + ab * u);
            let width = s.width.0 + (s.width.1 - s.width.0) * u;
            if distance >= reach(width) {
                continue;
            }
            let water = s.level.0 + (s.level.1 - s.level.0) * u;
            let half = width / 2.;
            let carved = if distance < half {
                let depth = s.depth.0 + (s.depth.1 - s.depth.0) * u;
                water - depth * (1. - (distance / half).powi(2))
            } else {
                let rise = (base - water).max(0.);
                let bank = (width * BANK_PER_WIDTH)
                    .max(MIN_BANK)
                    .max(rise / MAX_BANK_SLOPE)
                    .min(MAX_BANK.max(width * BANK_PER_WIDTH));
                let t = ((distance - half) / bank).clamp(0., 1.);
                let ease_out = 1. - (1. - t) * (1. - t);
                let smooth = t * t * (3. - 2. * t);
                water + rise * 0.5 * (ease_out + smooth)
            };
            height = height.min(carved);
        }
        height
    }
}

impl Channels {
    /// 1 in a channel, fading out just past its edge.
    fn wetness(&self, p: Vec3) -> f32 {
        let Some(ids) = self.buckets.get(&(p / BUCKET).floor().as_ivec3()) else {
            return 0.;
        };
        let mut wet: f32 = 0.;
        for &id in ids {
            let s = &self.segments[id as usize];
            let ab = s.b - s.a;
            let u = ((p - s.a).dot(ab) / ab.length_squared().max(1e-9)).clamp(0., 1.);
            let half = (s.width.0 + (s.width.1 - s.width.0) * u) / 2.;
            let distance = p.distance(s.a + ab * u);
            wet = wet.max(((half + 0.3 - distance) / 0.3).clamp(0., 1.));
        }
        wet
    }
}

/// Local wind: the ring (cells) the exposure compares a point's height with, the most the
/// wind speeds up or slows down (share) and the height difference (world units) for most of
/// it, and the slopes over which the wind turns along them instead of blowing up.
const WIND_RING: f32 = 6.;
const WIND_EXPOSURE: (f32, f32) = (0.5, 4.);
const WIND_STEEP: (f32, f32) = (0.3, 1.);

/// Channels are carved at least this wide (world units).
const MIN_CARVED_WIDTH: f32 = 0.6;

/// Distance from a river's axis where its banks end.
fn reach(width: f32) -> f32 {
    width / 2. + MAX_BANK.max(width * BANK_PER_WIDTH)
}

/// sRGB component to linear.
fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// Bicubic interpolation (Catmull-Rom) through four values, at `t` between the middle two.
fn catmull_rom([p0, p1, p2, p3]: [f32; 4], t: f32) -> f32 {
    0.5 * (2. * p1
        + (p2 - p0) * t
        + (2. * p0 - 5. * p1 + 4. * p2 - p3) * t * t
        + (3. * (p1 - p2) + p3 - p0) * t * t * t)
}

//! Sampling the planet's surface between its cells: each cube face as a 2D height grid,
//! padded with the neighbouring faces' cells (found by direction), interpolated bicubically
//! in the face's cell coordinates. The game meshes the planet face by face through it.

use std::collections::HashMap;

use bevy_math::{IVec3, Vec2, Vec3};

use crate::{
    WorldgenSettings,
    detail::gullies,
    grid::Grid,
    heightmap::catmull_rom,
    noise::{Noise, smoothstep},
    planet::Planet,
    planet_climate::{PlanetClimate, WindField},
    planet_rivers::PlanetRiver,
    planet_springs::{PlanetSpring, Springs, find_springs},
    sphere::{FACES, SphereGrid},
};

/// Cells of padding around each face: the bicubic stencil reaches two cells out, and fine
/// detail near a face's edge is also evaluated on the neighbouring face, up to
/// [`EDGE_BLEND`] cells past its edge.
const PAD: usize = 14;
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

/// The planet's surface for rendering: heights (with fine detail: roughness and gullies, as
/// on the flat map's `detail.rs`) and lake depths between cells.
#[derive(Clone)]
pub struct PlanetSurface {
    pub n: usize,
    /// Radius of the sea level (world units).
    pub radius: f32,
    pub heights: FaceField,
    pub lake_depth: FaceField,
    pub ruggedness: FaceField,
    pub calm: FaceField,
    /// Ground colour of each cell (linear RGB), from its climate (Köppen class at its altitude);
    /// `None` without a climate.
    pub ground: Option<[FaceField; 3]>,
    /// Ground material weights of each cell from its climate (see `Koppen::ground_weights`).
    pub weights: Option<[FaceField; 5]>,
    /// The rock of each cell (`geology::Rock` id; see [`PlanetSurface::rock_at`]), fresh lava
    /// (0..1, round active volcanoes) and the volcanoes.
    pub rock: FaceField,
    pub lava: FaceField,
    pub volcanoes: Vec<crate::volcanoes::Volcano>,
    /// Coral reef (0..1, see `reefs`).
    pub reef: FaceField,
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
    /// Gully wavelength (cells), octaves and depth (world units), as on the flat map.
    detail_wavelength: f32,
    detail_octaves: u32,
    detail_amplitude: f32,
}

impl PlanetSurface {
    pub fn new(planet: &Planet, climate: Option<&PlanetClimate>) -> Self {
        let flat = WorldgenSettings::default();
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
        let weights = climate.map(|climate| {
            crate::par::map(planet.grid.len(), |i| {
                climate.koppen_at(planet.grid.direction(i), planet.heights[i]).ground_weights()
            })
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
        let weights = weights.map(|weights: Vec<[f32; 5]>| {
            [0, 1, 2, 3, 4].map(|k| {
                FaceField::new(planet, &weights.iter().map(|w| w[k]).collect::<Vec<_>>())
            })
        });
        let mut surface = Self {
            wind: climate.map(|climate| std::sync::Arc::new(climate.wind_field())),
            weights,
            ground,
            ocean: FaceField::new(planet, &ocean),
            rock: FaceField::new(planet, &planet.rock.iter().map(|&r| r as f32).collect::<Vec<_>>()),
            lava: FaceField::new(planet, &planet.lava),
            reef: FaceField::new(planet, &planet.reef),
            volcanoes: planet.volcanoes.clone(),
            marsh: FaceField::new(planet, &marsh),
            rivers: Vec::new(),
            channels: Channels::default(),
            springs: Springs::default(),
            n: planet.grid.n(),
            radius: planet.radius(),
            heights: FaceField::new(planet, &planet.heights),
            lake_depth: FaceField::new(planet, &planet.lake_depth),
            ruggedness: FaceField::new(planet, &planet.ruggedness),
            calm: FaceField::new(planet, &planet.calm),
            grid: std::sync::Arc::new(SphereGrid::new(planet.grid.n())),
            seed: planet.settings.seed as u32 ^ 0xD37A,
            detail_wavelength: flat.detail_wavelength,
            detail_octaves: flat.detail_octaves,
            detail_amplitude: flat.detail_amplitude,
        };
        // River water levels: never above the ground across the channel (smoothing and
        // meanders move points over lower ground), never rising downstream.
        let mut rivers = planet.rivers.clone();
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
        // they drained into), as on the flat map (`end_rivers_at_coast`).
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
        let strength = self.ruggedness.bilinear(face, p)
            * (1. - self.calm.bilinear(face, p).clamp(0., 1.));
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
        height + (gully + roughness) * self.detail_amplitude * strength
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
        gullies(p, gradient, self.seed, self.detail_wavelength, self.detail_octaves)
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

    /// The ocean in a direction (see [`PlanetSurface::ocean`]): above 0.5 at sea.
    pub fn ocean_at(&self, direction: Vec3) -> f32 {
        let (face, p) = self.face_coords(direction);
        self.ocean.bilinear(face, p)
    }

    /// The rock at continuous cell coordinates of a face (the nearest cell's).
    pub fn rock_at(&self, face: usize, p: Vec2) -> crate::geology::Rock {
        crate::geology::Rock::from_id(self.rock.bilinear(face, p.round()).round() as u8)
    }

    /// The river springs (see `planet_springs`).
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
/// bank's mean slope under [`MAX_BANK_SLOPE`], up to [`MAX_BANK`] (as on the flat map,
/// `map/rivers.rs`).
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

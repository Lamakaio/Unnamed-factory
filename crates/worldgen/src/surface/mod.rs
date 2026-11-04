//! The planet's surface for the game: its fields sampled between the cells ([`FaceField`]),
//! the heights with fine relief (`relief`) and the rivers' channels carved in (`channels`), the
//! feature fields from the climate (`features`: river banks, karst, deserts, ice). The game
//! meshes the planet face by face through it.

mod channels;
mod features;
mod field;
mod relief;

pub use field::FaceField;

use bevy_math::{Vec2, Vec3};

use channels::Channels;
use features::{deserts, karst, riparian, sea_ice, tidal_flats, tides};

use crate::{
    climate::{PlanetClimate, WindField},
    grid::Grid,
    noise::smoothstep,
    planet::Planet,
    rivers::PlanetRiver,
    sphere::SphereGrid,
    springs::{PlanetSpring, Springs, find_springs},
};

/// The planet's surface for rendering: heights (with fine detail: roughness and gullies, see
/// `detail`) and lake depths between cells.
#[derive(Clone)]
pub struct PlanetSurface {
    pub n: usize,
    /// Whether there is life on land (`World::life`): without, no vegetation.
    pub life: bool,
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
    pub craters: Vec<crate::craters::Crater>,
    /// Soil depth (`World::ground`: 1 Earth's).
    pub soil: f32,
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
    /// Tidal range on the coasts (m; 0 inland and out at sea).
    pub tide: FaceField,
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
            [0, 1, 2]
                .map(|k| FaceField::new(planet, &colours.iter().map(|c| c[k]).collect::<Vec<_>>()))
        });
        let koppen = climate.map(|climate| {
            crate::par::map(planet.grid.len(), |i| {
                climate.koppen_at(planet.grid.direction(i), planet.heights[i])
            })
        });
        let weights = koppen.as_ref().map(|koppen| {
            koppen
                .iter()
                .map(|k| k.ground_weights())
                .collect::<Vec<_>>()
        });
        let koppen_cells = koppen.clone();
        let koppen = koppen.map(|koppen| {
            FaceField::new(
                planet,
                &koppen.iter().map(|k| k.id() as f32).collect::<Vec<_>>(),
            )
        });
        // Marsh where wet ground meets a wet enough climate (not frozen), salt flats on closed
        // flats where it is dry.
        let marsh: Vec<f32> = (0..planet.grid.len())
            .map(|i| {
                let [dryness, _, _, snow, _] =
                    weights.as_ref().map_or([0.3, 0., 0., 0., 0.], |w| w[i]);
                let moist = 1. - smoothstep(0.5, 0.85, dryness);
                let marsh = planet.wetland[i] * moist * (1. - snow);
                marsh - planet.basins[i] * (1. - moist)
            })
            .collect();
        let ocean: Vec<f32> = planet
            .sea
            .iter()
            .map(|&s| if s { 1. } else { 0. })
            .collect();
        let ocean = planet.grid.blur(&ocean, 3);
        // Glaciers and ice caps: the ice on the ground (smooth: no gullies), no lakes or
        // marsh under it.
        let thickness = climate.map_or_else(
            || vec![0.; planet.grid.len()],
            |climate| crate::glaciers::glaciers(planet, climate),
        );
        let ice: Vec<f32> = thickness
            .iter()
            .map(|&t| smoothstep(0.05, 0.25, t))
            .collect();
        let heights: Vec<f32> = planet
            .heights
            .iter()
            .zip(&thickness)
            .map(|(h, t)| h + t)
            .collect();
        let lake_depth: Vec<f32> = planet
            .lake_depth
            .iter()
            .zip(&ice)
            .map(|(&d, &ice)| if ice > 0. { 0. } else { d })
            .collect();
        let calm: Vec<f32> = planet
            .calm
            .iter()
            .zip(&ice)
            .map(|(c, i)| c.max(*i))
            .collect();
        let (erg, badlands) = match &koppen_cells {
            Some(koppen) => deserts(planet, koppen),
            None => (vec![0.; planet.grid.len()], vec![0.; planet.grid.len()]),
        };
        let sea_ice = climate.map_or_else(
            || vec![0.; planet.grid.len()],
            |climate| sea_ice(planet, climate),
        );
        let karst_cells = match &koppen_cells {
            Some(koppen) => karst(planet, koppen),
            None => vec![0.; planet.grid.len()],
        };
        // Tidal flats join the marshes (salt marsh, or mud without life).
        let tide = tides(planet);
        let flats = tidal_flats(planet, &tide);
        let marsh: Vec<f32> = marsh
            .iter()
            .zip(&flats)
            .zip(&ice)
            .map(|((&m, &f), i)| if f > m.abs() { f } else { m } * (1. - i))
            .collect();
        let weights = weights.map(|weights: Vec<[f32; 5]>| {
            [0, 1, 2, 3, 4]
                .map(|k| FaceField::new(planet, &weights.iter().map(|w| w[k]).collect::<Vec<_>>()))
        });
        let mut surface = Self {
            wind: climate.map(|climate| std::sync::Arc::new(climate.wind_field())),
            weights,
            koppen,
            ground,
            ocean: FaceField::new(planet, &ocean),
            rock: FaceField::new(
                planet,
                &planet.rock.iter().map(|&r| r as f32).collect::<Vec<_>>(),
            ),
            lava: FaceField::new(planet, &planet.lava),
            geothermal: FaceField::new(planet, &planet.geothermal),
            reef: FaceField::new(planet, &planet.reef),
            ice: FaceField::new(planet, &ice),
            sea_ice: FaceField::new(planet, &sea_ice),
            erg: FaceField::new(planet, &erg),
            badlands: FaceField::new(planet, &badlands),
            karst: FaceField::new(planet, &karst_cells),
            volcanoes: planet.volcanoes.clone(),
            craters: planet.craters.clone(),
            soil: planet.settings.world.ground.soil,
            marsh: FaceField::new(planet, &marsh),
            tide: FaceField::new(planet, &tide),
            rivers: Vec::new(),
            channels: Channels::default(),
            springs: Springs::default(),
            n: planet.grid.n(),
            life: planet.settings.world.life,
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
            let under = river
                .points
                .iter()
                .take_while(|p| surface.ice_at(p.direction) > 0.5)
                .count();
            let keep = river.points.len().saturating_sub(1);
            river.points.drain(..under.min(keep));
        }
        for river in &mut rivers {
            let mut previous = f32::MAX;
            let count = river.points.len();
            for k in 0..count {
                let point = river.points[k];
                let (before, after) = (
                    river.points[k.saturating_sub(1)],
                    river.points[(k + 1).min(count - 1)],
                );
                let up = point.direction;
                let across = (after.direction - before.direction)
                    .cross(up)
                    .normalize_or_zero();
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
            let reached = river.points.iter().position(|p| {
                surface.uncarved_height(p.direction) < -SEA_REACHED
                    && surface.ocean_at(p.direction) > 0.5
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
        lake.max(self.channels.wetness(world))
            .max(self.springs.carve(world, 0.).1)
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
        self.springs
            .carve(world, self.channels.carve(world, height))
            .0
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
        let tx = (self.direction(face, p + Vec2::X) - self.direction(face, p - Vec2::X))
            * self.radius
            / 2.;
        let ty = (self.direction(face, p + Vec2::Y) - self.direction(face, p - Vec2::Y))
            * self.radius
            / 2.;
        let dx = (self.heights.bilinear(face, p + Vec2::X)
            - self.heights.bilinear(face, p - Vec2::X))
            / 2.;
        let dy = (self.heights.bilinear(face, p + Vec2::Y)
            - self.heights.bilinear(face, p - Vec2::Y))
            / 2.;
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

/// Local wind: the ring (cells) the exposure compares a point's height with, the most the
/// wind speeds up or slows down (share) and the height difference (world units) for most of
/// it, and the slopes over which the wind turns along them instead of blowing up.
const WIND_RING: f32 = 6.;
const WIND_EXPOSURE: (f32, f32) = (0.5, 4.);
const WIND_STEEP: (f32, f32) = (0.3, 1.);

/// sRGB component to linear.
fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

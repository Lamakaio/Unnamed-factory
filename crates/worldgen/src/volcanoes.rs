//! Volcanoes: where plates make them, built on the eroded planet as young constructions.
//!
//! - **Arcs**: above subduction zones. Island arcs (an ocean plate overriding another) and
//!   continental arcs (young ranges along the coast, as the Andes): steep stratovolcanoes with
//!   a summit crater, spaced along the arc.
//! - **Hotspots**: mantle plumes fixed under the moving plates. The plate carries each volcano
//!   away as it grows a new one over the plume, so they come in chains, the youngest over the
//!   plume and older ones further along the plate's motion: broad shield volcanoes with a
//!   caldera, lower and more eroded the older (at sea, the old ones sink: seamounts).
//!
//! Each also gets a kind and a random design (see `VolcanoKind`, `Design`): stretch along a
//! rift axis, flank cones, maars, a breached side (landslides), radial gullies deepening with
//! age, a summit crater or caldera. Volcanoes are basalt; active ones have lava only in their
//! craters and calderas (lava lakes).

use bevy_math::{Vec2, Vec3};

use crate::{
    grid::Grid,
    noise::{Noise, hash, smoothstep},
    sphere::SphereGrid,
};

/// The kind of a volcano.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VolcanoKind {
    /// Steep cone with a summit crater, flank cones, often a breached (landslide) side, radial
    /// gullies (arcs: the Andes, Indonesia, Japan).
    Strato,
    /// Broad, low dome stretched along its rift zones, with a summit caldera and pit craters,
    /// landslide scarps on old ones (hotspots: Hawaii).
    Shield,
    /// A big compound volcano of several overlapping cones, the main one with a caldera
    /// (Kilimanjaro, Etna).
    Massif,
    /// A low, wide edifice collapsed into a large caldera (a lake once the lakes form),
    /// sometimes with a new cone inside (Crater Lake, Toba, Santorini).
    Caldera,
    /// A field of small cinder cones, domes and maars on a low plateau (the Chaîne des Puys in
    /// Auvergne, Michoacán).
    ConeField,
}

/// A volcano.
#[derive(Clone, Copy, Debug)]
pub struct Volcano {
    pub kind: VolcanoKind,
    /// Summit (unit direction).
    pub direction: Vec3,
    /// Height of the construction above the ground under it, and its radius (world units).
    pub height: f32,
    pub radius: f32,
    /// 0 (active) .. 1 (old, eroded, sunk).
    pub age: f32,
}

impl Volcano {
    pub fn active(&self) -> bool {
        self.age < ACTIVE_AGE
    }
}

/// Volcanoes younger than this are active (lava fields).
const ACTIVE_AGE: f32 = 0.15;
/// Volcanoes younger than this (age) have geothermal fields, on ground sloping less than
/// `GEOTHERMAL_SLOPE` (world units per cell, fading between the two).
const GEOTHERMAL_AGE: f32 = 0.3;
const GEOTHERMAL_SLOPE: (f32, f32) = (0.12, 0.3);
/// Arc volcanoes: spacing along the arc (cells), height (world units) and radius per height.
const ARC_SPACING: f32 = 120.;
const STRATO_HEIGHT: (f32, f32) = (7., 15.);
const STRATO_RADIUS: f32 = 1.9;
/// Hotspots: plumes per million cells, volcanoes per chain, spacing along it (cells), height
/// and radius per height.
const HOTSPOTS_PER_MILLION: f32 = 1.6;
const CHAIN: (usize, usize) = (4, 8);
const CHAIN_SPACING: (f32, f32) = (28., 45.);
const SHIELD_HEIGHT: (f32, f32) = (8., 13.);
const SHIELD_RADIUS: f32 = 3.8;
/// Cone fields: the plateau's height and the cinder cones' radius (world units).
const CONE_FIELD_PLATEAU: f32 = 1.2;
const CINDER_CONE: (f32, f32) = (2.5, 5.);
/// Lava lakes in active craters: at most this radius (world units).
const LAVA_LAKE: f32 = 2.5;
/// Maars: crater depth (world units).
const MAAR_DEPTH: f32 = 0.8;
/// How deep old volcanoes sink at sea (world units, at age 1).
const SUBSIDENCE: f32 = 9.;
/// Share of the hotspots under the oceans.
const OCEANIC_HOTSPOTS: f32 = 0.8;

/// Where volcanoes are. `arc(i)` is how much a cell is on an island arc (0..1), `coastal_belt`
/// a young range near the coast; `velocity(direction)` the surface velocity of the plate there
/// (its direction only matters).
pub fn place(
    grid: &SphereGrid,
    radius: f32,
    seed: u64,
    arc: impl Fn(usize) -> f32,
    coastal_belt: impl Fn(usize) -> f32,
    velocity: impl Fn(Vec3) -> Vec3,
    sea: impl Fn(usize) -> bool,
) -> Vec<Volcano> {
    let seed = seed as u32 ^ 0x701C;
    let random = |a: u32, b: u32| hash(seed, a, b) as f32 / u32::MAX as f32;
    let mut volcanoes: Vec<Volcano> = Vec::new();
    let far_from_all = |volcanoes: &[Volcano], d: Vec3, spacing: f32| {
        volcanoes
            .iter()
            .all(|v| v.direction.distance(d) * radius > spacing)
    };
    // Arcs: candidate cells in a random order, kept when far enough from the others.
    let mut candidates: Vec<(u32, usize)> = (0..grid.len())
        .filter_map(|i| {
            let weight = arc(i).max(coastal_belt(i));
            (weight > 0.5 && random(i as u32, 1) < 0.003 * weight)
                .then(|| (hash(seed, i as u32, 2), i))
        })
        .collect();
    candidates.sort_unstable();
    for (_, i) in candidates {
        let d = grid.direction(i);
        let spacing = ARC_SPACING * (0.7 + 0.6 * random(i as u32, 3));
        if !far_from_all(&volcanoes, d, spacing) {
            continue;
        }
        let height = STRATO_HEIGHT.0 + (STRATO_HEIGHT.1 - STRATO_HEIGHT.0) * random(i as u32, 4);
        volcanoes.push(Volcano {
            kind: VolcanoKind::Strato,
            direction: d,
            height,
            radius: height * STRATO_RADIUS,
            age: random(i as u32, 5).powf(1.5),
        });
    }
    // Hotspots: chains along the plate's motion, oldest furthest downstream.
    let count = (HOTSPOTS_PER_MILLION * grid.len() as f32 / 1e6).round() as u32;
    for k in 0..count {
        // Mostly under the oceans (as on Earth), now and then under a continent.
        let oceanic = random(k, 13) < OCEANIC_HOTSPOTS;
        let i = (0..32)
            .map(|t| (hash(seed, k, 10 + 1000 * t) as usize) % grid.len())
            .find(|&i| sea(i) == oceanic)
            .unwrap_or((hash(seed, k, 10) as usize) % grid.len());
        let plume = grid.direction(i);
        let motion = velocity(plume).reject_from(plume).normalize_or_zero();
        if motion == Vec3::ZERO {
            continue;
        }
        let length = CHAIN.0 + (hash(seed, k, 11) as usize) % (CHAIN.1 - CHAIN.0 + 1);
        let spacing = CHAIN_SPACING.0 + (CHAIN_SPACING.1 - CHAIN_SPACING.0) * random(k, 12);
        let mut here = plume;
        for step in 0..length {
            // (From new over the plume to old and sunk at the far end.)
            let age = step as f32 / (length - 1) as f32;
            let height = (SHIELD_HEIGHT.0
                + (SHIELD_HEIGHT.1 - SHIELD_HEIGHT.0) * random(k, 20 + step as u32))
                * (1. - 0.35 * age);
            if far_from_all(&volcanoes, here, spacing * 0.6) {
                volcanoes.push(Volcano {
                    kind: VolcanoKind::Shield,
                    direction: here,
                    height,
                    radius: height * SHIELD_RADIUS,
                    age,
                });
            }
            // Downstream, with a little wander.
            let side = here.cross(motion).normalize_or_zero();
            let wander = (random(k, 40 + step as u32) - 0.5) * 0.5;
            here = (here + (motion + side * wander) * spacing / radius).normalize();
        }
    }
    // Varied kinds: most arc volcanoes are stratovolcanoes, some compound massifs, calderas
    // and cone fields; hotspots under continents make calderas and cone fields more than
    // shields.
    for (k, v) in volcanoes.iter_mut().enumerate() {
        let roll = random(k as u32, 90);
        let on_land = !sea(grid.cell_at(v.direction));
        let (kind, height, radius) = match v.kind {
            VolcanoKind::Strato if roll < 0.62 => (VolcanoKind::Strato, v.height, v.radius),
            VolcanoKind::Strato if roll < 0.77 => {
                (VolcanoKind::Massif, v.height * 1.1, v.radius * 1.3)
            }
            VolcanoKind::Strato if roll < 0.84 => {
                (VolcanoKind::Caldera, v.height * 0.55, v.radius * 1.1)
            }
            VolcanoKind::Strato if on_land => {
                (VolcanoKind::ConeField, CONE_FIELD_PLATEAU, v.radius * 0.8)
            }
            VolcanoKind::Strato => (VolcanoKind::Strato, v.height, v.radius),
            _ if on_land && roll < 0.15 => (VolcanoKind::Caldera, v.height * 0.5, v.radius * 0.6),
            _ if on_land && roll < 0.6 => {
                (VolcanoKind::ConeField, CONE_FIELD_PLATEAU, v.radius * 0.5)
            }
            _ => (v.kind, v.height, v.radius),
        };
        v.kind = kind;
        v.height = height;
        v.radius = radius;
    }
    volcanoes
}

/// What the volcanoes leave besides their shape: volcanic ground (basalt), fresh lava (0..1)
/// and the ruggedness of their slopes (gullies on the older ones).
pub struct Built {
    pub volcanic: Vec<bool>,
    pub lava: Vec<f32>,
    pub ruggedness: Vec<f32>,
    /// Geothermal fields (hot springs, fumaroles: 0..1) in patches on the flanks of young
    /// volcanoes.
    pub geothermal: Vec<f32>,
}

/// Build the volcanoes on the heights (world units; sea level 0). On land a volcano sits on
/// the ground; at sea it rises from the sea floor to its summit above the water (old ones
/// sink: islands worn low, then seamounts). Craters, calderas and maars dig into the ground.
/// Cells raised above the sea become land.
pub fn build(
    grid: &SphereGrid,
    radius: f32,
    seed: u64,
    volcanoes: &[Volcano],
    heights: &mut [f32],
    sea: &mut [bool],
) -> Built {
    let cells = grid.len();
    let mut built = Built {
        volcanic: vec![false; cells],
        lava: vec![0.; cells],
        ruggedness: vec![0.; cells],
        geothermal: vec![0.; cells],
    };
    let noise = Noise::new(seed as u32 ^ 0x1A7A);
    let mut seen = vec![0u32; cells];
    for (k, v) in volcanoes.iter().enumerate() {
        let start = grid.cell_at(v.direction);
        let design = Design::new(
            v,
            seed as u32 ^ 0x5EED ^ (k as u32).wrapping_mul(0x9E37_79B9),
        );
        // (Cells seen for this volcano: stamped with its number.)
        let stamp = k as u32 + 1;
        seen[start] = stamp;
        let mut queue = std::collections::VecDeque::from([start]);
        let reach = v.radius * design.reach;
        let at_sea = sea[start];
        // Old volcanoes are lower; at sea, the summit's height above the water (sinking with
        // age).
        let height = v.height * (1. - 0.4 * v.age);
        let summit = height - SUBSIDENCE * v.age;
        // The local frame at the summit (world units east and north of it).
        let east = Vec3::Y
            .cross(v.direction)
            .try_normalize()
            .unwrap_or(Vec3::X);
        let north = v.direction.cross(east);
        let layer = noise.layer(k as u32);
        while let Some(i) = queue.pop_front() {
            let d = grid.direction(i);
            let r = d.distance(v.direction) * radius;
            if r > reach + 1.5 {
                continue;
            }
            for (j, _) in grid.neighbours(i) {
                if seen[j] != stamp {
                    seen[j] = stamp;
                    queue.push_back(j);
                }
            }
            if r > reach {
                continue;
            }
            let q = Vec2::new((d - v.direction).dot(east), (d - v.direction).dot(north)) * radius;
            let shape = design.shape(
                v,
                q,
                height,
                |p: Vec3, octaves| layer.fbm3(p, octaves),
                d * radius,
            );
            // (The digging by the volcano's own height: at sea, not by its rise from the floor.
            // At sea it rises from the floor under each cell, never below it: rising from the
            // summit cell's floor made a flat disc out to its reach, poking out of the water in
            // a perfect circle when that floor was shallow.)
            let built_height = if at_sea {
                let floor = heights[i];
                floor.max(floor + (summit - floor) * shape.rise) - height * shape.dig
            } else {
                heights[i] + height * (shape.rise - shape.dig)
            };
            if built_height > heights[i] || shape.dig > 0.01 {
                heights[i] = built_height;
                sea[i] = built_height <= 0.;
            }
            if shape.rise > 0.01 || shape.dig > 0.01 {
                built.volcanic[i] = true;
                built.ruggedness[i] = built.ruggedness[i].max(shape.rugged);
            }
            // Fresh lava only in the craters and calderas of active volcanoes (lava lakes).
            if v.active() {
                built.lava[i] = built.lava[i].max(shape.crater_floor);
            }
            // Geothermal fields: patches on the flanks of young volcanoes, on land (kept to
            // gentle ground below).
            let x = shape.x;
            if v.age < GEOTHERMAL_AGE && built_height > 0.2 {
                let ring = smoothstep(0.3, 0.45, x) * smoothstep(1.0, 0.8, x);
                let patch = smoothstep(
                    0.3,
                    0.55,
                    noise.layer(200 + k as u32).fbm3(d * radius / 7., 2),
                );
                built.geothermal[i] = built.geothermal[i].max(ring * patch);
            }
        }
    }
    // Hot springs on gentle ground only (terraces, valley floors), not on steep slopes.
    for i in 0..cells {
        if built.geothermal[i] > 0. {
            let slope = grid
                .neighbours(i)
                .map(|(j, d)| (heights[j] - heights[i]).abs() / d)
                .fold(0., f32::max);
            built.geothermal[i] *= smoothstep(GEOTHERMAL_SLOPE.1, GEOTHERMAL_SLOPE.0, slope);
        }
    }
    built
}

/// A volcano's shape at a point: its rise (share of its height, over the ground or the sea
/// floor), what its craters dig (share of its height, may go below the ground), how much a
/// crater or caldera floor it is (0..1), the ruggedness of its slopes, and the distance from
/// the summit (share of the radius, along its stretched shape).
struct Shape {
    rise: f32,
    dig: f32,
    crater_floor: f32,
    rugged: f32,
    x: f32,
}

/// A vent: a cone (or a maar, a crater without a cone) at an offset from the summit (world
/// units), its radius (world units), height (share of the volcano's height), crater (share of
/// its radius, 0 for a dome) and breached side (angle).
struct Vent {
    at: Vec2,
    radius: f32,
    height: f32,
    crater: f32,
    breach: Option<f32>,
    maar: bool,
}

/// A crater dug into the volcano: its centre (world units from the summit), radius (world
/// units), depth (share of the volcano's height), how far in its walls end (share of the
/// radius: low, gentle walls; high, sheer ones) and whether an active volcano has a lava lake
/// in it.
struct Pit {
    at: Vec2,
    radius: f32,
    depth: f32,
    floor: f32,
    lava: bool,
}

/// The random design of one volcano: stretch along its rift axis, a breached (landslide)
/// side (angle, half-width), how concave its cone is, its summit craters (or caldera), flank
/// vents.
struct Design {
    axis: Vec2,
    stretch: f32,
    breach: Option<(f32, f32)>,
    steepness: f32,
    pits: Vec<Pit>,
    vents: Vec<Vent>,
    /// How far the shape reaches (share of the radius).
    reach: f32,
}

/// The angle between two angles (radians, 0..π).
fn angle_between(a: f32, b: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    ((a - b + PI).rem_euclid(TAU) - PI).abs()
}

impl Design {
    fn new(v: &Volcano, seed: u32) -> Self {
        use std::f32::consts::TAU;
        let random = |k: u32| hash(seed, k, 7) as f32 / u32::MAX as f32;
        let axis = Vec2::from_angle(random(1) * TAU);
        let old = v.age;
        let mut vents = Vec::new();
        // A cone at a random angle, at a distance (share of the radius) and of a radius (world
        // units) in ranges.
        let cone = |k: u32, distance: (f32, f32), size: (f32, f32), height: f32| Vent {
            at: Vec2::from_angle(random(k) * TAU)
                * v.radius
                * (distance.0 + (distance.1 - distance.0) * random(k + 1)),
            radius: size.0 + (size.1 - size.0) * random(k + 2),
            height,
            crater: 0.25,
            breach: (random(k + 3) < 0.3).then(|| random(k + 4) * TAU),
            maar: false,
        };
        let (stretch, breach_chance, crater_radius, crater_depth) = match v.kind {
            VolcanoKind::Strato => {
                // Parasitic cones on the flanks.
                for j in 0..(random(10) * 4.) as u32 {
                    vents.push(cone(
                        20 + 10 * j,
                        (0.3, 0.7),
                        (0.07 * v.radius, 0.14 * v.radius),
                        0.12,
                    ));
                }
                // Old ones: the summit crater widened into a caldera.
                let caldera = old > 0.5 && random(5) < 0.6;
                let (radius, depth) = if caldera { (0.22, 0.14) } else { (0.08, 0.1) };
                (1. + 0.45 * random(2), 0.3 + 0.35 * old, radius, depth)
            }
            VolcanoKind::Shield => {
                // Cones and pit craters along the rift zones.
                for j in 0..(2. + random(10) * 5.) as u32 {
                    let along = (random(20 + 10 * j) - 0.5) * 1.4 * v.radius;
                    vents.push(Vent {
                        at: axis * along * (1.3 + 0.5 * random(2)),
                        radius: 0.05 * v.radius,
                        height: 0.06,
                        crater: 0.4,
                        breach: None,
                        maar: random(21 + 10 * j) < 0.3,
                    });
                }
                (1.3 + 0.6 * random(2), 0.25 + 0.4 * old, 0.14, 0.07)
            }
            VolcanoKind::Massif => {
                // Two or three big overlapping cones round the main one.
                for j in 0..(2. + random(10) * 1.5) as u32 {
                    let height = 0.55 + 0.25 * random(60 + j);
                    vents.push(cone(
                        20 + 10 * j,
                        (0.2, 0.4),
                        (0.35 * v.radius, 0.5 * v.radius),
                        height,
                    ));
                }
                (1.1 + 0.4 * random(2), 0.3, 0.16, 0.12)
            }
            VolcanoKind::Caldera => {
                // Sometimes a new cone inside the caldera.
                if random(11) < 0.5 {
                    vents.push(Vent {
                        at: Vec2::from_angle(random(12) * TAU) * v.radius * 0.12 * random(13),
                        radius: 0.12 * v.radius,
                        height: 0.55,
                        crater: 0.3,
                        breach: None,
                        maar: false,
                    });
                }
                (1.1 + 0.5 * random(2), 0.15, 0.35 + 0.12 * random(3), 0.5)
            }
            VolcanoKind::ConeField => {
                // Many small cinder cones, a few domes and maars, scattered over the field.
                let count = 8 + (random(10) * 14.) as u32;
                for j in 0..count {
                    let k = 20 + 10 * j;
                    let radius = CINDER_CONE.0 + (CINDER_CONE.1 - CINDER_CONE.0) * random(k + 2);
                    let roll = random(k + 3);
                    vents.push(Vent {
                        at: Vec2::from_angle(random(k) * TAU)
                            * v.radius
                            * random(k + 1).sqrt()
                            * 0.85,
                        radius,
                        // (Heights relative to the field's plateau.)
                        height: radius * (0.35 + 0.15 * random(k + 5)) / CONE_FIELD_PLATEAU,
                        crater: if roll < 0.15 { 0. } else { 0.3 },
                        breach: (roll > 0.65).then(|| random(k + 4) * TAU),
                        maar: roll > 0.88,
                    });
                }
                (1.2 + 0.5 * random(2), 0., 0., 0.)
            }
        };
        let breach = (random(3) < breach_chance).then(|| (random(4) * TAU, 0.45 + 0.4 * random(6)));
        // The summit crater (or caldera), a little off centre, and on most volcanoes a few
        // smaller ones of various sizes round it; walls sheer or gentle.
        let mut pits = Vec::new();
        if crater_radius > 0. {
            pits.push(Pit {
                at: Vec2::from_angle(random(70) * TAU) * v.radius * 0.05 * random(71),
                radius: crater_radius * v.radius * (0.8 + 0.4 * random(72)),
                depth: crater_depth,
                // (Calderas' walls gentler: slumped and eroded.)
                floor: if v.kind == VolcanoKind::Caldera {
                    0.25 + 0.3 * random(73)
                } else {
                    0.45 + 0.4 * random(73)
                },
                lava: v.kind != VolcanoKind::Caldera,
            });
            let extra = if v.kind == VolcanoKind::Caldera {
                1.5
            } else {
                4.
            };
            for j in 0..(random(74) * extra) as u32 {
                let k = 80 + 6 * j;
                pits.push(Pit {
                    at: Vec2::from_angle(random(k) * TAU) * v.radius * (0.05 + 0.2 * random(k + 1)),
                    radius: v.radius * (0.02 + 0.06 * random(k + 2)),
                    depth: crater_depth * (0.4 + 0.6 * random(k + 3)),
                    floor: 0.3 + 0.5 * random(k + 4),
                    lava: false,
                });
            }
        }
        let steepness = 1.2 + 1.1 * random(75);
        let beyond = vents
            .iter()
            .map(|w| (w.at.length() + w.radius) / v.radius - 1.)
            .fold(0., f32::max);
        Design {
            axis,
            stretch,
            breach,
            steepness,
            pits,
            vents,
            reach: 1.4 + beyond.max(0.),
        }
    }

    /// The shape at `q` (world units east and north of the summit); `height` is the volcano's
    /// (world units), `noise` a 3D noise, `world` the point (world units).
    fn shape(
        &self,
        v: &Volcano,
        q: Vec2,
        height: f32,
        noise: impl Fn(Vec3, u32) -> f32,
        world: Vec3,
    ) -> Shape {
        let young = 1. - v.age;
        // Stretched along the rift axis, the outline in irregular lobes.
        let local = Vec2::new(q.dot(self.axis) / self.stretch, q.perp_dot(self.axis));
        let wobble = 1.
            + 0.32 * noise(world / (v.radius * 0.9), 2)
            + 0.1 * noise(world / (v.radius * 0.3), 2);
        let x = local.length() / (v.radius * wobble);
        let angle = local.y.atan2(local.x);
        let profile = |x: f32| match v.kind {
            // Concave: steep near the summit, flaring out; blunter with age.
            VolcanoKind::Strato | VolcanoKind::Massif => (1. - x)
                .max(0.)
                .powf(self.steepness * (0.75 + 0.25 * young))
                .min(1. - 0.25 * v.age),
            VolcanoKind::Shield => (1. - x * x).max(0.).powf(1.3),
            VolcanoKind::Caldera => (1. - x).max(0.).powf(1.2),
            VolcanoKind::ConeField => (1. - x * x).max(0.),
        };
        let mut rise = profile(x);
        // Radial gullies and ridges (barrancos), deeper with age; young cones jagged with small
        // ribs. (Noise in polar coordinates: stretched down the slope.)
        if v.kind != VolcanoKind::ConeField {
            let polar = Vec3::new(angle.cos() * 2.6, angle.sin() * 2.6, x * 1.2);
            let ribs = 1. - noise(polar * (1. + young), 2).abs();
            let gully_depth =
                (0.05 + 0.25 * v.age) * smoothstep(0.08, 0.3, x) * smoothstep(1., 0.7, x);
            rise -= gully_depth * ribs.powi(3) * rise.max(0.15).sqrt();
            rise += 0.03 * young * noise(world / 2.5, 2) * smoothstep(0.9, 0.2, x);
        }
        // Landslides: a horseshoe amphitheatre open down one side from the summit (a scarp on
        // shields), its floor sloping gently out.
        if let Some((direction, width)) = self.breach {
            let side = smoothstep(width, width * 0.3, angle_between(angle, direction));
            let shield = v.kind == VolcanoKind::Shield;
            let (from, to) = if shield { (0.35, 1.) } else { (0., 0.6) };
            let inside = smoothstep(to + 0.1, to - 0.2, x) * smoothstep(from - 0.1, from + 0.1, x);
            let floor = profile(to) + (to - x).max(0.) * 0.25;
            let depth = if shield { 0.6 } else { 0.85 };
            rise -= (rise - floor).max(0.) * side * inside * depth;
        }
        // The craters (or caldera): flat floors in walls sheer or gentle, shallower with age,
        // stretched with the volcano and a little irregular.
        let mut dig = 0f32;
        let mut crater_floor = 0f32;
        for pit in &self.pits {
            let centre = Vec2::new(
                pit.at.dot(self.axis) / self.stretch,
                pit.at.perp_dot(self.axis),
            );
            let r = (local - centre).length() / pit.radius;
            if r > 1.3 {
                continue;
            }
            let r = r * (1. + 0.15 * noise(world / (pit.radius * 0.7) + Vec3::splat(5.), 2));
            dig = dig.max(pit.depth * (0.5 + 0.5 * young) * smoothstep(1., pit.floor, r));
            // (A small lava lake in the middle of the floor; none in collapsed calderas, which
            // hold water.)
            if pit.lava {
                let lake = (pit.floor * 0.7).min(LAVA_LAKE / pit.radius);
                crater_floor = crater_floor.max(smoothstep(lake, lake * 0.6, r));
            }
        }
        // Flank cones, domes and maars.
        for vent in &self.vents {
            let r = (q - vent.at).length() / vent.radius;
            if r > 1.2 {
                continue;
            }
            if vent.maar {
                // A round crater dug into the ground, a low ring round it.
                dig += MAAR_DEPTH / height.max(1.) * smoothstep(0.75, 0.45, r);
                rise += MAAR_DEPTH * 0.3 / height.max(1.)
                    * smoothstep(1.1, 0.8, r)
                    * smoothstep(0.5, 0.8, r);
                continue;
            }
            let base = profile((vent.at.length() / (v.radius * wobble)).min(1.));
            let mut cone = (1. - r).max(0.).powf(1.2) * vent.height;
            if vent.crater > 0. {
                let mut crater = smoothstep(vent.crater, vent.crater * 0.6, r) * vent.height * 0.35;
                // Breached: the crater open down one side.
                if let Some(b) = vent.breach {
                    let open = smoothstep(1.0, 0.4, angle_between((q - vent.at).to_angle(), b));
                    cone *= 1. - 0.6 * open * smoothstep(0.9, 0.3, r);
                    crater *= 1. - open;
                }
                cone -= crater;
            }
            rise = rise.max(base + cone);
        }
        let rugged = match v.kind {
            VolcanoKind::Shield => 0.1 + 0.6 * v.age,
            VolcanoKind::ConeField => 0.1,
            _ => 0.3 + 0.5 * v.age,
        } * smoothstep(1.1, 0.7, x);
        Shape {
            rise,
            dig,
            crater_floor,
            rugged,
            x,
        }
    }
}

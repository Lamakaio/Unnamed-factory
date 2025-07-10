use bevy::{
    log::{info, warn},
    math::{NormedVectorSpace, Vec2, Vec3, cubic_splines::CubicHermite},
};
use fast_hilbert;
use kdtree_collisions::{KdTree, KdValue};
use noiz::{
    Noise, SampleableFor,
    cell_noise::MixCellValuesForDomain,
    cells::{OrthoGrid, SimplexGrid, WithGradient},
    curves::Smoothstep,
    math_noise::{Pow2, PowF},
    misc_noise::{Constant, WithGradientOf},
    prelude::{
        BlendCellGradients, EuclideanLength, FractalLayers, LayeredNoise, Masked, MixCellGradients,
        Normed, NormedByDerivative, Octave, Offset, PeakDerivativeContribution, Persistence,
        QuickGradients, SNormToUNorm, Scaled, SimplecticBlend,
    },
    rng::{NoiseRng, SNorm},
};
use rand::SeedableRng;
use rand_distr::Distribution;
use std::{
    collections::{BTreeMap, BTreeSet},
    f32::consts::PI,
    ops::{Index, IndexMut},
};

use crate::map::{Chunk, GRID_SQUARE_SIZE};

type NoiseT = Noise<(
    LayeredNoise<
        Normed<WithGradient<f32, Vec2>>,
        Persistence,
        (
            Octave<(OceanNoiseT, Scaled<f32>)>,
            Octave<Masked<ContinentNoiseT, FlatnessNoiseT>>,
        ),
    >,
    SNormToUNorm,
)>;

type OceanNoiseT = (
    Scaled<f32>,
    noiz::prelude::Offset<MixCellValuesForDomain<OrthoGrid, Smoothstep, SNorm>>,
    BlendCellGradients<SimplexGrid, SimplecticBlend, QuickGradients, true>,
    SNormToUNorm,
    PowF,
);

type ContinentNoiseT = (
    LayeredNoise<
        NormedByDerivative<WithGradient<f32, Vec2>, EuclideanLength, PeakDerivativeContribution>,
        Persistence,
        FractalLayers<Octave<MixCellGradients<OrthoGrid, Smoothstep, QuickGradients, true>>>,
    >,
    SNormToUNorm,
);

type FlatnessNoiseT = (
    noiz::prelude::Offset<MixCellValuesForDomain<OrthoGrid, Smoothstep, SNorm>>,
    Masked<
        (
            Scaled<f32>,
            BlendCellGradients<SimplexGrid, SimplecticBlend, QuickGradients, true>,
            SNormToUNorm,
        ),
        (
            Scaled<f32>,
            BlendCellGradients<SimplexGrid, SimplecticBlend, QuickGradients, true>,
            SNormToUNorm,
            Pow2,
        ),
    >,
    Pow2,
    Offset<(Constant<f32>, WithGradientOf<Vec2>)>,
    Scaled<f32>,
);
pub struct TerrainPoint {
    pub height: f32,
    pub wetness: f32,
    pub grad: Vec2,
}
#[derive(Clone, Default, Debug)]
pub struct Hydrologypoint {
    pub momentum: Vec2,
    pub amount: f32,
    dead_end: bool,
    pub visit: u8,
    pub source: usize,
    ctrlpoint: bool,
    next: usize,
    prev: usize,
}

pub struct Continent {
    points: Vec<TerrainPoint>,
    hydrology: Vec<Hydrologypoint>,
    height_noise: NoiseT,
    offset: Vec2,
    pub river_paths: Vec<CubicHermite<Vec3>>,
    pub lakes: Vec<usize>,
    pub to_sea: BTreeMap<usize, usize>,
    pub to_lake: BTreeMap<usize, usize>,
}

impl Continent {
    pub const CONTINENT_SIZE_PO2: u8 = 11;
    pub const CONTINENT_SIZE: u32 = 1 << Self::CONTINENT_SIZE_PO2;
    pub const OCEAN_HEIGHT_LIMIT: f32 = 0.534;

    pub fn new_and_generate(seed: u32) -> Self {
        let mut new = Self {
            points: Vec::with_capacity(1 << (2 * Self::CONTINENT_SIZE_PO2)),
            hydrology: vec![
                Hydrologypoint {
                    amount: 1.,
                    ..Default::default()
                };
                1 << (2 * Self::CONTINENT_SIZE_PO2)
            ],
            height_noise: Self::get_noise(seed),
            offset: Vec2::new(0., 0.),
            river_paths: Vec::default(),
            lakes: Vec::default(),
            to_sea: BTreeMap::default(),
            to_lake: BTreeMap::default(),
        };
        new.generate();
        new
    }

    fn get_noise(seed: u32) -> NoiseT {
        Noise {
            noise: (
                LayeredNoise::new(
                    Normed::default(),
                    Persistence(1.),
                    (
                        Octave((
                            (
                                Scaled(0.1),
                                noiz::prelude::Offset {
                                    offset_strength: 0.4,
                                    ..Default::default()
                                },
                                BlendCellGradients::default(),
                                SNormToUNorm::default(),
                                PowF(0.4),
                            ),
                            Scaled(0.2),
                        )),
                        Octave(Masked(
                            (
                                LayeredNoise::new(
                                    NormedByDerivative::default().with_falloff(0.35),
                                    Persistence(0.6),
                                    FractalLayers {
                                        layer: Octave::default(),
                                        lacunarity: 1.8,
                                        amount: 8,
                                    },
                                ),
                                SNormToUNorm::default(),
                                //WithGradientOf(Vec2::ZERO)
                            ),
                            (
                                noiz::prelude::Offset {
                                    offset_strength: 0.2,
                                    ..Default::default()
                                },
                                Masked(
                                    (
                                        Scaled(0.1),
                                        BlendCellGradients::default(),
                                        SNormToUNorm::default(),
                                        //WithGradientOf(Vec2::ZERO)
                                    ),
                                    (
                                        Scaled(0.2),
                                        BlendCellGradients::default(),
                                        SNormToUNorm::default(),
                                        Pow2::default(),
                                    ),
                                ),
                                Pow2::default(),
                                Offset {
                                    offseter: (Constant(0.1), WithGradientOf(Vec2::ZERO)),
                                    offset_strength: 1.,
                                },
                                Scaled(1.5),
                            ),
                        )),
                    ),
                ),
                SNormToUNorm::default(),
            ),
            seed: NoiseRng(seed),
            frequency: 0.04,
        }
    }

    fn generate(&mut self) {
        for i in 0..(1 << (Self::CONTINENT_SIZE_PO2 * 2)) {
            let pos: (u32, u32) = fast_hilbert::h2xy(i, Self::CONTINENT_SIZE_PO2);
            let offset = (1 << (Self::CONTINENT_SIZE_PO2 - 1)) as f32;
            let edge_mult = 1.
                - ((Vec2::new(pos.0 as f32, pos.1 as f32) - offset).abs() / offset)
                    .powf(8.)
                    .norm();
            let pos = self.offset + Vec2::new(pos.0 as f32, pos.1 as f32) * GRID_SQUARE_SIZE;
            let sample: WithGradient<f32, Vec2> = self.height_noise.sample(pos);
            self.points.push(TerrainPoint {
                height: sample.value * edge_mult,
                wetness: 1.,
                grad: -sample.gradient,
            })
        }
        self.make_hydrology_map();
    }

    fn make_hydrology_map(&mut self) {
        const HEIGHT_THRESHOLD: f32 = 0.05;
        //get sources
        for x in 1u32..((1 << Self::CONTINENT_SIZE_PO2) - 1) {
            for y in 1..((1 << Self::CONTINENT_SIZE_PO2) - 1) {
                let id = Self::xy2h(x, y);
                let grad = self.points[id].grad;
                //Compute the angle, and add a perturbation (bigger if the grad is small)
                let angle = grad.angle_to(Vec2::Y)
                    / (PI / 4.)
                        //+ dist.sample(&mut rng) * (1. / (grad.norm() + 0.2)))
                        .round();
                //.clamp(-4., 4.);
                let target = match angle as i32 {
                    -3 => (x - 1, y - 1),
                    -2 => (x - 1, y),
                    -1 => (x - 1, y + 1),
                    0 => (x, y + 1),
                    1 => (x + 1, y + 1),
                    2 => (x + 1, y),
                    3 => (x + 1, y - 1),
                    _ => (x, y - 1),
                };
                let target_id: usize = Self::xy2h(target.0, target.1);
                if self.points[id].height + HEIGHT_THRESHOLD < self.points[target_id].height {
                    self.hydrology[id].dead_end = true;
                    self.hydrology[id].momentum = grad;
                    self.hydrology[id].amount = 1.; //FIXME
                } else {
                    self.hydrology[target_id].visit = 1;
                    self.hydrology[id].momentum = grad;
                    self.hydrology[id].amount = 1.; //FIXME
                }
            }
        }
        info!("Generating map...");
        info!("Finding sources");
        //find sources
        let sources: Vec<usize> = self
            .hydrology
            .iter()
            .enumerate()
            .filter_map(|(i, h)| if h.visit == 0 { Some(i) } else { None })
            .collect();

        info!("Chosing relevant sources");
        let mut estuaries = Vec::<(u32, u32)>::default();
        const SOURCE_CULLING_RADIUS: u32 = 30;
        const SEP_SLOPE_ANGLE: f32 = PI / 2.;
        let mut chosen_sources: BTreeSet<usize> = BTreeSet::default();
        let mut tree: KdTree<U32Value, 10> = KdTree::default();
        for s in sources {
            let (x, y): (u32, u32) = fast_hilbert::h2xy(s as u64, Self::CONTINENT_SIZE_PO2);

            let grad = self.points[s].grad;
            if tree
                .query_point(x, y)
                .filter(|p| p.grad.angle_to(grad).abs() < SEP_SLOPE_ANGLE)
                .next()
                .is_none()
            {
                if self.points[s].height > 0.555 {
                    let val = U32Value {
                        x,
                        y,
                        he: SOURCE_CULLING_RADIUS,
                        grad: self.points[s].grad,
                    };
                    tree.insert(val);
                    chosen_sources.insert(s);
                }
            }

            //dbg!("plop");
        }
        let mut forks = BTreeMap::default();
        let mut to_sea = BTreeMap::default();
        let mut to_lake = BTreeMap::default();
        info!("Generate river paths");
        //make paths
        for s in chosen_sources.iter() {
            self.go_through_path(*s, &mut estuaries, &mut forks, &mut to_sea, &mut to_lake);
        }
        self.lakes = forks
            .iter()
            .filter_map(|(s1, s2)| {
                if self.hydrology[*s1].source == self.hydrology[*s2].source {
                    Some(*s1)
                } else {
                    None
                }
            })
            .collect();
        info!("Propagate water");
        //Reverse order for amounts
        for s in chosen_sources.iter().rev() {
            self.propagate_amount(*s);
        }

        info!("Group estuaries");
        let estuary_groups = self.make_estuary_groups(estuaries, &forks);

        info!("Generate forks");
        self.fork_estuaries(estuary_groups, &mut forks, &mut chosen_sources);
        info!("Generate river curves");
        self.make_curves(&chosen_sources);
        info!("Hydrology done.");

        self.to_sea = to_sea;
        self.to_lake = to_lake;
    }

    fn make_curves(&mut self, sources: &BTreeSet<usize>) {
        const TILES_PER_POINT: u32 = 30;
        let dist = rand_distr::Normal::new(0., 0.5).unwrap();
        let mut rng = rand::rngs::StdRng::seed_from_u64(self.height_noise.seed.0 as u64);

        for s in sources {
            let mut points = Vec::new();
            let mut velocities = Vec::new();

            let origin = *s;
            let mut tile = *s;
            let mut prev = self.to_world(*s) - Vec3::new(1., 0., 1.);

            let mut maxamount = 0f32;
            while self.hydrology[tile].source == origin
                && self.hydrology[tile].next != 0
                && self.hydrology[tile].visit != 2
            {
                let grad = Vec2::from_angle(dist.sample(&mut rng))
                    .rotate(self.hydrology[tile].momentum.normalize())
                    * TILES_PER_POINT as f32
                    / 2.;
                let point = self.to_world(tile);
                let h_grad = (point.y - prev.y) / (point.distance(prev));
                points.push(point);
                velocities.push(Vec3::new(grad.x, h_grad, grad.y));
                self.hydrology[tile].ctrlpoint = true;

                prev = point;
                //go further in the curve
                for _ in 0..TILES_PER_POINT {
                    //Stop if going out of bounds, or if looping on the same source
                    if self.hydrology[tile].next == 0
                        || (self.hydrology[tile].visit == 2
                            && self.hydrology[tile].source == origin)
                    {
                        self.hydrology[tile].visit = 2;
                        break;
                    }
                    // Add visiting if on original river
                    if self.hydrology[tile].source == origin {
                        self.hydrology[tile].visit = 2;
                    }
                    //Stop on ctrl point of other river curve
                    if self.hydrology[tile].ctrlpoint && self.hydrology[tile].source != origin {
                        break;
                    }
                    if self.hydrology[tile].source == origin {
                        maxamount = maxamount.max(self.hydrology[tile].amount);
                    }
                    tile = self.hydrology[tile].next;
                }
            }

            while self.hydrology[tile].source != origin
                && self.hydrology[tile].next != 0
                && self.hydrology[tile].visit != 5
                && !self.hydrology[tile].ctrlpoint
            {
                self.hydrology[tile].visit = 5;
                tile = self.hydrology[tile].next;
            }
            //put last point in curve for nice merging
            self.hydrology[tile].ctrlpoint = true;
            let grad = self.hydrology[tile].momentum.normalize() * 10.;
            let point = self.to_world(tile);
            let h_grad = (point.y - prev.y) / (point.distance(prev));
            points.push(point);
            velocities.push(Vec3::new(grad.x, h_grad, grad.y));

            while points.len() < 3 {
                points.push(points.last().unwrap().clone());
                velocities.push(Vec3::ZERO);
            }
            if maxamount >= 80. {
                self.river_paths.push(CubicHermite::new(points, velocities));
            }
        }
    }

    pub fn to_world(&self, p: usize) -> Vec3 {
        let (x, y) = Self::h2xy(p);
        let (x, y) = (
            x as i32 - Self::CONTINENT_SIZE as i32 / 2,
            y as i32 - Self::CONTINENT_SIZE as i32 / 2,
        );
        let (x, y) = (x as f32 * GRID_SQUARE_SIZE, y as f32 * GRID_SQUARE_SIZE);
        let h = self.points[p].height * Chunk::SCALE_Y + 1.;
        Vec3::new(x, h, y)
    }

    fn fork_estuaries(
        &mut self,
        estuary_groups: BTreeMap<(u32, u32), Vec<(u32, u32)>>,
        forks: &mut BTreeMap<usize, usize>,
        sources: &mut BTreeSet<usize>,
    ) {
        const RIVER_UNMERGE_RADIUS: f32 = 25.;

        for (main, others) in estuary_groups {
            let mut main = Self::xy2h(main.0, main.1);
            let mut prev;
            let mut prevs: Vec<usize> = others.into_iter().map(|(x, y)| Self::xy2h(x, y)).collect();
            while main != 0 && !prevs.is_empty() {
                prev = main;
                for _ in 0..5 {
                    main = self.hydrology[main].prev;
                }

                fn d(a: (u32, u32), b: (u32, u32)) -> f32 {
                    Vec2::new(a.0 as f32, a.1 as f32).distance(Vec2::new(b.0 as f32, b.1 as f32))
                }
                let mut to_remove = Vec::new();
                for (i, v) in prevs.iter_mut().enumerate() {
                    //go back on the main river, then go back on the others to match
                    let main_pos = Self::h2xy(main);
                    let mut pos = Self::h2xy(*v);
                    let mut prev_dist = 1000.;
                    let mut new_dist = d(main_pos, pos);
                    while new_dist < prev_dist {
                        if self.hydrology[*v].prev == 0 {
                            to_remove.push(i);
                            sources.remove(v);
                            new_dist = 0.;
                            break;
                        }
                        *v = self.hydrology[*v].prev;
                        pos = Self::h2xy(*v);
                        prev_dist = new_dist;
                        new_dist = d(main_pos, pos);
                        //Change the fork dest to the main river
                        if let Some(fork) = forks.get_mut(v) {
                            self.hydrology[*fork].next = prev;
                            *fork = prev;
                        }
                    }
                    //split if needed
                    if new_dist > RIVER_UNMERGE_RADIUS {
                        forks.insert(prev, *v);
                        self.hydrology[*v].next = prev;
                        to_remove.push(i)
                    }
                }
                for i in to_remove.into_iter().rev() {
                    prevs.swap_remove(i);
                }
            }
        }
    }

    pub fn xy2h(x: u32, y: u32) -> usize {
        fast_hilbert::xy2h(x, y, Self::CONTINENT_SIZE_PO2) as usize
    }

    pub fn h2xy(h: usize) -> (u32, u32) {
        fast_hilbert::h2xy(h as u64, Self::CONTINENT_SIZE_PO2)
    }

    fn make_estuary_groups(
        &mut self,
        estuaries: Vec<(u32, u32)>,
        forks: &BTreeMap<usize, usize>,
    ) -> BTreeMap<(u32, u32), Vec<(u32, u32)>> {
        //make groups of estuaries
        const ESTUARY_MERGE_RADIUS: u32 = 20;
        let mut estuary_groups: BTreeMap<(u32, u32), Vec<(u32, u32)>> = BTreeMap::default();
        let mut tree: KdTree<U32Value, 10> = KdTree::default();
        for (x, y) in estuaries
            .into_iter()
            .chain(forks.values().map(|h| Self::h2xy(*h)))
        {
            //collect intersecting points
            fn dist(a: &U32Value, b: (u32, u32)) -> f32 {
                Vec2::new(a.x as f32, a.y as f32).distance(Vec2::new(b.0 as f32, b.1 as f32))
            }
            //get closest match
            let min = tree.query_point(x, y).reduce(|a, b| {
                if dist(a, (x, y)) < dist(b, (x, y)) {
                    a
                } else {
                    b
                }
            });

            if let Some(min) = min.cloned() {
                let repr = Self::xy2h(min.x, min.y);
                let current = Self::xy2h(x, y);
                // add to closest group if repr is estuary and not current, or if repr is bigger than current
                if self.hydrology[repr].amount >= self.hydrology[current].amount
                    || (self.points[current].height > Self::OCEAN_HEIGHT_LIMIT
                        && self.points[repr].height <= Self::OCEAN_HEIGHT_LIMIT)
                {
                    estuary_groups
                        .get_mut(&(min.x, min.y))
                        .unwrap()
                        .push((x, y));
                }
                //if current is larger, make it repr
                else {
                    tree.remove_one(min.clone());
                    let val = U32Value {
                        x,
                        y,
                        he: ESTUARY_MERGE_RADIUS,
                        ..Default::default()
                    };
                    tree.insert(val);

                    let mut old = estuary_groups.remove(&(min.x, min.y)).unwrap();
                    old.push((min.x, min.y));
                    estuary_groups.insert((x, y), old);
                }
            } else {
                //create new group
                estuary_groups.insert((x, y), vec![]);
                let val = U32Value {
                    x,
                    y,
                    he: ESTUARY_MERGE_RADIUS,
                    ..Default::default()
                };
                tree.insert(val);
            }
        }
        estuary_groups
    }

    fn propagate_amount(&mut self, s: usize) {
        let mut node = s;
        let mut next = s;
        while next != 0
            && self.hydrology[next].source == self.hydrology[node].source
            && self.hydrology[next].visit != 3
        {
            self.hydrology[next].visit = 3;
            node = next;
            next = self.hydrology[node].next;
            self.hydrology[next].amount += self.hydrology[node].amount;
        }
    }

    fn go_through_path(
        &mut self,
        s: usize,
        estuaries: &mut Vec<(u32, u32)>,
        forks: &mut BTreeMap<usize, usize>,
        to_sea: &mut BTreeMap<usize, usize>,
        to_lake: &mut BTreeMap<usize, usize>,
    ) {
        let mut node: usize = s;
        self.hydrology[node].source = s;
        let mut rng = rand::rngs::StdRng::seed_from_u64(self.height_noise.seed.0 as u64 + s as u64);
        let dist = rand_distr::Normal::new(0., PI / 20.).unwrap();
        let mut skew = 0.;
        let (mut x, mut y) = (0, 0);
        while self.points[node].height > Self::OCEAN_HEIGHT_LIMIT {
            skew = skew + dist.sample(&mut rng);
            let angle = ((self.hydrology[node].momentum.angle_to(Vec2::Y)) / (PI / 2.)).round();
            (x, y) = Self::h2xy(node);
            let offset = match angle as i32 {
                -1 => (-1, 0),
                0 => (0, 1),
                1 => (1, 0),
                _ => (0, -1),
            };
            let target = ((x as i32 + offset.0) as u32, (y as i32 + offset.1) as u32);
            let actual = Vec2::new(offset.0 as f32, offset.1 as f32).normalize()
                * self.hydrology[node].momentum.norm();
            //Corrected momentum to account for movement in the wrong direction
            let corrected = (2. * self.hydrology[node].momentum - actual).normalize()
                * self.hydrology[node].momentum.norm();

            let next: usize = Self::xy2h(target.0, target.1);

            self.hydrology[node].next = next;

            if self.hydrology[next].source != 0 {
                if self.hydrology[next].source == s {
                    to_lake.insert(s, node);
                }
                forks.insert(next, node);
                if let Some(es) = to_sea.get(&self.hydrology[next].source) {
                    to_sea.insert(s, *es);
                } else if let Some(es) = to_lake.get(&self.hydrology[next].source) {
                    to_lake.insert(s, *es);
                } else {
                    warn!("Error : river goes nowhere");
                }
                return;
            }
            self.hydrology[next].source = s;
            self.hydrology[next].prev = node;

            let slowdown = 0.6;

            self.hydrology[next].momentum = Vec2::from_angle(skew.clamp(-0.01, 0.01))
                .rotate(self.hydrology[next].momentum * (1. - slowdown) + corrected * slowdown)
                + corrected.normalize() / 40.;

            node = next;
        }
        to_sea.insert(s, node);
        estuaries.push((x, y));
    }

    pub fn get_hydro(&self, x: u32, y: u32) -> &Hydrologypoint {
        let id: u64 = fast_hilbert::xy2h(x, y, Self::CONTINENT_SIZE_PO2);
        &self.hydrology[id as usize]
    }
}

#[derive(Clone, Default, PartialEq)]
struct U32Value {
    x: u32,
    y: u32,
    he: u32,
    grad: Vec2,
}

impl KdValue for U32Value {
    type Position = u32;

    fn min_x(&self) -> Self::Position {
        self.x - self.he.min(self.x)
    }

    fn min_y(&self) -> Self::Position {
        self.y - self.he.min(self.y)
    }

    fn max_x(&self) -> Self::Position {
        (self.x + self.he).min(Continent::CONTINENT_SIZE)
    }

    fn max_y(&self) -> Self::Position {
        (self.y + self.he).min(Continent::CONTINENT_SIZE)
    }
}

impl Index<(u32, u32)> for Continent {
    type Output = TerrainPoint;

    fn index(&self, index: (u32, u32)) -> &Self::Output {
        &self.points[fast_hilbert::xy2h::<u32>(index.0, index.1, Self::CONTINENT_SIZE_PO2) as usize]
    }
}

impl IndexMut<(u32, u32)> for Continent {
    fn index_mut(&mut self, index: (u32, u32)) -> &mut Self::Output {
        &mut self.points
            [fast_hilbert::xy2h::<u32>(index.0, index.1, Self::CONTINENT_SIZE_PO2) as usize]
    }
}

use bevy::math::{Curve, NormedVectorSpace, Vec2, Vec3, VectorSpace, curve::Interval};
use fast_hilbert;
use foldhash::{HashMap, HashSet};
use kdtree_collisions::{KdTree, KdValue};
use noiz::{
    Noise, Sampleable, SampleableFor,
    cell_noise::MixCellValuesForDomain,
    cells::{OrthoGrid, SimplexGrid, Voronoi, WithGradient},
    curves::Smoothstep,
    math_noise::{NoiseCurve, Pow2, Pow4, PowF},
    misc_noise::WithGradientOf,
    prelude::{
        BlendCellGradients, EuclideanLength, FractalLayers, LayeredNoise, ManhattanLength, Masked,
        MixCellGradients, Normed, NormedByDerivative, Octave, PeakDerivativeContribution,
        PerCellPointDistances, Persistence, QuickGradients, SNormToUNorm, Scaled, SimplecticBlend,
        WorleyLeastDistance,
    },
    rng::{NoiseRng, SNorm},
};
use rand::SeedableRng;
use rand_distr::Distribution;
use std::{
    default,
    f32::consts::PI,
    ops::{Index, IndexMut},
};

use crate::map::GRID_SQUARE_SIZE;

#[derive(Default, Clone)]
struct TestCurve;
impl Curve<f32> for TestCurve {
    fn domain(&self) -> Interval {
        Interval::new(0., 1.).unwrap()
    }

    fn sample_unchecked(&self, t: f32) -> f32 {
        1. - (t + 0.33)
    }
}

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
            //WithGradientOf::<Vec2>
        ),
        (
            Scaled<f32>,
            BlendCellGradients<SimplexGrid, SimplecticBlend, QuickGradients, true>,
            SNormToUNorm,
            Pow2,
        ),
    >,
    Pow2,
    Scaled<f32>,
);
pub struct TerrainPoint {
    pub height: f32,
    pub wetness: f32,
    pub grad: Vec2,
}
#[derive(Clone, Default)]
pub struct Hydrologypoint {
    pub momentum: Vec2,
    pub amount: f32,
    dead_end: bool,
    pub visit: bool,
    pub source: usize,
    next: usize,
}

pub struct Continent {
    points: Vec<TerrainPoint>,
    hydrology: Vec<Hydrologypoint>,
    height_noise: NoiseT,
    offset: Vec2,
}

impl Continent {
    pub const CONTINENT_SIZE_PO2: u8 = 11;
    pub const CONTINENT_SIZE: u32 = 1 << Self::CONTINENT_SIZE_PO2;

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
        let dist = rand_distr::Normal::new(0., 0.1).unwrap();
        let mut rng = rand::rngs::StdRng::seed_from_u64(self.height_noise.seed.0 as u64);
        //get sources
        for x in 1u32..((1 << Self::CONTINENT_SIZE_PO2) - 1) {
            for y in 1..((1 << Self::CONTINENT_SIZE_PO2) - 1) {
                let id = fast_hilbert::xy2h(x, y, Self::CONTINENT_SIZE_PO2) as usize;
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
                let target_id: usize =
                    fast_hilbert::xy2h(target.0, target.1, Self::CONTINENT_SIZE_PO2) as usize;
                if self.points[id].height + HEIGHT_THRESHOLD < self.points[target_id].height {
                    self.hydrology[id].dead_end = true;
                    self.hydrology[id].momentum = grad;
                    self.hydrology[id].amount = 1.; //FIXME
                } else {
                    self.hydrology[target_id].visit = true;
                    self.hydrology[id].momentum = grad;
                    self.hydrology[id].amount = 1.; //FIXME
                }
            }
        }

        //find sources
        let sources: Vec<usize> = self
            .hydrology
            .iter()
            .enumerate()
            .filter_map(|(i, h)| if h.visit == false { Some(i) } else { None })
            .collect();

        let mut estuaries = HashSet::<(u32, u32)>::default();
        const SOURCE_CULLING_RADIUS: u32 = 10;
        const SEP_SLOPE_ANGLE: f32 = PI / 4.;
        let mut chosen_sources: Vec<usize> = Vec::default();
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
                if self.points[s].height > 0.545 {
                    let val = U32Value {
                        x,
                        y,
                        he: SOURCE_CULLING_RADIUS,
                        grad: self.points[s].grad,
                    };
                    tree.insert(val);
                    chosen_sources.push(s);
                }
            }

            //dbg!("plop");
        }

        //make paths
        for s in chosen_sources.iter() {
            self.go_through_path(*s, &mut estuaries);
        }
        //Reverse order for amounts
        for s in chosen_sources.iter().rev() {
            self.propagate_amount(*s);
        }

        //make groups of estuaries
        const ESTUARY_MERGE_RADIUS: u32 = 20;
        let mut estuary_groups: HashMap<(u32, u32), Vec<(u32, u32)>> = HashMap::default();
        tree = KdTree::default();
        for (x, y) in estuaries {
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
                // add to closest group
                estuary_groups
                    .get_mut(&(min.x, min.y))
                    .unwrap()
                    .push((x, y));
            } else {
                //create new group
                estuary_groups.insert((x, y), vec![(x, y)]);
                let val = U32Value {
                    x,
                    y,
                    he: ESTUARY_MERGE_RADIUS,
                    ..Default::default()
                };
                tree.insert(val);
            }
        }
    }

    fn propagate_amount(&mut self, s: usize) {
        let mut node = s;
        let mut next = self.hydrology[node].next;
        while next != 0
            && self.hydrology[next].source == self.hydrology[node].source
            && self.hydrology[node].visit == false
        {
            self.hydrology[next].amount += self.hydrology[node].amount;
            self.hydrology[node].visit = true;
            node = next;
            next = self.hydrology[node].next;
        }
        println!(
            "{} - {} - {} {} - {}",
            self.hydrology[node].amount,
            next,
            self.hydrology[next].source,
            self.hydrology[node].source,
            self.hydrology[node].visit
        );
    }

    fn go_through_path(&mut self, s: usize, estuaries: &mut HashSet<(u32, u32)>) {
        let mut node = s;
        self.hydrology[node].source = s;
        let mut rng = rand::rngs::StdRng::seed_from_u64(self.height_noise.seed.0 as u64);
        let dist = rand_distr::Normal::new(0., PI / 20.).unwrap();
        let mut skew = 0.;
        let (mut x, mut y) = (0, 0);
        while self.points[node].height > 0.534 {
            skew = skew + dist.sample(&mut rng);
            let angle = ((self.hydrology[node].momentum.angle_to(Vec2::Y)) / (PI / 2.)).round();
            (x, y) = fast_hilbert::h2xy(node as u64, Self::CONTINENT_SIZE_PO2);
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
            let target_id: usize =
                fast_hilbert::xy2h(target.0, target.1, Self::CONTINENT_SIZE_PO2) as usize;

            self.hydrology[node].visit = false;
            self.hydrology[node].next = target_id;

            if self.hydrology[target_id].source != 0 {
                return;
            }
            self.hydrology[target_id].source = self.hydrology[node].source;

            let slowdown = 0.9;

            self.hydrology[target_id].momentum = Vec2::from_angle(skew.clamp(-0.1, 0.1))
                .rotate(self.hydrology[target_id].momentum + corrected * slowdown);

            node = target_id;
        }

        estuaries.insert((x, y));
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

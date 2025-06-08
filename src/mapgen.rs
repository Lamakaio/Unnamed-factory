use bevy::math::{Curve, curve::Interval};
use fast_hilbert;
use noiz::{
    Noise,
    cell_noise::MixCellValuesForDomain,
    cells::{OrthoGrid, SimplexGrid},
    curves::Smoothstep,
    math_noise::{NoiseCurve, Pow4},
    prelude::{
        BlendCellGradients, EuclideanLength, FractalLayers, LayeredNoise, Masked, MixCellGradients,
        Normed, NormedByDerivative, Octave, PeakDerivativeContribution, Persistence,
        QuickGradients, SNormToUNorm, Scaled, SimplecticBlend,
    },
    rng::{NoiseRng, SNorm},
};
use std::ops::{Index, IndexMut};

#[derive(Default, Clone)]
struct TestCurve;
impl Curve<f32> for TestCurve {
    fn domain(&self) -> Interval {
        Interval::new(0., 1.).unwrap()
    }

    fn sample_unchecked(&self, t: f32) -> f32 {
        1. - (2. * t + 0.75).clamp(0., 1.)
    }
}

type NoiseT = Noise<(
    LayeredNoise<
        Normed<f32>,
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
    BlendCellGradients<SimplexGrid, SimplecticBlend, QuickGradients>,
    NoiseCurve<TestCurve>,
);

type ContinentNoiseT = (
    LayeredNoise<
        NormedByDerivative<f32, EuclideanLength, PeakDerivativeContribution>,
        Persistence,
        FractalLayers<Octave<MixCellGradients<OrthoGrid, Smoothstep, QuickGradients, true>>>,
    >,
    SNormToUNorm,
);

type FlatnessNoiseT = (
    Scaled<f32>,
    BlendCellGradients<SimplexGrid, SimplecticBlend, QuickGradients>,
    SNormToUNorm,
    Pow4,
);

pub struct TerrainPoint {
    pub height: f32,
    pub wetness: f32,
}

pub struct Continent {
    points: Vec<TerrainPoint>,
    height_noise: NoiseT,
}

impl Continent {
    const CONTINENT_SIZE_PO2: u8 = 12;

    fn new_and_generate(seed: u32) -> Self {
        let mut new = Self {
            points: Vec::with_capacity(1 << (2 * Self::CONTINENT_SIZE_PO2)),
            height_noise: Self::get_noise(seed),
        };
        new.generate();
        new
    }

    fn get_noise(seed: u32) -> NoiseT {
        Noise {
            noise: (
                LayeredNoise::new(
                    Normed::<f32>::default(),
                    Persistence(1.),
                    (
                        Octave(
                            (
                                (
                                    Scaled(0.1),
                                    noiz::prelude::Offset::<
                                        MixCellValuesForDomain<OrthoGrid, Smoothstep, SNorm>,
                                    > {
                                        offset_strength: 0.4,
                                        ..Default::default()
                                    },
                                    BlendCellGradients::<
                                        SimplexGrid,
                                        SimplecticBlend,
                                        QuickGradients,
                                    >::default(),
                                    NoiseCurve::<TestCurve>::default(),
                                ),
                                Scaled(0.3),
                            ),
                        ),
                        Octave(
                            Masked(
                                (
                                    LayeredNoise::new(
                                        NormedByDerivative::<
                                            f32,
                                            EuclideanLength,
                                            PeakDerivativeContribution,
                                        >::default()
                                        .with_falloff(0.3),
                                        Persistence(0.6),
                                        FractalLayers {
                                            layer: Octave::<
                                                MixCellGradients<
                                                    OrthoGrid,
                                                    Smoothstep,
                                                    QuickGradients,
                                                    true,
                                                >,
                                            >::default(
                                            ),
                                            lacunarity: 1.6,
                                            amount: 6,
                                        },
                                    ),
                                    SNormToUNorm::default(),
                                ),
                                (
                                    Scaled(0.1),
                                    BlendCellGradients::<
                                        SimplexGrid,
                                        SimplecticBlend,
                                        QuickGradients,
                                    >::default(),
                                    SNormToUNorm::default(),
                                    Pow4::default(),
                                ),
                            ),
                        ),
                    ),
                ),
                SNormToUNorm::default(),
            ),
            seed: NoiseRng(seed),
            frequency: 0.03,
            ..Default::default()
        }
    }

    fn generate(&mut self) {
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

//! Climate (moisture, temperature) and biomes of the final terrain.

use bevy_math::Vec2;

use crate::{
    Biome, WorldgenSettings,
    flow::neighbours,
    noise::Noise,
    terrain::{blur, rain_from},
};

pub struct Climate {
    /// Relative moisture (mean ~1 on land): rain plus wet valley floors.
    pub moisture: Vec<f32>,
    /// 0 (snow line and above) .. 1 (warm lowlands).
    pub temperature: Vec<f32>,
    pub biome: Vec<Biome>,
}

pub fn climate(
    settings: &WorldgenSettings,
    noise: Noise,
    heights: &[f32],
    base_level: &[bool],
    shore: &[f32],
    sediment: &[f32],
    wetland: &[f32],
    lake_of: &[Option<u16>],
    drainage: &[f32],
) -> Climate {
    let size = settings.size;
    let n = size * size;
    let water: Vec<bool> = (0..n).map(|i| base_level[i] || lake_of[i].is_some()).collect();
    let rain = rain_from(settings, heights, &water, 1.);
    let jitter = noise.layer(20);
    let mut moisture = vec![0.; n];
    let mut temperature = vec![0.; n];
    let mut biome = vec![Biome::Sea; n];

    // Valley floors along big drainage are wet. Spread over a few cells: single-cell drainage
    // lines run straight across flat ground and would show as lines of vegetation.
    let valley: Vec<f32> = drainage
        .iter()
        .map(|d| (d.max(1.).log10() - 2.).clamp(0., 2.) * 0.35)
        .collect();
    let valley = blur(size, &blur(size, &valley, 2), 2);

    for i in 0..n {
        let (x, z) = (i % size, i / size);
        let p = Vec2::new(x as f32, z as f32);
        let h = heights[i];
        let wobble = jitter.fbm(p / 25., 3) * 0.12;

        // Deltas are wetlands.
        moisture[i] = rain[i] * (1. + wobble) + valley[i] * 1.5 + wetland[i] * 1.5;
        temperature[i] = (1. - h / settings.snow_line + wobble * 0.6).clamp(0., 1.);

        let slope = neighbours(size, i)
            .map(|(j, distance)| (heights[j] - h).abs() / distance)
            .fold(0., f32::max);

        // By height: deltas turned some sea cells into land.
        biome[i] = if h <= 0. {
            Biome::Sea
        } else if lake_of[i].is_some() {
            Biome::Lake
        } else if shore[i] < beach_width(noise, p, sediment[i])
            && h < 2. + sediment[i]
            && slope < 0.5
        {
            Biome::Beach
        } else if temperature[i] < 0.08 {
            Biome::Snow
        } else if slope > 1.4 || (temperature[i] < 0.25 && slope > 0.8) {
            Biome::Rock
        } else if temperature[i] < 0.3 {
            Biome::Alpine
        } else if moisture[i] > 1.5 && slope < 0.08 && h < 12. {
            Biome::Marsh
        } else if moisture[i] < 0.55 + wobble {
            Biome::Steppe
        } else if moisture[i] < 0.95 + wobble {
            Biome::Meadow
        } else if temperature[i] < 0.55 + wobble {
            Biome::ConiferForest
        } else {
            Biome::DeciduousForest
        };
    }

    Climate {
        moisture,
        temperature,
        biome,
    }
}

/// Width of the beach (cells from the shore): varying along the coast, and wide on the
/// sediment fans of river mouths.
fn beach_width(noise: Noise, p: Vec2, sediment: f32) -> f32 {
    let variation = 0.5 + 0.5 * noise.layer(21).fbm(p / 45., 3);
    1. + 3.5 * variation * variation + 9. * sediment
}

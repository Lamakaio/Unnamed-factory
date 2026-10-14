//! Distribution of a tectonic history's uplift (young and old), crust and oceanic age.
//!
//! `cargo run -p worldgen --release --example tectonics_stats -- [seeds...]`

use worldgen::{
    grid::Grid,
    tectonics::{TectonicSettings, run},
};

fn main() {
    let seeds: Vec<u64> = std::env::args().skip(1).map(|a| a.parse().unwrap()).collect();
    for seed in if seeds.is_empty() { vec![1082] } else { seeds } {
        let t = run(&TectonicSettings { seed, ..Default::default() });
        let percentiles = |mut v: Vec<f32>| {
            v.sort_by(f32::total_cmp);
            [0.5, 0.9, 0.99, 1.].map(|p| v[((v.len() - 1) as f32 * p) as usize])
        };
        let young: Vec<f32> = (0..t.grid.len()).map(|i| t.orogeny[i] * (-t.orogeny_age[i] / 8.).exp()).collect();
        let uplifted = t.orogeny.iter().filter(|&&o| o > 0.).count() as f32 / t.orogeny.len() as f32;
        println!("seed {seed}: uplifted {:.1}% of cells", 100. * uplifted);
        println!("  orogeny p50/p90/p99/max {:?}", percentiles(t.orogeny.clone()));
        println!("  young   p50/p90/p99/max {:?}", percentiles(young));
        println!("  ocean age {:?}", percentiles(t.age.iter().copied().filter(|&a| a > 0.).collect()));
    }
}

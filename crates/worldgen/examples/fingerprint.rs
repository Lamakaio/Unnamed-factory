//! A fingerprint of generated worlds (heights, surface, rivers, lakes), to check that a
//! refactor leaves generation unchanged.
//!
//! `cargo run -p worldgen --release --example fingerprint -- [seed...]`

use worldgen::{WorldgenSettings, generate};

fn main() {
    let seeds: Vec<u64> = std::env::args().skip(1).map(|s| s.parse().unwrap()).collect();
    let seeds = if seeds.is_empty() { vec![1082] } else { seeds };
    for seed in seeds {
        let c = generate(&WorldgenSettings { seed, ..Default::default() });
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |bits: u32| {
            hash ^= bits as u64;
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        };
        c.heights.iter().for_each(|h| mix(h.to_bits()));
        c.surface.values.iter().for_each(|h| mix(h.to_bits()));
        for river in &c.rivers {
            river.points.iter().for_each(|p| {
                mix(p.position.x.to_bits());
                mix(p.position.y.to_bits());
                mix(p.width.to_bits());
            });
        }
        c.lakes.iter().for_each(|lake| mix(lake.level.to_bits()));
        println!("seed {seed}: {hash:016x} ({} rivers, {} lakes)", c.rivers.len(), c.lakes.len());
    }
}

//! Print a planet's cells around a longitude / latitude (degrees).
//!
//! `cargo run -p worldgen --release --example planet_probe -- <n> <seed> <lon> <lat>`

use bevy_math::Vec3;
use worldgen::{
    planet::{PlanetSettings, generate_planet},
    surface::PlanetSurface,
};

fn main() {
    let args: Vec<f32> = std::env::args()
        .skip(1)
        .map(|a| a.parse().unwrap())
        .collect();
    let planet = generate_planet(&PlanetSettings {
        n: args[0] as usize,
        seed: args[1] as u64,
        ..Default::default()
    });
    let surface = PlanetSurface::new(&planet, None);
    for dlat in [-1.5f32, -0.75, 0., 0.75, 1.5] {
        let mut line = String::new();
        for dlon in [-1.5f32, -0.75, 0., 0.75, 1.5] {
            let (lon, lat) = ((args[2] + dlon).to_radians(), (args[3] + dlat).to_radians());
            let d = Vec3::new(lat.cos() * lon.cos(), lat.sin(), lat.cos() * lon.sin());
            let i = planet.grid.cell_at(d);
            line += &format!(
                "{:>6.2}/{:>6.2}{}{:<5} ",
                planet.heights[i],
                surface.height_at(d),
                if planet.sea[i] { "S" } else { " " },
                if planet.lake_depth[i] > 0.05 {
                    format!("L{:.1}", planet.lake_depth[i])
                } else {
                    String::new()
                }
            );
        }
        println!("lat {:+.2}: {line}", args[3] + dlat);
    }
}

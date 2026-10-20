//! Generation timings: wall time per stage and the mean number of busy cores during it (the
//! process's CPU time, sampled; Windows only, elsewhere the wall time alone), for finding the
//! stages that use few cores. `cargo run -p worldgen --release --example timings [-- <n>]`.
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use worldgen::{planet::*, climate::*, surface::PlanetSurface};

/// The process's CPU time so far (seconds, all threads).
#[cfg(windows)]
fn cpu() -> f64 {
    #[repr(C)]
    struct FileTime {
        low: u32,
        high: u32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn GetProcessTimes(
            p: isize,
            c: *mut FileTime,
            e: *mut FileTime,
            k: *mut FileTime,
            u: *mut FileTime,
        ) -> i32;
    }
    let zero = || FileTime { low: 0, high: 0 };
    let (mut created, mut exited, mut kernel, mut user) = (zero(), zero(), zero(), zero());
    // SAFETY: plain out-parameters for the current process's times.
    unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    };
    let seconds = |f: &FileTime| ((f.high as u64) << 32 | f.low as u64) as f64 * 1e-7;
    seconds(&kernel) + seconds(&user)
}

#[cfg(not(windows))]
fn cpu() -> f64 {
    0.
}

fn main() {
    let start = Instant::now();
    let samples = Arc::new(Mutex::new(vec![(0f64, cpu())]));
    let done = Arc::new(AtomicBool::new(false));
    let sampler = {
        let (s, d) = (samples.clone(), done.clone());
        std::thread::spawn(move || {
            while !d.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(20));
                s.lock()
                    .unwrap()
                    .push((start.elapsed().as_secs_f64(), cpu()));
            }
        })
    };
    let n = std::env::args()
        .nth(1)
        .map_or(1024, |n| n.parse().expect("cells per face edge"));
    let planet = generate_planet(&PlanetSettings {
        seed: 1082,
        n,
        ..Default::default()
    });
    for t in planet.timings.iter().filter(|t| t.0.starts_with("    ")) {
        println!("  {:<22} {:>6.2?}", t.0.trim(), t.1);
    }
    let mut stages: Vec<(String, f64)> = planet
        .timings
        .iter()
        .filter(|t| !t.0.starts_with("    "))
        .filter(|t| t.0 != "erosion")
        .map(|t| (t.0.trim().to_string(), t.1.as_secs_f64()))
        .collect();
    let t = Instant::now();
    let climate = simulate(&planet, &ClimateSettings::default());
    stages.push(("climate".into(), t.elapsed().as_secs_f64()));
    let t = Instant::now();
    let surface = PlanetSurface::new(&planet, Some(&climate));
    stages.push(("surface".into(), t.elapsed().as_secs_f64()));
    let t = Instant::now();
    let _ = worldgen::sea::sea_field(&planet, &surface);
    stages.push(("sea".into(), t.elapsed().as_secs_f64()));
    done.store(true, Ordering::Relaxed);
    sampler.join().unwrap();
    let samples = samples.lock().unwrap();
    let at = |time: f64| {
        let k = samples
            .partition_point(|s| s.0 < time)
            .min(samples.len() - 1);
        samples[k].1
    };
    let mut clock = 0.;
    let (mut wall, mut busy) = (0., 0.);
    for (name, d) in &stages {
        let cores = (at(clock + d) - at(clock)) / d;
        if cfg!(windows) {
            println!("{name:<24} {d:>6.2}s  {cores:>5.1} cores");
        } else {
            println!("{name:<24} {d:>6.2}s");
        }
        clock += d;
        wall += d;
        busy += cores * d;
    }
    println!("total {wall:.1}s, {:.1} cores on average", busy / wall);
}

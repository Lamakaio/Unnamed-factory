# AGENTS.md — Unnamed Factory: project recap & Bevy guidelines

> Audience: AI agents (and humans) about to work on this repo. Read this first.
> Written 2026-10-09. Bevy facts were checked against the official migration guides,
> release notes and the `v0.20.0` examples on that date. **The code is on Bevy 0.20**
> (upgraded version by version on 2026-10-09; see §4). §3 describes the architecture, which
> the upgrade left largely unchanged. When an API detail here disagrees
> with the compiler or docs.rs, the compiler wins; fix this file.

---

## 1. What this project is

**Since 2026-10-13: a life simulator on a living planet.** The game is turning into an
evolution simulation over deep time on a small spherical planet whose tectonics, erosion and
climate keep evolving. The design draft and all decisions are in
[`docs/living-world.md`](docs/living-world.md) (small cube-sphere planet, R ≈ 650 units;
two clocks: game time and deep time; evolution as the life simulation; sphere first). The
city builder was **retired** (removed from the app, kept in git history before commit
"Retire the city builder").

### The former city-builder design (history)

The crate is called `unnamed-factory`; it started as a **3D city builder** in Rust with
Bevy. Setting: a "perfect world" (no crime, no war). The game was about **occupations,
buildings, zones and resource flows**.

Design source: [`notes.md`](notes.md) (preliminary, in English with some typos). Summary:

- **Population** is split into age groups and **occupations**. Each occupation has a
  population, a demand, productivity, happiness and commute:
  - *Collecter* (food from farms/fishing, raw materials from forests/mines),
    *Craftperson* (refines materials, boosts productivity), *Builder* (speeds up construction,
    maintains buildings and amenities), *Researcher* (science, demand driven by idleness),
    *Teacher* (education → productivity, demand driven by births), *Artist* (happiness,
    immigration, fame). Optional: *Emergency services* (disasters).
- **Demand** drives immigration and the job new inhabitants pick. The player can steer
  new inhabitants toward a job.
- **Resources**: money (from trade only, no taxes), food (stored, spoils, has upkeep),
  materials (raw/refined), science. **Stats**: happiness, productivity, population,
  commute, fame, immigration. Happiness and research are not stored.
- **Zones**: collecting district, working district, habitation district. There are no
  commercial or industrial zones ("smart towns").
- **Amenities**: electricity, water, waste. They need builder upkeep. Missing amenities
  lower happiness and stop some buildings.
- **Transportation**: each inhabitant computes shortest paths to its destinations and
  picks one at random. Overloaded links slow down. Cars exist but are nerfed.
- **Migration** between cities depends on demand, happiness and distance.
- **Implementation intent** (from the notes): put sim details in a **scripting language**
  for fast iteration, with typed object tables and linear "links" batched into matrix
  multiplies. A Rhai prototype existed; it was retired with the city builder.

### Assets & licensing
- `assets/models/*.glb`, the fonts and some images come from **purchased asset packs**.
  The user has many more packs available.
- `.gitignore` excludes `*.glb`, so models are **not in git**. Never commit or
  redistribute purchased assets.
- Data-driven content lives in `assets/` as RON files with custom extensions (`.mapmat`).
  The purchased building models (`assets/models/*.glb`) are unused since the city builder
  was retired.

---

## 2. Repo map (as of the `bevy-upgrade` branch, 2026-10-20)

```
Cargo.toml              bevy 0.20 (+ bevy_remote, file_watcher, bevy_dev_tools, render_dev_tools),
                        half, worldgen (path). Workspace with crates/worldgen. All versions pinned.
                        Feature `tracy`: `cargo run --features tracy` for Tracy profiling.
Cargo.lock              tracked
rust-toolchain.toml     pinned nightly (nightly-2026-10-08). The user prefers nightly.
.cargo/config.toml      (git-ignored, local) rust-lld linker + `jobs = -4` to keep the PC usable while compiling
notes.md                former city-builder design notes (see §1)
docs/living-world.md    living planet design and decisions (the current direction)
src/main.rs             assembles plugins; `AppState { Loading, InGame }`; `--seed`, `--n`
src/camera.rs           spawns the 3D camera (HDR, bloom, TAA, ambient light); the planet camera takes it over
src/environment.rs      the sun (F rotates it)
src/dev_tools.rs        BRP remote, F3 wireframe, `UF_SCREENSHOT`
src/planet/             PlanetPlugin: generation task, chunks (mesh.rs, LOD), camera.rs, water.rs (+ river_profile.rs),
                        vegetation.rs (placement) + nature.rs (models, species, LODs), grass.rs (near blades),
                        inspect.rs (click reports, issue reports)
src/shaders.rs          materials: PlanetTerrain, PlanetWater, Toon, LodToon, SwayToon, GrassBlade
src/ui.rs               loading text (BSN), `label()` text scene, PointerOverUi system param
crates/worldgen/        world generation (no Bevy ECS), in pipeline order:
                        tectonics.rs → planet/ (`generate_planet` in mod.rs runs the stages: shape.rs: plates,
                        provinces, `Shaper`, `terrain.rs` erosion; coast.rs: world units, sea floor, shores; the
                        features: fjords.rs, volcanoes.rs, reefs.rs, barriers.rs; water.rs: lakes.rs, rivers.rs,
                        floodplains, mouths.rs (+ channels.rs)) → climate.rs → surface/ (mod.rs: `PlanetSurface`;
                        field.rs: `FaceField`; features.rs: riparian, karst, deserts, sea ice; relief.rs: gullies,
                        dunes, karst towers; channels.rs: carved rivers; glaciers.rs, springs.rs) → sea.rs;
                        shared: sphere.rs + grid.rs (cube-sphere grid), flow.rs (routing, filling), par.rs
                        (all parallelism), noise.rs, geology.rs (rock types), detail.rs (gullies), world.rs
                        (`World`: age, star, orbit, atmosphere, ocean, albedos, life; presets; `docs/evolution.md`)
crates/worldgen/examples planet_preview, planet_probe, climate_preview, tectonics_preview, tectonics_stats, timings
tools/nature/           asset pipeline for the nature models (CC0 Kenney + Quaternius packs → Blender LOD builder, see its README)
assets/models/nature/   generated nature models <name>_lod{0,1,2}.glb (git-ignored)
assets/shaders/         planet_terrain.wesl, planet_water.wesl, toon.wesl, toon_sway.wesl, grass_blades.wesl
issues/                 (git-ignored) issue reports from the game (`planet/inspect.rs`); `fixed/` the solved ones
CREDITS.md              sources of every borrowed technique (papers, articles, shaders); keep it up to date
```

The flat continent (the former city builder's map: `src/map`, the flat worldgen pipeline,
its shaders and examples) was removed on 2026-10-20; the planet reuses its techniques (see the
history in git and the notes below, "as on the flat map").

### Controls
Camera: left or middle drag pan · Q / E turn (physical keys: A / E on AZERTY) · right drag
orbit · wheel zoom · arrows move. Click the ground: report in the log. F6 or the button: report
an issue. Debug: F rotate sun · F3 wireframe.
(Bevy's `RenderDebugOverlay` defaults to F1/F2 but is disabled by default.)

---

## 3. Current architecture (how it actually works)

### 3.1 App (`main.rs`, `camera.rs`, `environment.rs`, `dev_tools.rs`)
`main.rs` adds `DefaultPlugins` (nearest-neighbour images, includes TAA and UI widgets),
`MeshPickingPlugin`, and one plugin per module. `AppState` starts in `Loading` (the planet is
generated in a background task, "Generating world..." text, ~30 s at `--n 1024`) and moves to
`InGame` once it exists (`cargo run --release -- [--seed <n>] [--n <cells>]`, default seed
1082, n 1024).
- **Camera** (`camera.rs`): HDR, Bloom, TAA, depth prepass, a weak sky-blue `AmbientLight`
  override so the sun shapes the relief. `planet::camera` takes it over (`PlanetCamera`).
- **Environment**: sun `DirectionalLight` (`lux::RAW_SUNLIGHT`), 4 shadow cascades (first bound
  25, max 600) with a 4096 shadow map; the planet camera keeps it lighting the side in view.
- **Dev tools**: `RemotePlugin` + `RemoteHttpPlugin` (BRP inspector), `WireframePlugin`.
  `UF_SCREENSHOT=<png>` (and `UF_SCREENSHOT_DELAY`, default 20 s after the world is ready)
  saves a screenshot and exits; `UF_SCREENSHOTS=<view>=<png>;<view>=<png>;...` takes several
  from one world generation (the camera moves to each view, waits `UF_SCREENSHOT_DELAY`,
  default 10 s, saves): use it rather than one run per view; with `UF_PLANET_CAMERA` (view) and `UF_PLANET_REPORT` (logged
  reports of points) agents check spots without a human. See §6.

### 3.2 World generation (`crates/worldgen`) and the planet (`src/planet`)
- **`worldgen` crate** (workspace member, no Bevy ECS; depends on `bevy_math`, `bevy_tasks`
  (its own thread pool, see `par.rs`) and `rand`; `opt-level = 3` even in dev). Noise is the
  crate's own `noise.rs` (Perlin 2D/3D + fBm / ridged / warp). Erosion, flow routing and blurs
  are generic over a `Grid` (`grid.rs`; the only one is the cube-sphere, `sphere.rs`).
  Parameters: `PlanetSettings` (planet/mod.rs), `terrain::ErosionSettings` (built from it per
  erosion stage), `ClimateSettings`; everything else is module constants. Disc queries ("every
  cell within r") go through `SphereGrid::for_cells_within` (a window scan inside a face).
- **Rock types** (`geology.rs`, `Rock`): granite (cratons, old ranges' roots, some plains and
  hills), metamorphic (young belts' cores), basalt (arcs, rifts, the sea floor), sandstone
  (plateaus, foothills), shale (plains), limestone (patches) — `planet/shape.rs::rock_of`, from the
  boundaries, old ranges, provinces and a noise. Each scales the erodibility and diffusion
  (`PROPERTIES`); sandstone, shale and limestone are layered: hard and soft beds
  (`bedding`) along strata surfaces `height + offset = const`, the offset a smooth field
  whose slope is the dip (`STRATA_WAVELENGTH`; ~0.02 on plateaus, steep in foothills:
  cuestas), so erosion carves steps, mesas, wide valleys in soft rock, narrow gorges in hard
  rock. `Planet::rock`, `strata`, `strata_scale` (world units per erosion unit, from the
  height scaling). The game draws them: a rock field texture per face (`Rgba16Float`: rock
  id, strata offset; `planet/mod.rs::rock_textures`, a terrain material per face) read by
  `planet_terrain.wesl::rock_color` (palettes by rock, beds along the same strata surfaces,
  irregular borders from a warped nearest cell); `planet/vegetation.rs::scatter_rocks`
  strews rocks by rock type (`nature::scattered_rocks`) where the ground is steep, dry or
  cold (in fields), `PlanetSurface::rock_at`.
- **Fjords** (`fjords.rs`, after the coasts are shaped, before the volcanoes): poleward of
  `FJORD_LATITUDE`, valleys draining at least `MIN_DRAINAGE` to the sea with `MIN_RELIEF`
  ground within `RELIEF_RADIUS` of the mouth (mouths `SPACING` apart) are traced up the
  drainage for `LENGTH` cells and carved into U-shaped troughs below sea level (deepest a
  little inland, a sill at the mouth); the sea floods the carved cells.
- **Glaciers and ice caps** (`glaciers.rs`, static, at the surface stage since they need the
  climate): ice where the warmest month at the cell's height is below `ICE_WARMEST`, only in
  valleys (`VALLEY_DEPTH` below the ground blurred `VALLEY_RADIUS`) and on flat ground, never
  steeper than `MAX_SLOPE` (ridges stay bare; below `SHEET_WARMEST` ice sheets cover all but
  steep ground; small holes closed), valley fill capped at `MAX_FILL`; tongues
  down the steepest way while it is below `TONGUE_WARMEST` (at most `TONGUE_LENGTH` cells).
  The thickness (filling valleys to the smoothed ground, doming away from the edge) is added
  to `PlanetSurface::heights`; `PlanetSurface::ice` (0..1) clears lakes, marsh, gullies,
  vegetation, grass and rocks, trims river sources under the ice (they start at the snouts)
  and goes to the shader as the rock field's negative alpha (bare ice, crevasses where it
  steepens, firn on top, moraines at the edge).
- **Biomes from the climate class** (`PlanetSurface::koppen_at`, the nearest cell's Köppen
  class at its height; `planet/vegetation.rs::biome`): rainforest (Af, Am: jungle trees,
  bamboo), cloud forest (tropical forest cooled to a temperate class above
  `CLOUD_FOREST_HEIGHT`), savanna (Aw: grass, scattered acacias), mangroves (low tropical
  shores in stretches, standing in the shallows down to `MANGROVE_DEPTH`), tundra (ET poleward
  of `TUNDRA_SIN`: shrubs, stones) and alpine meadows (ET on mountains: grass, flowers, rocks).
- **Desert types** (`surface/features.rs::deserts`): in desert climates (BWh, BWk), sand seas (ergs)
  on gentle low ground in noise stretches (`ERG_*`), with transverse dunes added to the
  heights (`PlanetSurface::dunes`: crests north–south, a whole number round the planet,
  slip faces west); badlands on soft rocks (shale, sandstone) sloping more than
  `BADLANDS_SLOPE` in dry climates (deeper gullies, bare banded rock); the rest is stony
  desert (reg: gravel, pebbles, clay flats); salt flats as before. `PlanetSurface::{erg,
  badlands}` reach the shader packed in the rock field (lava's negative, the rock id's
  fraction); `Biome::{Desert, Erg, Badlands}`.
- **Sea ice** (`PlanetSurface::sea_ice`, static): pack ice where the warmest month at sea
  level stays below `SEA_ICE_WARMEST`; sent as the sea wave field's 4th channel
  (`water::sea_textures`) and drawn by `planet_water.wesl` as floes and leads (no waves).
- **Karst** (`surface/features.rs::karst`): on limestone, tower karst in the humid tropics and
  subtropics (Af, Am, Aw, Cwa, Cfa) and sinkholes in other wet climates, added to the heights
  (`PlanetSurface::karst_relief`: features on a jittered lattice `KARST_SPACING` apart).
- **Tectonic history sampling**: its coarse fields (crust, young and old uplift) are sampled
  at points warped at two scales (`planet/shape.rs::HISTORY_WARP`) and belts fade across the
  continents' edge: its fronts would otherwise run straight for hundreds of cells.
  `SphereGrid::bilinear` and `PlanetClimate::months_at` sample across the faces' edges (no
  seams).
- **Volcanoes** (`volcanoes.rs`, after the coasts are shaped, before the lakes): `place`
  spaces stratovolcanoes along island arcs (`Boundary::arc`) and continental arcs (young
  belts within `COASTAL_ARC` of the sea), `ARC_SPACING`, and hotspot chains
  (`HOTSPOTS_PER_MILLION` plumes, `CHAIN` volcanoes, `CHAIN_SPACING` apart along the plate's
  motion: `PlateMotion::rotation` or the plates' without history; age grows downstream).
  Then each gets a kind (`VolcanoKind`: arcs mostly `Strato`, some `Massif`, `Caldera`,
  `ConeField` on land; hotspots `Shield`, under continents often `Caldera` or `ConeField`)
  and a random `Design`: stretch along a rift axis, lobed outline, flank cones (parasitic
  cones, rift-zone cones and pit craters, a massif's overlapping cones, a cone field's cinder
  cones, domes and maars, some breached), a landslide amphitheatre (a scarp on shields,
  likelier with age), radial gullies deepening with age, ribs on young cones, a lobed
  outline, a concavity of its own (`Design::steepness`), craters (`Pit`: the summit crater a
  little off centre, often a few smaller ones of various sizes, walls sheer or gentle; old
  stratovolcanoes: a caldera) or a big collapsed caldera (it becomes a lake).
  `Design::shape` gives the rise and what craters dig (below the ground for calderas and
  maars): on land on the ground, at sea from the floor to a summit above the water that
  sinks with age (`SUBSIDENCE`: worn islands, then seamounts); cells raised above the sea
  become land. Volcanic cells are basalt; active ones (`ACTIVE_AGE`) have lava only as a small
  lava lake in the summit crater (`LAVA_LAKE`; none in collapsed calderas) (`Planet::lava`,
  the rock field's blue: drawn bare and black; no vegetation); older slopes are more rugged.
  Black sand on basalt coasts. `Planet::volcanoes`, `PlanetSurface::volcanoes` (the inspector
  reports them).
- **River banks** (`PlanetSurface::riparian`, from the river points, wider along big
  rivers): greener ground and grass in dry country (`mesh::RIPARIAN_GREEN`), and
  `Biome::{Riparian, Oasis}` (gallery forests; palms where hot) beyond `RIPARIAN_BIOME`.
- **Barrier islands** (`barriers.rs`, right after the reefs): on low coasts (land below
  `LOW_COAST` near the shore) with a shallow shelf, between `LATITUDE`s, in noise stretches: a
  sand bar `BAR_DISTANCE` offshore (flat top, gentle `SHOREFACE`, steeper `BACK`), broken by
  inlets, a shallow lagoon behind; their rock is the mainland's (not the sea floor's basalt).
  The terrain shader keeps beaches sand on their steep seaward slopes.
- **Geothermal fields** (`volcanoes::Built::geothermal`, `Planet::geothermal`): patches on
  the flanks of volcanoes younger than `GEOTHERMAL_AGE`, only on gentle ground
  (`GEOTHERMAL_SLOPE`); drawn as sinter, microbial mats and hot pools (sent per vertex: UV1.y
  negative, sharing the coast's channel), bare of plants.
- **Reefs** (`reefs.rs`, right after the volcanoes): in the tropics (`TROPICS`), fringing reefs
  a little offshore (`FRINGE` cells: rim just under the water, shallow lagoon behind), further
  out round volcanic islands from `BARRIER_AGE` (barrier reefs), atolls on sunk shield
  volcanoes from `ATOLL_AGE` (a ring at `ATOLL_RING` of the old radius, flat lagoon inside,
  islets where it breaks the surface). Hotspots are mostly oceanic (`OCEANIC_HOTSPOTS`) and
  every chain ages from new to sunk along its length, so tropical chains end in atolls.
  `Planet::reef` (the rock field's alpha): coral under the water, coral sand on the islets
  (`planet_terrain.wesl`); `Biome::TropicalBeach` (tropical beaches, reef islets) has palms.
  Beaches use a narrow coast measure (`mesh::COAST` on the blurred ocean): small islands are
  land, not all sand; the sea floor colour only along the ocean.
- **Erosion engine** (`terrain.rs::erode_timed`, from a `Shape` of per-cell uplift,
  erodibility, diffusion, rain, provinces, ranges, subsidence, glacial noise, rock and strata:
  the rock's bed at each cell's height scales its erodibility and diffusion):
     implicitly from the outlets upstream (Braun & Willett 2013), plus hillslope diffusion.
     Each step routes through the depressions (`FlowGraph::with_depressions`, below) with
     **stochastic D8** (random slope weights against grid-aligned valleys), then accumulates
     the drainage. The sea is base level, fixed at 0 during erosion. Afterwards:
- **Planet** (milestone 1 of `docs/living-world.md`; the game since 2026-10-20):
  - `grid.rs`: the `Grid` trait (cells, neighbours with distances in cells, border, blur).
    `flow.rs` (depression filling, `FlowGraph`) and `terrain::erode_timed` are generic over
    it. Erosion routes with `FlowGraph::with_depressions` (Cordonnier et
    al. 2019 basin graph: steepest descent, pits' basins linked through their lowest passes,
    minimum spanning tree to the outlets, pits carved out over their pass; also returns the
    water surface, equal to Priority-Flood without epsilon), not a Priority-Flood per step.
    `erode_timed` reports the time of each part.
  - `sphere.rs`: `SphereGrid`, an equiangular cube-sphere of `6 n²` cells, indexed
    `(face * n + z) * n + x`; `direction(i)`, `cell_at(direction)`, `face_coords`, `area(i)`
    (relative to the mean). Neighbours across face edges are found by direction (at cube
    corners a cell has 7). Tests: round trip, areas, mutual neighbours, everything drains.
  - `noise.rs`: 3D noise (`get3`, `fbm3`, `ridged3`, `warp3`) to sample the sphere by direction.
  - `planet/`: `generate_planet(&PlanetSettings) -> Planet` (module docs explain it; the stages
    in `shape.rs`, `coast.rs`, `water.rs`):
    rigid plates (Euler-pole rotations), each continental or oceanic; a crust value = continent
    noise + the plates' bias (blended across boundaries, `CRUST_BLEND`), cut at the level
    leaving `land_fraction` above, with shelves just below (`SHELF_BAND`). Boundaries act by
    crust on each side: continent–continent collision → wide ranges, ocean under continent →
    coastal cordillera + trench, ocean–ocean → island arc (volcanoes raise the crust), rifts in
    continents, mid-ocean ridges at sea. Old sutures between ancient blocks (`cratons`) are worn
    ranges, lower and more diffusive the older. Provinces (plains / broad rounded hills /
    plateaus, about `province_cells` each) blended softly; near the sea, plateaus give way to
    lowlands and interior uplift ramps down. Then `terrain::erode`, in two stages: the first
    `coarse_steps` (45 of 60) on a grid `coarse_factor` (2) times coarser (shaped the same way:
    `shape_on(grid, scale)` keeps every length in full-size cells; `ErosionSettings::cell_size`
    tells erosion how long a cell is), then upsampled (`upsample`) and refined at full
    resolution (`--coarse 0` in the preview: all at full resolution). Then scaling, `cap_plateaus`, a
    sea floor of shelf / slope / abyss (raised at ridges, deepened at trenches). Both shaping
    passes run on all cores (`par::map`); provinces and blocks are found through a coarse
    `PointIndex`. Landforms are sized in cells (radius `2n/π` cells; n = 1024 is the target
    planet, 256 the fast default for previews).
  - `tectonics.rs` (milestone 4, v1): `run(&TectonicSettings) -> Tectonics`, a deep-time
    history (default 50 steps × 4 Myr) on a coarse cube-sphere (n = 96). Each plate keeps its
    crust in its own frame (a raster turning with it: no resampling drift); each step every
    world cell asks every plate what covers it; overlaps where plates converge (relative
    motion across the boundary) resolve by crust (continental floats; between oceans the
    younger wins): oceanic losers are consumed, continents stack (collision: thickening +
    uplift), cordilleras over subduction, island arcs (accreting into continental crust);
    uncovered cells are new oceanic crust (ridges). Rifts split the plate with the most
    continent along a great circle; continents colliding for `WELD_STEPS` steps weld (same
    motion; collisions are kept in ordered maps so welds happen in the same order on every
    run: generation must stay deterministic). Outputs crust, plate, oceanic age, uplift and its age. Previews:
    `tectonics_preview` (frames over the history), `tectonics_stats`.
    `PlanetSettings::history` (the default; preview `--no-history` for the noise version) shapes the planet from it: continents from
    the crust (noise details the coasts), young uplift → belts / foothills / island arcs, old
    uplift → worn ranges, young ocean floor → ridges; ridges run along belts (`across` from the
    belt strength's contours).
  - `world.rs` (`docs/evolution.md` E1a): `World` (in `PlanetSettings::world`) holds the
    world's parameters; `World::climate()` gives the `ClimateSettings` (sunlight from the star
    and its age with Gough's brightening, greenhouse from CO₂ and CH₄, haze and dust albedo,
    orbit, day length, ground albedos). Presets `earth` (default), `virgin`, `oxidised`;
    `World::parse` takes `virgin,day=240,...` (`climate_preview --world`, the game's
    `UF_WORLD`). `life: false` (E1b): no reefs, limestone (→ sandstone, shale), riparian
    green, vegetation or grass blades (`PlanetSurface::life`), more ergs; the terrain shader's
    `world` uniform (life, `World::oxidation()`) draws bare regolith, mudflats, no hot-spring
    mats, and greys the ochres without oxygen; the water's `planet.w` is the sea's iron
    (green seas); sunlight takes `Star::color()`; salinity moves the sea ice's freezing point.
  - `climate.rs` (milestone 3, v1): `simulate(&Planet, &ClimateSettings) ->
    PlanetClimate`, (orbit: Kepler with eccentricity and perihelion, year length; day length
    sets the circulation cells per hemisphere, Coriolis turning and heat transport; albedo
    from sea, rock or plants, deserts (a second energy-balance pass once the rain is known),
    ice, air) twelve months per cell of a coarse cube-sphere (n = 48), as if the planet
    were Earth-sized: energy balance per cell (insolation by latitude and tilt, `A + B T`,
    ice albedo, ocean/land heat capacity, implicit diffusive transport weaker over land,
    rule-based ocean current anomalies), winds from pressure belts around the ITCZ (plus warm
    land lows) turned by Coriolis; mountains block the wind blowing up them (turning part of
    it along the slope), high ground is windier, land friction slows the surface wind (not the
    wind aloft, `ALOFT` × faster, that carries the moisture). `WIND_SCALE` / `POLAR_WIND` are
    calibrated on Earth's mean surface winds (trades ~8 m/s, westerlies ~9, polar easterlies
    ~5 over the sea); `PlanetClimate::prevailing` (the year's mean wind) and `strength` (mean
    speed with storm winds, spread `STORMINESS` × the north–south temperature gradient: the
    westerlies' storm track is strong but unsteady), sampled with `WindField::at`;
    `PlanetSurface::wind_at(face, p)` adds the local terrain (exposure against a ring of
    heights, turning along steep slopes). Moisture evaporated, advected semi-Lagrangian and rained out
    by terrain uplift, convergence and saturation (sinking air dry). `Koppen::classify`, and
    `PlanetClimate::koppen_at(direction, height)` (bilinear months, lapse rate with
    `meters_per_unit` = 100). Tuned on the zonal means printed by `climate_preview`
    (`--example climate_preview -- <out_dir> [--n] [seeds]`, also Köppen / rain / January and
    July temperature maps). In the game the ground colour is the cell's Köppen
    `ground_color` (`PlanetSurface::ground`), sand at the water's edge.
  - Preview: `cargo run -p worldgen --release --example planet_preview -- <out_dir> [--n
    <cells>] [seeds…]` writes `<seed>_map.png` (equirectangular relief with rivers),
    `<seed>_globes.png` (two orthographic views) and `<seed>_plates.png` (continental plates
    bright, oceanic dark), with lakes. Whole world at n = 1024 (planet, climate, surface, sea
    field): ~23 s on 20 threads; `cargo run -p worldgen --release --example timings [-- <n>]`
    prints each stage's time and how many cores it kept busy. All parallel work goes through
    `par.rs` (`map`, `for_each_mut`, `for_each_group`, `Shared` for disjoint writes) on
    worldgen's own `bevy_tasks` pool (persistent threads, chunks handed out dynamically;
    results identical to sequential): no other threads, no rayon. `priority_flood` is computed
    from the depression routing's water surface plus a parallel epsilon relaxation (checked
    against the classic queue version in the tests); `FlowGraph::trees` lets accumulation and
    erosion's incision walk the independent trees on all cores.
    In-face neighbour distances are precomputed per face position (`local_distance`).
- **The game** (`cargo run --release -- [--n <cells>] [--seed <n>]`, milestone 2,
  work in progress; `src/planet/`): `PlanetPlugin` replaces `MapPlugin` (the flat map's
  systems in `camera.rs` / `dev_tools.rs` run only if `Map` exists). It generates the planet
  in the background, keeps a `worldgen::surface::PlanetSurface` (per-face fields padded with
  the neighbouring faces' cells, bicubic in face cell coordinates; `position(face, p)` on the
  sphere; heights get fine detail as on the flat map: 3D-noise roughness and `detail::gullies`,
  scaled by `Planet::ruggedness`, faded by `Planet::calm` near rivers and lakes, and blended
  with the neighbouring face's gullies within `EDGE_BLEND` cells of a face edge), and spawns
  `PlanetChunk`s (64 × 64 cells of a face, meshed relative to their sea
  level point) meshed in background tasks (≤ 8) with a quadtree LOD (`planet/mesh.rs`:
  patches of 16² quads, cell ≤ 0.004 × distance, ≥ 0.5 cell; skirts toward the centre;
  drawn with `PlanetTerrainMaterial`, `planet_terrain.wesl`: the flat map's toon ground generated
  from `map_material.wesl` with face cell coordinates (UV0) for its patterns, radial heights and
  up, and ground materials from the climate per vertex (see Lakes below for the channels),
  see `Koppen::ground_weights`), re-meshed when the camera moved by 25% of their distance. The
  Rivers (`worldgen::rivers`: traced on the sphere as on the flat map, smoothed,
  resampled every 0.5 cells, meandering on flat valleys; width `RIVER_WIDTH.0` × (area /
  `RIVER_MIN_AREA`)^`RIVER_WIDTH.1`, some 20× from the smallest to the largest; tributaries end
  on their parent's final path `JOIN_LEAD` points downstream of the nearest, the move eased
  over `JOIN_BLEND` points (one running alongside its parent ends where it first touches
  it); bends tighter than `MIN_BEND_RADIUS` widths are relaxed (`relax_bends`, as on the flat
  map: meander offsets fold over on tight valley bends); channels are carved at least
  `MIN_CARVED_WIDTH` wide (`surface/channels.rs`: narrower ones showed as holes in the mesh); rivers run through the lakes they enter or leave, to or from the
  deepest cell at the lake's level, as on the flat map: `through_lakes`; rivers split on
  flat land: `split_on_flats` gives wide rivers on flat stretches without tributaries a side
  channel (`PlanetRiver::branch_of`, ending in `Junction` of the same river) round a low
  island, capped on the river's edge at both ends by `planet/water.rs` (its rows backwards
  for the start); rivers left with one point after the coast cut are kept, so indices hold) get their water levels clamped
  under the surface across the channel and end where they reach the sea (`PlanetSurface::new`).
  Lakes (`lakes.rs::shape_lakes`, after the coasts, before the rivers): sea patches
  cut off from the ocean (under `SEA_MIN_SHARE` of the cells) become land below sea level, a
  dry basin (its lowest cell a sink in the routing base) while under `LOWLAND_MAX_INFLOW`
  drains in; every depression is a lake only from `LAKE_MIN_CELLS`, `LAKE_MIN_DEPTH` and
  `LAKE_AREA_PER_INFLOW` × its inflow (big rivers end only in very large lakes or the sea),
  else filled flat (wet, a closed flat). Kept lakes get their own basin: the blurred mask
  rounds the outline, the bed deepens from the shore (`SHORE_DEPTH` + `DEPTH_SLOPE` × cells,
  at most ∝ √cells), banks rise just above the level (not across streams, nor on the outlet's
  path); twice, then a last fill of leftover hollows. Lakes over their cap (`LAKE_MAX_CELLS`,
  a range: each lake its own) first drain through a gorge cut at the level where they fit:
  across the old bed by a least-cost path (low ground, a winding noise: `GORGE_*`) to the
  outlet, then down it until lower ground (exposed beds: wet ground). Outlines are rounded by
  `LAKE_ROUNDING` blur passes. Salt-flat potential (`basins`) is only the dry basins (no
  outlet); filled hollows and drained beds are wetland.
  After the floodplains and mouths reshape the ground, `lakes::settle` fills the new
  hollows outside lakes and recomputes the drainage and lake depths (they were stale: lake
  levels sloped where a mouth's sediment plain had lowered a lake). `Planet::wetland` (closed flats, dry
  basins, lake shores, floodplains, low flat ground near streams, delta land) and
  `Planet::basins`; `PlanetSurface::marsh` is marsh where the climate is wet (not frozen),
  negative (salt flats) on closed flats where it is dry. `PlanetSurface::ocean` (the sea mask,
  blurred) tells the ocean from land below sea level: rivers end, beaches form and vegetation
  stops only along it; the sea's wave field has depth -1 over basins not open to the sea and
  the water shader discards there. Terrain vertices: colour = (dryness, forest, marsh / -salt,
  wet bed), UV1 = (cold + snow, coast); the shader takes deserts from dryness.
  Floodplains (`planet/water.rs::flatten_floodplains`): `rivers::extract`
  also returns the un-meandered valley points (level, floodplain half-width where the river
  meanders); the cells around are lowered to water + 0.4 (lake shores kept above their lake),
  walking the grid with `rivers::for_cells_within`, and calmed (no fine detail).
  Springs (`springs.rs`, the flat map's `map::springs` in the plane tangent at each
  source: x downstream, y across): river sources off lakes and other rivers get a pond (flat
  ground) or a hillside notch, carved after the channels in `PlanetSurface::height` (bucketed
  in 3D; ponds count as wet). `planet/water.rs` draws the ponds (rivers fade in from the rim)
  and hillside spouts (a raised row in the notch, whitewater below); `planet/vegetation.rs`
  puts the flat map's rock outcrop round each spout.
  Mouths (`mouths.rs`, the flat map's `mouths.rs` in the plane tangent at each mouth,
  cells walked on the sphere's grid; deltas rarer than estuaries; distributaries never cross
  land above `DELTA_MAX_GROUND` (forks left without branches go too), grow toward the sea mask
  (not land below sea level), fan at most `MAX_TURN` (tighter than the flat map's: sideways
  channels cut across to the next bay; `grow_channels` takes the limit), channels mostly over
  old land are dropped (`MAX_OVERLAND`), delta land sinks to a bar toward the tips (`TIP_TRIM`), the sea
  floor round it is raised into a sloping front (`FRONT_*`) and the whole smoothed, and each
  tip gets a path to the open sea if land closes it in; the new land keeps above the sea, and
  a corridor of land runs along every channel but its tip; a fork's branches start
  `FORK_GAP` past its end, which the dropping rules match by): deltas (distributaries from `channels::grow_channels`,
  low land raised along them, `sea` updated) and estuaries, with a sediment plain around;
  `PlanetSurface` carves their channels (parabolic channel + banks, segments bucketed in 3D).
  `planet/water.rs` draws the water with the flat map's toon water on the sphere
  (`PlanetWaterMaterial`, `planet_water.wesl`, generated from `water.wesl`: pattern
  coordinates from vertex colour red/green, radial heights and up, no sea swell yet). Rivers
  are unrolled into (along, level) points and fed to the flat map's `river_profile` (falls,
  domes), then rebuilt as ribbons on the sphere (rows with forward/across directions); flow in
  UV0, (whitewater, opacity) in UV1. As on the flat map they fade out where both banks reach
  the sea (or go under their lake), carrying on straight if needed, fade in out of a lake, and
  tributaries end in a junction cap on their parent's ribbon edge (crossings found in the
  plane tangent at the junction; the cap uses the tributary's pattern coordinates), with
  whitewater on the parent below; the few that never cross it fade out. Lakes are outlined per
  face as on the flat map (BFS over the cell samples from the lakes: lower ground, up to
  `SHORE_STEPS` onto the shore, just into river channels; marching squares on
  level + `SHORE_MARGIN` - ground, clipped to the face; samples one past the edges); the sea is a per-face grid at sea level, subdivided so it sags less than
  `SEA_SAG`. Each face of the sea has its wave field (`worldgen::sea::sea_field`, the
  flat map's `map::sea` on the sphere's grid: Dijkstra travel time from the shore, stopped at
  `MAX_TRAVEL`, depth, exposure from the local wind's direction and strength, wind speed; a
  `Rgba16Float` texture per face, a texel per cell, `PlanetWaterShader::sea`), and the shader's
  swell (`sea_waves`) runs on it as on the flat map. Lakes and the sea carry the local wind in
  UV0 (direction × m/s, pattern coordinates): ripples drift with it and grow with its speed.
  `planet/vegetation.rs` places the flat map's trees, rocks and ground cover
  (`planet/nature.rs`: the flat map's species, rules, models and LODs, through
  `VegetationModelsPlugin`; instances carry a full rotation, upright on the sphere) on each
  terrain chunk's face cells: large things within `PLACE_DISTANCE` (merged LOD2, dropped past
  `DROP_DISTANCE`), ground cover when a chunk comes near. Biomes come from the climate's ground
  weights (forest weight through a groves noise, conifers where cold), beaches below
  `BEACH_HEIGHT`, rock on steep cells. Swaying models bend along the wind projected across
  their own up axis (`toon_sway.wesl`: a fixed world direction made tangent on the planet).
  `planet/inspect.rs`: clicking the ground logs a report (position, heights, water, climate,
  wind, nearest rivers and springs, the view as `UF_PLANET_CAMERA`, which now takes a 5th
  value, the heading); "Report issue" (button or F6), a click and a description (Enter) write
  `issues/issue-<n>.md` + `.png` (git-ignored: read them when the user points to issues; when
  the user files new ones, move the previous batch to `issues/fixed/`, whose numbers are not
  reused).
  `UF_PLANET_REPORT=lon,lat[;...]` logs the report of points at startup (for agents).
  `planet/grass.rs` builds the flat map's near grass blades on 1×1 tiles of face cells where
  the terrain draws grass (from the climate weights); each tile is turned upright with local x
  downwind (`PlanetSurface::wind_at`): `grass_blades.wesl` bends in the mesh's frame and takes
  its gust pattern from UV1 (along / across the wind in face coordinates) when present. Coasts are shaped as on
  the flat map (`planet/coast.rs`: blurred signed distance to the shore, land ramp, shelf). The camera (`planet/camera.rs`, `PlanetCamera`, replacing the
  pan-orbit controller) looks at a focus point on the surface from a distance, with a
  tangent forward direction parallel-transported as the focus moves (no flip at the poles);
  left or middle drag / arrows move the focus, Q / E turn (physical keys: A / E on AZERTY), right
  drag orbits and tilts, wheel zooms; far out it looks
  straight down; it stays above the ground and fits its clip planes to the view. No
  atmosphere or fog. The sun follows the view (low afternoon sun over the focus, F turns
  it) until the planet has a day cycle. `UF_PLANET_CAMERA=lon,lat[,distance[,pitch°[,heading°]]]`
  sets the starting view (screenshots; clicks log the current one).
- `planet::start_generation` runs the planet, its climate, surface and sea field on the
  `AsyncComputeTaskPool`; `finish_generation` polls the task with `check_ready`, inserts
  `PlanetWorld` and enters `AppState::InGame`.

### 3.3 Buildings (retired)
Removed on 2026-10-13 with the city builder (placement, zones, tools, selection, build grid).

### 3.4 Rendering (`shaders.rs`, `assets/shaders`)
All materials are `ExtendedMaterial<StandardMaterial, _>` with WESL shaders. The planet's
terrain shader (`planet_terrain.wesl`, `PlanetTerrainShader`) was generated from the flat map's
`map_material.wesl` (in git history): the same ground painting and cel lighting, with face
cell coordinates for the patterns, radial heights and up, and ground materials per vertex
(see the planet notes above):
- **Art direction: toon / cel-shaded, not realistic.** Ground materials are flat painted
  colours with crisp patches from world-space value noise (fBm with octaves fading below a
  pixel): grass (lush → dry in steps, lighter patches, dirt spots), alpine grass, forest
  floors, marsh (dark pools), sand, rock (two-tone strata on cliffs), snow, river/lake beds,
  sea floor (sand → teal, darker with depth). Edges are antialiased with `fwidth` (`crisp`).
  - **Grass carpet** (no geometry, `grass_carpet`): the view ray is marched (`GRASS_STEPS`)
    through a thin layer (`BLADE_HEIGHT`) whose blades are peaks of value noise
    (`BLADE_SCALE` per unit, narrow peaks, taller in `clumps`), tips bent downwind; bases are shaded, tips
    lighter, some tips are flowers in lush meadows. Fixed world size (perspective); it fades
    to its average shade only when blades are well under a pixel apart (`BLADE_FADE`, TAA
    turns them into a grain). Very close to the camera, real blades (`planet/grass.rs`: a mesh
    of blade triangles per 1×1 tile around the point looked at, where the biome has grass,
    `GrassBladeMaterial` with `grass_blades.wesl` bending them with the same wind, gusts and
    sway as the carpet) stand in front of it and fade out with distance. Grass flattens near
    water and on edited ground (`TerrainHeights::grass_clearance`, mesh vertex colour green). Gusts sweep downwind as lighter
    bands at every distance. The wind is the world's prevailing wind (`wind`, binding 108,
    set with the textures). Forest floors get a weaker carpet.
  - **Snow** settles on flat ground (steep rock stays bare), with a ragged noisy edge and
    wind-blown drifts. **Desert** (`ground_c.r`): dune ripples, cracked clay flats, redder
    rock. **Marsh**: reed beds (wind strokes) around still teal pools with a muddy rim.
- **Water** (`planet_water.wesl`, `PlanetWaterShader`): crisp depth bands (`BANDS`; sea depth from the
  sea field, else the depth prepass). Sea: swell crests on isolines of the travel time
  (`WAVE_LENGTH`, `WAVE_PERIOD`), arriving obliquely with the wind, in segments; breakers
  with lacy foam in the surf zone (`BREAK_DEPTH` scaled by exposure, `SURF_ZONE`); a swash
  foam line pulsing at the shore; wind ripple lines offshore and on lakes. Rivers: a two-phase
  flow map (`FLOW_PERIOD`): world-space noise pushed along the velocity, smeared along the
  flow into streaks (no rotated coordinates: they swing wildly far from the origin), so the
  pattern is continuous where rivers meet; whitewater patches, and falls (steep faces from the screen-space normal) with falling streaks.
- **Lighting** is custom (no PBR/specular): the sun (`lights.directional_lights[0]`, its
  brightness × `view.exposure`) gives a hard two-tone lit / cool-shadow step from the slope
  and `fetch_directional_shadow`, plus a highlight band and a rim light; fog etc. via
  `main_pass_post_lighting_processing`.
- Tune colours in the shader (they are sRGB constants through `srgb()`); `tint` multiplies
  everything.

### 3.5 Simulation
The Rhai city simulation was retired with the city builder. The planet simulation is
designed in `docs/living-world.md`.

### 3.6 UI (`ui.rs`)
Only the loading text remains ("Generating world...", a `bsn!` scene). `label(text)` is the
shared text scene; the font is an asset path in the scene (`UI_FONT`), and scenes with assets
are spawned with `queue_spawn_scene`, which waits for the font. `PointerOverUi` (a
`SystemParam`) tells the camera when the cursor is over UI.

### 3.7 Known issues / smells (fix opportunistically)
- `bevy_dev_tools` was enabled for the (retired) build grid; check whether anything else
  still needs it. Bevy says not to ship dev tools.
- Tests exist only for `worldgen`.
- Planet generation takes ~30 s at n = 1024 (the lake shaping floods the whole planet
  several times; the climate ~9 s).

---

## 4. Bevy upgrades (0.16 → 0.20 done on 2026-10-09)

Bevy releases: 0.17 (2025-09-30), 0.18 (2026-01-13), 0.19 (2026-06-19), **0.20 (2026-10-08,
latest)**. Migration guides: `https://bevy.org/learn/migration-guides/0-16-to-0-17/`, then
`0-17-to-0-18`, `0-18-to-0-19` and `0-19-to-0-20`.

**Status:** done on branch `bevy-upgrade`, with one commit per version (0.17, 0.18, 0.19, 0.20).
Each step was checked by compiling and running the game for ~60 s with the logs scanned for
errors. For future upgrades, keep the same approach: **one minor version at a time**, and
commit each step.

Gotchas hit during the upgrade:
- The then git-ignored local `Cargo.lock` held stale versions that broke builds (`tracy-client-sys`,
  `gpu-allocator`). Fix them with a targeted `cargo update -p <crate>`. Since then all
  dependencies are pinned and bumped (rand 0.10, rhai 1.26, ...).
- 0.17: `TemporalAntiAliasPlugin` is in `DefaultPlugins`, so adding it again panics.
- 0.18: the WGSL `apply_decal_base_color` was removed (clustered decals now use `apply_decals`).
- 0.20: system errors panic through the fallback handler, so `query.single()?` on a
  possibly-empty query (e.g. a `Changed<..>` filter) crashes. Use `let Ok(..) = .. else`.

### 4.1 Toolchain & dependencies (all deps may be changed)
- Bevy 0.20 needs **rustc ≥ 1.97.1**. The toolchain is pinned to `nightly-2026-10-08` (the
  user prefers nightly). Bump the date deliberately. A recent-nightly lint required
  `#![recursion_limit = "256"]` in `main.rs` for the `AsBindGroup` derive.
- `noiz` was used for noise until the world generation rework. It was dropped, together with
  the local Bevy 0.20 port it needed (`../noiz-bevy-0.20`, safe to delete). `worldgen` has its
  own small noise module.
- `kdtree-collisions` (the user's own crate) and `rhai` were dropped with the city builder.
- Latest versions (2026-10-09): `ron 0.12` (Bevy no longer re-exports ron),
  `rand 0.10` (`Rng`→`RngExt`, `RngCore`→`Rng`), `rand_distr 0.6`, `foldhash 0.2`,
  `fast_hilbert 2.1`, `thiserror 2` (Bevy examples use it for loader errors).
- `trace_tracy` is now the opt-in `tracy` feature. Still possible: put `file_watcher` and
  `bevy_remote` in a `dev` feature, and use `bevy/dynamic_linking` for dev builds only.

### 4.2 Changes that hit this codebase (cumulative, 0.16 → 0.20)

**ECS / events**
- Buffered events → **messages**: `#[derive(Message)]`, `MessageReader` / `MessageWriter`
  (`.write()`), `Messages<T>`, `add_message`. `Event` is now only for observers.
  `EventReader<AssetEvent<_>>` becomes `MessageReader<AssetEvent<_>>`, and
  `EventReader<MouseWheel>` becomes `MessageReader<MouseWheel>`.
- Observers: `Trigger<OnAdd, T>` → (0.17) `On<Add, T>` → (0.20) **`On<Add<T>>`**.
  `OnRemove` becomes `Remove<T>`. 0.19 renamed `Replace` to `Discard`.
  `trigger.target()` is gone: read the entity from the event (`add.entity`, or
  `EntityEvent::event_target()`). Entity-targeted custom events `#[derive(EntityEvent)]` and
  carry an `entity` field, triggered with `commands.trigger(MyEvent { entity })`.
- 0.17: `Option<Single<..>>` is `None` when **several** entities match (it used to skip the
  system). This matters for `SelectedBuild` / `Highlighted`.
- System sets renamed with a `Systems` suffix (`TransformSystems`, `UiSystems`, ...).
  `Condition` → `SystemCondition`. `StateScoped` → `DespawnOnExit`.
- 0.18: hierarchy `remove_children` → `detach_children`, etc. 0.18: a failed system inside a
  combinator counts as `false`.
- 0.19: **resources are stored as components** on singleton entities. Don't derive both
  `Component` and `Resource` on one type. Broad queries (`Query<Entity>`, `Query<EntityRef>`)
  may need `Without<IsResource>`. `NextState::set_if_neq` → `set_if_different` (0.20).
- 0.20: `Name: From<&'static str>` only, so use `Name::new(s.to_owned())` for owned strings.
  The `iter_many*` iterators yield `Result`. `chain_weak()` is available.

**Rendering / modules** (`bevy_render` was split in 0.17)
- Paths: camera types → `bevy::camera` (`Exposure`, `Projection`, `primitives::Aabb`,
  `visibility::*`). Lights → `bevy::light`. `Mesh`/`Indices` → `bevy::mesh`.
  `RenderAssetUsages` → `bevy::asset`. `ShaderRef` → `bevy::shader`.
  `Bloom` → `bevy::post_process::bloom`. TAA → `bevy::anti_alias::taa`.
  `AlphaMode` / `OpaqueRendererMethod` → `bevy::material` (0.19).
- `Camera { hdr: true }` → `Hdr` component (`Bloom` and `Atmosphere` require it anyway).
- `Atmosphere`: 0.18 replaced the inline params with `Handle<ScatteringMedium>`. In 0.19 it
  is **its own entity** (`commands.spawn(Atmosphere::earth(mediums.add(ScatteringMedium::earth(256, 256))))`),
  lives in `bevy::light::Atmosphere`, and `AtmosphereSettings` stays on the camera.
  `bottom_radius`/`top_radius` → `inner_radius`/`outer_radius`.
- `AmbientLight` resource → `GlobalAmbientLight`. `AmbientLight` as a camera component
  overrides it (0.18). The current code already uses it on the camera.
- Light field `shadows_enabled` → `shadow_maps_enabled` (0.19).
- `Gizmos::cuboid` → `Gizmos::cube` (0.18).
- Mesh: smooth normals are now angle-weighted (0.17). 0.18 adds `try_*` accessors
  (`try_attribute_mut`, ...) because RENDER_WORLD-only meshes stay in `Assets<Mesh>`.
  `Assets::get_mut` returns `AssetMut<A>` (0.19). AABBs recompute automatically on mesh
  change (0.18).
- `SceneRoot` / `Scene` → **`WorldAssetRoot` / `WorldAsset`** (0.19, `bevy::world_serialization`).
  glTF scenes: `WorldAssetRoot(asset_server.load(GltfAssetLabel::Scene(0).from_asset(path)))`.
- Material bind group: `@group(2)` → `@group(#{MATERIAL_BIND_GROUP})` (0.17) →
  `@group(constants::MATERIAL_BIND_GROUP)` (WESL, 0.20).
- **0.20: custom shaders must be WESL** (`.wesl`). `#import a::b` → `import a::b;`,
  `#ifdef X` → `@if(X)`, and `#define_import_path` was removed. Module paths moved, e.g.
  `bevy_pbr::render::pbr_fragment`, `bevy_pbr::prepass::io`, `bevy_pbr::deferred::functions`.
  `alpha_discard(material.flags, material.alpha_cutoff, color)`. Use `@if(X) { .. } @else { .. }`
  for conditional blocks. `map_material.wesl` and Bevy's `crates/bevy_pbr/src/render/pbr.wesl`
  are good references.
- `MaterialPlugin { prepass_enabled, shadows_enabled }` → `MaterialExtension::enable_prepass()`
  / `enable_shadows()` (0.18).

**Assets**
- `AssetLoader` impls need `#[derive(TypePath)]` (0.18). `LoadContext::path()` returns
  `AssetPath`. `LoadContext::loader()` → `load_builder()` (0.19).
- Prefer a `thiserror` error enum over `anyhow::Error` in loaders (Bevy example style).

**UI / text / picking / input**
- `BorderColor(c)` → `BorderColor::all(c)` (0.17). `ScrollPosition` is a `Vec2` newtype.
  `BorderRadius` is a `Node` field (0.18).
- `TextFont.font` is a `FontSource` (a `Handle` or a family). `font_size` is `FontSize::Px(..)`,
  and the default is `FontSize::Rem(1.)` (0.19). `Val::Em`/`Rem` and helpers `px()`,
  `percent()`, `em()`, `rem()` exist.
- **`Interaction` is deprecated (0.20)**. Use `ui_widgets::Button` + observers
  (`On<PointerPress>` / `Activate`), and `Hovered` / `Pressed` for styling. `UiWidgetsPlugins`
  is part of `DefaultPlugins`.
- Picking events are flat in 0.20: `Pointer<Press>` → `PointerPress`, and
  `press.pointer.position`.
- Feature flags: `ui` is no longer implied by `3d` (0.19). Picking backends are named
  `mesh_picking`, `ui_picking`. Consider the feature collections (`3d`, `ui`, `default_app`).

**Built-ins that can replace custom code**
- `bevy::camera_controller::pan_orbit_camera` (0.20; `PanOrbitCamera` + `MeshPickingPlugin` +
  `DefaultPanOrbitCameraPlugins`) could replace the hand-rolled orbit camera. A city builder
  still needs terrain-following, keyboard pan and clamping, so evaluate before switching.
- `InfiniteGridPlugin` (0.19) for a build grid overlay. `TransformGizmoPlugin` for editor tools.
- `RenderDebugOverlay` (0.19) is in default plugins but disabled; its default keys are
  **F1/F2, which collide with our F2 AABB toggle**.
- `commands.delayed()` (0.19), `SettingsPlugin` + `SettingsGroup` for persisted settings.

---

## 5. Bevy coding guidelines for this repo

These follow the official examples (0.20), the migration guides, and the community
`bevy_new_2d` template's conventions. Apply them when touching code. Bulk restyling is part
of the migration work.

### 5.1 Project structure
- **One plugin per feature module.** Use `pub(super) fn plugin(app: &mut App)` (bevy_new_2d
  style) or `struct FooPlugin; impl Plugin`. The parent module aggregates them with
  `app.add_plugins((a::plugin, b::plugin))`. `main.rs` only builds the `App` and adds the
  top-level plugins.
- Keep a feature's components, resources, messages and systems in its own module. Expose
  only what other modules need (`pub(crate)`).
- Suggested target layout:
  ```
  src/main.rs            App::new + top-level plugins only
  src/camera.rs          orbit/pan camera, CameraSettings
  src/environment.rs     sun, atmosphere, fog, ambient
  src/dev_tools.rs       F-key debug toggles, gizmos (rivers), wireframe, BRP — compiled only with `dev` feature
  src/terrain/{mod, chunk, material, streaming}.rs
  src/terrain/gen/{noise, hydrology, rivers}.rs   (pure Rust, unit-tested, no ECS)
  src/ui/{mod, widgets}.rs
  src/states.rs          AppState / Screen states
  ```
- Keep **pure algorithms** (world generation, sim maths) free of ECS types so they can be
  tested and run on background threads. Bevy math types (`Vec2`, curves) are fine.

### 5.2 Components, bundles, spawning
- Use **required components** (`#[require(Transform, Visibility)]`) instead of bundles. Never
  write new `Bundle` structs.
- Spawn with tuples. Reusable entity templates are functions: on 0.18 and earlier, functions
  returning `impl Bundle`; on **0.19+, prefer `bsn!` scenes** (`fn x() -> impl Scene`), which
  are especially good for UI. Use `children![...]` (≤0.18) or `Children [ ... -- ... ]` in BSN
  for hierarchies, instead of `with_children` closures.
- Use **marker components** (`struct Sun;`) for querying. Use `Name::new(..)` on important
  entities for the BRP / inspector.
- Use newtype components with `#[derive(Component, Deref, DerefMut)]` for single values.
- Don't encode state machines by component presence or absence (e.g. "has `BuildId` but no
  `Transform`"). Use explicit components, `States`, or an observer on an event such as
  `StartPlacement(Handle<Building>)`.

### 5.3 Systems & scheduling
- Name systems as verb phrases (`spawn_chunks`, `update_scoreboard`). Keep them small and focused.
- Order deliberately: `.chain()` for real ordering, `.chain_weak()` when only data
  conflicts matter (0.20). For cross-module ordering, define an app-level `SystemSet` enum
  (bevy_new_2d's `AppSystems { TickTimers, RecordInput, Update }`) and
  `configure_sets(Update, (..).chain())`.
- Use **run conditions** instead of early returns: `.run_if(input_just_pressed(KeyCode::F3))`,
  `.run_if(in_state(AppState::InGame))`, `resource_changed::<T>`, `any_with_component::<T>`.
- **Simulation logic goes in `FixedUpdate`** (configure with `Time::<Fixed>::from_hz(..)`).
  Input/visual logic goes in `Update`. Frame-rate-dependent movement uses `time.delta_secs()`.
  Accumulated mouse motion must **not** be multiplied by delta time.
- One-time setup goes in `Startup` or, better, `OnEnter(state)`. Heavy work (continent gen)
  goes on `AsyncComputeTaskPool`, with the resulting `Task` polled in a system.
- Use `Res` instead of `ResMut` when not mutating. Unneeded `ResMut` serializes systems.

### 5.4 Communication between systems
- **Messages** (`MessageWriter` / `MessageReader`) for buffered, many-per-frame,
  pull-based data.
- **Observers** (`app.add_observer(fn)`, `entity.observe(fn)`, `commands.trigger(ev)`) for
  reactive, immediate, push-based logic. Lifecycle hooks: `On<Add<T>>`, `On<Remove<T>>`.
  Use `EntityEvent` for entity-targeted events. Picking and UI interaction should use
  observers.
- Use change detection (`Changed<T>`, `Added<T>`, `Ref<T>::is_changed`, `resource_changed`)
  instead of polling.

### 5.5 Queries
- `Single<D, F>` when exactly one entity must exist (the system is skipped otherwise).
  `Option<Single<..>>` for zero-or-one. `Query::single()` returns a `Result`.
- `Populated<..>` skips the system when the query is empty.
- Use `Without<..>` filters to disjoin conflicting `&mut Transform` queries, or a `ParamSet`.

### 5.6 Error handling
- Systems may return `Result` (`bevy::ecs::error::Result`). Use `?` instead of `unwrap()`.
  `BevyError` supports `.context()` / `.with_context()` (0.20). Panics in systems are routed
  to `FallbackErrorHandler` (0.20).
- Use `let Some(x) = .. else { return; };` for expected absence.
- Asset loaders return a dedicated `thiserror` error enum.

### 5.7 Assets & data
- Load through `AssetServer` and keep the handles. Group handles needed at startup into a
  `Resource` asset collection with `#[dependency]` fields, preloaded during a loading state
  (bevy_new_2d's `load_resource` pattern), instead of ad-hoc `load` calls in systems
  (e.g. `make_sim_ui` currently reloads the font).
- Data-driven content (buildings, materials, sim params) lives in RON with a custom loader per
  type. Keep loaders thin: parse a `*File` DTO, then convert and load dependencies via
  `load_context`.
- React to loading with `AssetEvent` messages or `AssetServer::is_loaded_with_dependencies`.
- 0.19+: inside `bsn!`, asset fields accept path strings directly (`Sprite { image: "a.png" }`).

### 5.8 UI
- 0.19+: build UI with `bsn!` scenes + `ui_widgets` (`Button`, scrollbars) and observers.
  Consider `bevy_feathers` widgets for editor/debug-like panels.
- Use the `px()` / `percent()` helpers. Use theme constants for colours and sizes (a `theme.rs`
  module).
- Don't poll `Interaction` (deprecated).

### 5.9 Rendering & materials
- The look is toon / cel-shaded (see §3.4): keep new materials stylized (flat colours, crisp
  bands, rim lights), not PBR-realistic.
- Credit every borrowed technique in `CREDITS.md` and in the code comment where it is used.
- Objects use `ToonMaterial` (`shaders::toon` converts a `StandardMaterial`, e.g. from
  glTF via `standard_material_from_gltf_material`), so they share the terrain's cel lighting.
  Its `snow` uniform covers upward faces above the snow line (matching the terrain).
  `SwayToonMaterial` adds `toon_sway.wesl`, a vertex shader bending grass-like models in the
  wind (no depth prepass, static shadows).
- Prefer data textures or **custom vertex attributes** (`MeshVertexAttribute`) for new
  terrain data. The terrain mesh currently reuses UV0 (feature gradient) and vertex colour
  red (wetness) as data, which works because the terrain has no real UVs or colours.
- Group material uniforms into one `#[uniform(100)]` struct (`ShaderType`) instead of five
  bindings. Mirror the struct in WESL.
- Keep shader asset paths as `const` next to the material type.

### 5.10 Style & hygiene
- `cargo fmt` and `cargo clippy --workspace --all-targets` clean (`clippy.toml` allows up to
  10 arguments: Bevy systems take many). Remove dead and commented-out code rather than keeping
  it (git has history).
- Doc comments (`///`) on every component, resource, message and plugin, saying *why* it exists.
- Use `info!` / `debug!` / `warn!`, never `println!` / `dbg!` in committed code.
- Name constants in SCREAMING_CASE at module top. Avoid magic sentinel numbers.
- Comments and identifiers in English. The user's notes have typos ("Ressources",
  "Collecter"); use correct spelling in code (`Resource`, `Collector`), except where it would
  collide with Bevy names. In that case pick a domain name like `Stockpile`.

---

## 6. Working on this repo (agent checklist)

1. `git status` first. The user often has uncommitted WIP.
   Don't discard or reformat their uncommitted work.
2. Build: `cargo run` (dev profile opt-level 1, deps opt-level 3). The first build is slow. The
   game needs `assets/models/*.glb`, which is not in git and must be present locally.
3. When upgrading or changing Bevy APIs, check the **exact version's** docs.rs and the example
   at the matching git tag (`github.com/bevyengine/bevy/tree/v0.X.0/examples`). The Bevy book
   and the unofficial cheatbook lag behind.
4. Keep `notes.md` as the design source of truth. If an implementation decision diverges from
   it, say so to the user.
5. Commit in small steps (one migration step or one refactor per commit).
6. **Verify without a human:** run
   `UF_SCREENSHOT=<scratch>/shot.png cargo run --release` (optionally `UF_SCREENSHOT_DELAY=<secs>`,
   counted from when the world is ready) with `UF_PLANET_CAMERA=lon,lat,distance,pitch,heading`
   (the view; the user's issue reports and click logs give it) and `UF_PLANET_REPORT=lon,lat;...`
   (what generation knows at points). The game saves a screenshot and exits. Don't judge 3D
   screenshots yourself: send them to the user (or give them positions). Scan the log for
   `ERROR` / `panicked`; generation stage timings are logged. For worldgen data, small
   throwaway examples (`crates/worldgen/examples/zz_*.rs`, deleted after) are the quickest. To exercise interactive paths, apply a *temporary* patch (e.g. a
   fake cursor at window centre), take the screenshot, then revert the patch. Never move the user's real
   mouse.
7. Full rebuilds take 10–20 minutes. Don't run a blind `cargo update`; prefer targeted
   `cargo update -p <crate>`.

## 7. References
- Bevy news / release notes: https://bevy.org/news/
- Migration guides: https://bevy.org/learn/migration-guides/introduction/
- Examples (pin to a tag): https://github.com/bevyengine/bevy/tree/v0.20.0/examples. Most
  relevant: `showcase/breakout.rs` (overall style), `scene/bsn.rs` (BSN), `ecs/observers.rs`,
  `ecs/message.rs`, `asset/custom_asset.rs` (loaders), `shader/extended_material.rs` +
  `assets/shaders/extended_material.wesl`, `3d/atmosphere.rs`, `3d/decal.rs`,
  `3d/mesh_ray_cast.rs`, `camera/pan_orbit_camera_*.rs`, `camera/camera_orbit.rs`.
- Project-structure conventions: https://github.com/TheBevyFlock/bevy_new_2d (`docs/design.md`)
- API docs: https://docs.rs/bevy/latest/bevy/

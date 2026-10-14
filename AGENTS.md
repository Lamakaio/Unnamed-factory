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

## 2. Repo map (as of the `bevy-upgrade` branch, 2026-10-13)

```
Cargo.toml              bevy 0.20 (+ bevy_remote, file_watcher, pan_orbit_camera,
                        bevy_dev_tools, render_dev_tools), ron 0.12, serde, anyhow,
                        rand 0.10, foldhash 0.2, half, worldgen (path). Workspace with crates/worldgen. All versions pinned.
                        Feature `tracy`: `cargo run --features tracy` for Tracy profiling.
Cargo.lock              tracked
rust-toolchain.toml     pinned nightly (nightly-2026-10-08). The user prefers nightly.
.cargo/config.toml      (git-ignored, local) rust-lld linker + `jobs = -4` to keep the PC usable while compiling
notes.md                former city-builder design notes (see §1)
docs/living-world.md    living planet design draft and decisions (the current direction)
src/main.rs             assembles plugins; `AppState { Loading, InGame }`
src/camera.rs           CameraPlugin: PanOrbitCamera + custom input mapping, arrow-key pan, keep above terrain
src/environment.rs      EnvironmentPlugin: sun (F rotates it), atmosphere entity
src/dev_tools.rs        DevToolsPlugin: BRP remote, wireframe, debug gizmo groups, terrain click log, UF_SCREENSHOT
src/map/                MapPlugin, Map resource; heights.rs (height function), rivers.rs, edits.rs, mesh.rs (LOD), water.rs, spatial.rs, ground.rs (ground material textures), vegetation.rs (trees, rocks, LODs)
tools/nature/           asset pipeline for the nature models (CC0 Kenney + Quaternius packs → Blender LOD builder, see its README)
assets/models/nature/   generated nature models <name>_lod{0,1,2}.glb (git-ignored)
crates/worldgen/        world generation crate: noise, erosion, lakes, rivers; grid.rs/sphere.rs/planet.rs (the planet); examples/preview.rs, planet_preview.rs (PNG previews)
src/shaders.rs          ShadersPlugin: MapMaterial = ExtendedMaterial<StandardMaterial, TerrainShader>, RON `.mapmat` loader
src/ui.rs               UiPlugin: loading text (BSN), `label()` text scene, PointerOverUi system param
assets/materials/map.mapmat terrain material params (pbr + tint as hex)
assets/shaders/map_material.wesl   terrain fragment shader (WESL): toon ground colours + cel lighting, per-pixel normals
assets/shaders/water.wesl          water fragment shader (WESL): toon water (depth bands, refracted swell and breakers, river flow and falls, glints)
CREDITS.md              sources of every borrowed technique (papers, articles, shaders); keep it up to date
```

### Controls
Camera: right drag orbit · middle drag pan · wheel zoom (all anchored on the point under the
cursor; nothing starts over UI) · arrows pan.
Debug: F rotate sun · F3 wireframe · F4 rivers/lakes · click terrain logs its data.
(Bevy's `RenderDebugOverlay` defaults to F1/F2 but is disabled by default.)

---

## 3. Current architecture (how it actually works)

### 3.1 App (`main.rs`, `camera.rs`, `environment.rs`, `dev_tools.rs`)
`main.rs` adds `DefaultPlugins` (nearest-neighbour images, includes TAA and UI widgets),
`MeshPickingPlugin`, and one plugin per module. `AppState` starts in `Loading` (world
generation, "Generating world..." text) and moves to `InGame` once the `Map` exists. Systems
that read `Map` are gated with `run_if(in_state(AppState::InGame))`, and observers take
`Option<Res<Map>>`.
- **Camera** (`camera.rs`): HDR, Bloom, TAA, `AtmosphereSettings`, `DistanceFog` and an
  `AmbientLight` override on the camera. It uses Bevy 0.20's `PanOrbitCamera`, which ships
  without input bindings. The input mapping is adapted from Bevy's
  `pan_orbit_camera_custom_input_plugin` example and runs in `PreUpdate`, before
  `SyncCameraPosition`. The camera controller applies deltas to `Transform`, so
  `keyboard_pan` (Update) and `keep_above_terrain` (PostUpdate) edit the transform directly.
  The camera spawns at startup (it renders the loading screen) and `look_at_start` places it
  over `Continent::start_point()` on entering `InGame` (`UF_CAMERA=x,z[,dx,dy,dz]`
   (and `cargo run -- --seed <n>` picks the world seed, default 1082) overrides
  the target and offset, for automated screenshots). The ambient light is a weak sky blue so
  the sun shapes the relief.
- **Environment**: sun `DirectionalLight` (`lux::RAW_SUNLIGHT`, low and from the side of the
  starting view: front lighting makes the terrain look flat), 4 shadow cascades (first bound
  25, max 600) with a 4096 shadow map, and an `Atmosphere` entity
  (`ScatteringMedium::default()`).
- **Dev tools**: `RemotePlugin` + `RemoteHttpPlugin` (BRP inspector), `WireframePlugin`, gizmo
  config group `RiverGizmos` toggled with a run condition. Setting
  `UF_SCREENSHOT=<png>` (and optionally `UF_SCREENSHOT_DELAY`, default 20 s) saves a
  screenshot and exits. `UF_LAB=<dir>` (lab mode) keeps the game running and takes
  screenshots on request (`<dir>/shots.txt`). See §6.

### 3.2 World generation (`crates/worldgen`) and terrain (`map/`)
- **`worldgen` crate** (workspace member, no Bevy ECS; depends on `bevy_math` and `rand`;
  `opt-level = 3` even in dev). `worldgen::generate(&WorldgenSettings)` builds one
  **continent**: a 512×512 grid of 1-unit cells (the continent spans ±256 world units around
  its own center; sea level is 0). Plan: several independent continents placed at offsets
  instead of one bigger map (generation cost grows with area). Stages, ~3.5–4.5 s in total
  (shape ~0.7 s, erosion + post-processing ~2–3 s):
  1. **Shape** (`terrain.rs::shape`): domain-warped noisy disc coastline (sea forced near the
     borders), then **tectonic provinces**: ~10 plates (warped Voronoi, blended over
     `province_blend` cells), each with a random velocity and an interior type: plains, hill
     country or plateau (`province_weights`). Per cell, `(d_k − d1)/2` approximates the
     distance to the boundary with plate k (continuous where the nearest plate changes):
     - strongly **converging** boundaries get a **mountain range belt** (`range_width` cells
       each side) made of several parallel ridges (`ridge_spacing`, wavy) broken into peaks
       and passes by ridged noise, plus wider foothills;
     - **diverging** boundaries get rift basins (lowered uplift).
     Provinces set uplift, erodibility (plateaus are hard caprock) and hillslope diffusion
     (hills are rounded). `Shape::range` (belt weight) drives the `Landform` classification
     and the detail ruggedness. Rain: orographic sweep along a prevailing wind (random per
     seed), with sideways mixing and a blur, so ranges leave dry rain shadows. Noise is the
     crate's own `noise.rs` (Perlin + fBm / ridged / warp).
  2. **Erosion** (`terrain.rs::erode`): stream power law `dh/dt = U − K·A^m·S` (n = 1), solved
     implicitly from the outlets upstream (Braun & Willett 2013), plus hillslope diffusion.
     Each step does a Priority-Flood with epsilon (Barnes 2014; depressions are filled), then
     **stochastic D8** routing (random slope weights against grid-aligned valleys), then
     drainage accumulation. The sea is base level, fixed at 0 during erosion. Afterwards:
     - land is scaled so the 99th percentile of range cells is `max_height` (38), and sea
       cells get their sea floor depth;
     - `smooth_cells`: a light binomial blur (erosion leaves a 1-cell checker on flats);
     - `shape_coast`: land rises `inland_rise` (3) over 100 cells from the coast, and is
       capped by a ramp from the shore (gentle beaches; no cliffs dropping into the sea); the
       sea floor starts as a shallow shelf. Both follow a blurred signed distance to the
       shore (returned, and used for beaches), so the shore doesn't follow the cell staircase;
     - `cap_plateaus`: plateau provinces are cut at their cap level (tablelands, escarpments);
     - `limit_slopes`: thermal erosion to a talus slope (~0.45–0.7 per cell, steeper on
       plateau edges), so nothing is abruptly steep.
  3. **Lakes** come out of erosion: basins sink (`Shape::subsidence`: rift floors in segments
     between raised shoulders, kettle hollows on plains) or are dug by glaciers (high valleys
     with large drainage, the last `GLACIAL_STEPS` steps, uneven along their course).
     `erode` routes flow on the water surface (Priority-Flood) but doesn't fill basins: no
     incision under a lake, sediment fills `LAKE_FILL` of its depth per step. `find_lakes`
     (`lakes.rs`) reads the basins of the final heights: flooded > 0.01 (the shallow fringe
     belongs to the lake), ≥ 20 cells, ≥ 0.4 deep; smaller ones are filled in. Basins whose
     inflow (rain on the terrain, as for the climate) can't match their evaporation (more
     where drier) are endorheic: they shrink to what they sustain, leave salt flats
     (`Continent::salt_flat`, ground_c.g), and are sinks for river routing. Floodplains keep
     lake shores above their level (`LAKE_RIM`); after the fine detail, `settle_lakes`
     lowers each level to its basin's spill point on the fine surface (Priority-Flood), and
     inflows / outlets meet it. Endorheic only if every cell within `DRY_RADIUS` is dry, the
     inflow is low, and the basin is deep (`ENDORHEIC_BARRIER`) or near sea level (rare).
     Rivers are led through their lake to its deepest point at the lake's level, outlets
     from it and inflows to it (`extend_through_lakes`), so their channels are carved
     through the bed; drawn, they stay visible ~1 unit past the shore, then fade
     (`LAKE_FADE_INSIDE`, `OUTLET_FADE`), only at their own lakes. `settle_lakes` (after the
     fine detail) sets each lake's level to the spill point of its deepest point's basin on
     the fine surface (Priority-Flood; lake beds below sea level are not sea; lakes with no
     outlet `NO_OUTLET_MARGIN` below it) and floods its extent from the samples of its cells
     in that basin over the fine samples ≥ `LAKE_MIN_DEPTH` below the level, then drops
     strands narrower than ~2 × `STRAND_RADIUS` samples (`open_mask`, e.g. a river's trench
     running out of the lake):
     `Continent::lake_mask` (`LakeMask`), the one definition of "under a lake"
     (`Continent::lake_over`) for river levels (at the lake's level there, also across
     their width), river fades and lake surfaces (`water.rs::lakes_mesh`: marching squares
     on the drawn ground's samples: grown from the mask over lower ground connected to it
     and up to `SHORE_STEPS` samples onto the shore, outlined where the ground rises
     `SHORE_MARGIN` above the level, never across the rim; less the opaque river ribbons,
     cut out exactly with `subtract_triangle`, so lakes and rivers meet edge to edge; drawn
     `LAKE_SINK` under the level, below the ribbons fading in or out over them). River banks
     never carve a lake's rim below its level within `RIM_HOLD` of it (`rivers.rs`,
     `LakeRims`). The terrain mesh is fine (`SHORE_CELL`) within
     `SHORE_REACH` cells of lake edges.
   - **Limits that normal worlds should not hit log a warning** (lake flood reach, river bend
     relaxation rounds, tributaries that never cross their parent's edge, lab screenshot
     timeout). Treat such a warning as a bug to fix, not noise.
  4. **Rivers** (`rivers.rs`): routing on the epsilon-filled surface; cells with drainage
     ≥ `river_min_area` are rivers, so they always merge. They are traced from mouths (sea or
     lake) upstream, the main stem following the largest donor, and tributaries end exactly on
     their parent's polyline (`RiverEnd::Junction(parent)`, parent index < own index).
     Then Chaikin smoothing and resampling every 0.5 units. Width ∝ √discharge, depth
     ∝ discharge^0.4. Meanders where the valley is flat (wavelength ≈ 12 widths, amplitude in
     widths × flatness, tapered at the ends); bends tighter than `MIN_BEND_RADIUS` widths
     (where the meander offset would fold over, and at junction snaps) are relaxed
     (`relax_bends`). Water levels follow the terrain
     (**breaching**, never filling), are never increasing downstream, and are finally clamped
     to the terrain under each point. Floodplains are flattened to water + 0.4 around
     meandering reaches (`lib.rs::flatten_floodplains`, soft edges). The final clamp
     (`clamp_rivers_to_terrain`) uses the lowest fine surface across the channel width.
     **Mouths** (`mouths.rs`): each river ending in the sea gets a `Mouth` from its width,
     the flatness of the coast and a seeded roll (`Continent::mouths`). Plain: unchanged.
     Estuary: the last stretch widens, with a sand fan (low flat land, wide beach). Delta
     (large rivers on flat coasts): the river ends at the shore and splits into a tree of
     distributaries (uneven forks, wandering channels kept within 75° of the seaward
     direction, tips grown until they reach the sea); land is raised along the channels
     (wide near the apex, levees further out, open at each tip), and marked as wetland
     (extra moisture → marsh). Distributaries are extra `River`s after the original ones.
     The shore distance is recomputed afterwards.
  5. **Climate** (`climate.rs`): moisture = orographic rain on the final terrain + wet valley
     floors (drainage, blurred so single-cell flow lines don't show), temperature from
     altitude (`snow_line`), and a `Biome` per cell (beach, marsh, meadow, steppe, forests,
     alpine, rock, snow; `Biome::color()` is shared by previews and the game). Beach width
     varies along the coast (noise, 1–4.5 cells) and widens on estuary sand fans. Biomes
     are by height (`h <= 0` is sea), since deltas turn sea cells into land.
  6. **Detail** (`detail.rs`): the eroded grid upsampled ×4 (bicubic, 0.25 units) into
     `Continent::surface`, with gullies on slopes (branching "erosion noise" stripes,
     rotated lattice per octave, patchy strength) and small fBm roughness, scaled by
     ruggedness and faded near rivers and lakes. Threaded with `std::thread::scope`.
  - Output `Continent`: `heights` (coarse), `surface` (fine, what the game renders),
    `drainage`, `lake_of`, `lakes` (level + cells), `rivers` (polylines of
    `RiverPoint { position, width, depth }` + `RiverEnd`), `landform`, `ruggedness`,
    `moisture`, `temperature`, `biome`, `wind_angle`, `timings`, `start_point()` (beside the
    widest lowland river; the camera starts there).
  - **Preview tool**: `cargo run -p worldgen --release --example preview -- <out_dir> [seeds…]`
    writes `<seed>_biomes/_landforms/_relief.png` (hillshaded, with rivers and lakes) and
    `<seed>_detail.png` (a crop of the fine surface), and prints per-landform height / slope
    stats and timings. Use it to tune generation without the game. Don't pipe its output into
    `head`: the broken pipe kills it before it writes the images. Tests:
    `cargo test -p worldgen --release`. `fingerprint` (`--example fingerprint -- [seeds]`)
    prints a hash of the generated heights, surface, rivers and lakes: run it before and after
    a refactor that must not change generation.
- **Planet** (milestone 1 of `docs/living-world.md`, not in the game yet):
  - `grid.rs`: the `Grid` trait (cells, neighbours with distances in cells, border, blur).
    `flow.rs` (Priority-Flood, `FlowGraph`) and `terrain::erode` are generic over it; the flat
    map passes a `PlaneGrid`. Erosion routes with `FlowGraph::with_depressions` (Cordonnier et
    al. 2019 basin graph: steepest descent, pits' basins linked through their lowest passes,
    minimum spanning tree to the outlets, pits carved out over their pass; also returns the
    water surface, equal to Priority-Flood without epsilon), not a Priority-Flood per step.
    `erode_timed` reports the time of each part.
  - `sphere.rs`: `SphereGrid`, an equiangular cube-sphere of `6 n²` cells, indexed
    `(face * n + z) * n + x`; `direction(i)`, `cell_at(direction)`, `face_coords`, `area(i)`
    (relative to the mean). Neighbours across face edges are found by direction (at cube
    corners a cell has 7). Tests: round trip, areas, mutual neighbours, everything drains.
  - `noise.rs`: 3D noise (`get3`, `fbm3`, `ridged3`, `warp3`) to sample the sphere by direction.
  - `planet.rs`: `generate_planet(&PlanetSettings) -> Planet` (module docs explain it):
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
    `shape_on(grid, scale)` keeps every length in full-size cells; `WorldgenSettings::cell_size`
    tells erosion how long a cell is), then upsampled (`upsample`) and refined at full
    resolution (`--coarse 0` in the preview: all at full resolution). Then scaling, `cap_plateaus`, a
    sea floor of shelf / slope / abyss (raised at ridges, deepened at trenches). Both shaping
    passes run on all cores (`par_map`); provinces and blocks are found through a coarse
    `PointIndex`. Landforms are sized in cells (radius `2n/π` cells; n = 1024 is the target
    planet, 256 the fast default for previews).
  - `planet_climate.rs` (milestone 3, v1): `simulate(&Planet, &ClimateSettings) ->
    PlanetClimate`, twelve months per cell of a coarse cube-sphere (n = 48), as if the planet
    were Earth-sized: energy balance per cell (insolation by latitude and tilt, `A + B T`,
    ice albedo, ocean/land heat capacity, implicit diffusive transport weaker over land,
    rule-based ocean current anomalies), winds from pressure belts around the ITCZ (plus warm
    land lows) turned by Coriolis, moisture evaporated, advected semi-Lagrangian and rained out
    by terrain uplift, convergence and saturation (sinking air dry). `Koppen::classify`, and
    `PlanetClimate::koppen_at(direction, height)` (bilinear months, lapse rate with
    `meters_per_unit` = 100). Tuned on the zonal means printed by `climate_preview`
    (`--example climate_preview -- <out_dir> [--n] [seeds]`, also Köppen / rain / January and
    July temperature maps). In the game the ground colour is the cell's Köppen
    `ground_color` (`PlanetSurface::ground`), sand at the water's edge.
  - Preview: `cargo run -p worldgen --release --example planet_preview -- <out_dir> [--n
    <cells>] [seeds…]` writes `<seed>_map.png` (equirectangular relief with rivers),
    `<seed>_globes.png` (two orthographic views) and `<seed>_plates.png` (continental plates
    bright, oceanic dark), with lakes. Whole planet at n = 1024: ~13 s (shape 3 s, erosion 8 s,
    of which routing's sequential part is 5 s); n = 512: ~3 s. Per-cell passes run on all
    cores through `par.rs` (scoped threads, results identical to sequential).
    In-face neighbour distances are precomputed per face position (`local_distance`).
- **Planet mode** (`cargo run --release -- --planet [--n <cells>] [--seed <n>]`, milestone 2,
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
  colour by height for now, drawn with the toon `ToonMaterial`, snow off), re-meshed when the camera moved by 25% of their distance. The
  Rivers (`worldgen::planet_rivers`: traced on the sphere as on the flat map, smoothed,
  resampled every 0.5 cells, meandering on flat valleys) get their water levels clamped
  under the surface across the channel and end where they reach the sea (`PlanetSurface::new`);
  `PlanetSurface` carves their channels (parabolic channel + banks, segments bucketed in 3D).
  `planet/water.rs` draws river ribbons and per-face lake surfaces (cell squares with a
  corner under a lake, at the water level) with a translucent `StandardMaterial` (no flow
  yet). The
  sea is a translucent sphere, subdivided by radius (`sea_sphere`: Bevy's `ico(k)` splits each
  edge into `k + 1` segments, coarse ones sag below shallow water). Coasts are shaped as on
  the flat map (`planet.rs`: blurred signed distance to the shore, land ramp, shelf). The camera (`planet/camera.rs`, `PlanetCamera`, replacing the
  pan-orbit controller) looks at a focus point on the surface from a distance, with a
  tangent forward direction parallel-transported as the focus moves (no flip at the poles);
  right drag orbits, middle drag / arrows move the focus, wheel zooms; far out it looks
  straight down; it stays above the ground and fits its clip planes to the view. No
  atmosphere or fog. The sun follows the view (low afternoon sun over the focus, F turns
  it) until the planet has a day cycle. `UF_PLANET_CAMERA=lon,lat[,distance[,pitch°]]` sets
  the starting view (screenshots).
- In the game, `map::start_generation` runs `worldgen::generate` plus `TerrainHeights::new` on
  the `AsyncComputeTaskPool`. `finish_generation` polls the task with `check_ready`, inserts
  `Map` (`Map.continent` is the `worldgen::Continent`), and enters `AppState::InGame`.
- **Height function** (`map/heights.rs`, `TerrainHeights`, cheap `Arc` clones for tasks):
  `height(p)` = the continent's fine surface (0.25 units) with bicubic (Catmull-Rom)
  interpolation → river layer → edit layer. Beyond the continent the border values extend
  (open sea). Wetness = river wetness, or bilinear lake mask. Ground colour = biome colours
  (bilinear per cell, sampled at a noise-warped position so borders don't follow the grid).
  `Map::get_height` / `Map::sample` use it, so the camera, placement and mesh always agree.
  Normals use finite differences at a fixed epsilon (1/16). `gradient_texture()` bakes the
  base surface gradient into an `Rg16Float` texture (one texel per surface sample) for the
  terrain shader; `feature_gradient(p)` is what rivers and edits add on top.
  - **River layer** (`map/rivers.rs`): the generated river polylines, as segments in a spatial
    grid. Cross-section: a parabolic channel (`width`, `depth` below the water line), then a
    continuous ease-out bank from the water line back to the terrain, max(0.8, 0.6·width)
    wide and widened (up to 3) to keep its slope under 0.8: steep walls alias into a
    sawtooth on the mesh grid. It only ever lowers
    the terrain. The valleys themselves are baked by erosion. Wetness is 1 in the channel.
  - **Edit layer** (`map/edits.rs`): `TerrainEdit::Flatten` (flat over the inner 60% of
    the radius, smootherstep blend outside) and `Raise` (smootherstep bell). Edits apply in
    insertion order and are found through a spatial grid. `Map::add_edit` records the touched
    chunks for re-meshing. Nothing makes edits since the city builder was retired; kept as the
    path for runtime terrain changes.
- **Meshing** (`map/mesh.rs`): each 64-unit chunk is a quadtree of 16×16-quad patches, meshed
  in 4×4 separate parts (`TerrainPart` child entities with `GroundMesh`, the picking target;
  `PART_DEPTH`), so rebuilds near the camera stay small. Cells
  go from 4 units down to `MIN_CELL` = 1/16. A node splits while its cell is larger than
  needed:
  - for the base terrain: curvature (max-curvature pyramid in `BaseGrid`) against an error
    tolerance of 0.0015 × camera distance, never finer than a quarter of the base cell;
  - for rivers / edits overlapping it: river width / 6, or edit falloff width / 8, relaxed to
    0.003 × camera distance.
  Patches don't share vertices across levels. Edge vertices next to a coarser leaf (found by
  re-running the split test from the chunk root, `leaf_at`) are snapped onto the coarser
  edge, so there are no T-junction cracks; skirts (2 cells deep) still hide cracks against
  chunks meshed for another camera position. Vertex colour red = river/lake wetness (the
  shader draws the ground). **UV0 is data**: the feature gradient (slope added by rivers
  and edits), which the shader adds to the gradient texture.
- **Chunks** (`map/mod.rs`): entities with `IsGround(coords)` + `TerrainChunk` LOD state,
  spawned within 6 chunks of the camera (never despawned). `schedule_chunk_meshes` starts
  at most 4 background mesh tasks, edited chunks first and then nearest. A chunk is re-meshed
  when an edit touched it, or when the camera moved by max(4, 25% of its distance) since its
  mesh was built. `finish_chunk_meshes` swaps the mesh asset in place. Mesh build times and
  vertex counts are logged at `debug` (`RUST_LOG=info,unnamed_factory=debug`): up to ~0.1 s /
  170k vertices per chunk on the eroded terrain.
- **Water** (`map/water.rs`): one translucent, double-sided `WaterMaterial`
  (`ExtendedMaterial<StandardMaterial, WaterShader>`, `water.wesl`, see §3.4) for a sea plane
  at 0, lake surfaces (contoured, see the Lakes step of world generation above), and river ribbons at the
  water line (width + 0.25 per side). Ribbons stop where the water reaches the sea (within 0.12: the
  sea plane covers the carved channel) or the lake the river flows into (overlapping
  translucent surfaces look darker). Ground under water is darkened at once (×0.6), so
  shallows don't read as dry sand. Water has `Pickable::IGNORE` and `NotShadowCaster`.
  - **River profile** (`river_profile`): where the water line is steeper than
    `CASCADE_SLOPE` (0.18), it runs in irregular steps (per-point `jitter`): pools (keeping
    a random `POOL_TILT` share of the slope from their lowest point, never above the natural
    line, about `CASCADE_STEP` × depth high) with a fall row between them (slant
    `FALL_RUN`), and sloping chutes (`CHUTE_SHARE`). The channels are carved along this
    drawn water line (`water_line`, falls included), so water never stands above its banks.
    Steep stretches (`DOME_SLOPE`) get volume: the ribbon's cross-section becomes the top of
    an oval (`Row::dome`, `DOME_SHAPE`), bulging in the middle with its edges sunk under the
    banks, and lowered by `DOME_DROP` (the terrain mesh lags under steep channels), flat again over each river's first and last rows (ponds, lakes, junction caps). Single-point pools (very steep streams) stay ramps
    (continuous rapids). Tributary ribbons end on the parent's actual mesh edge: each
    tributary edge is cut where it crosses the parent's near edge, and the end is filled by a
    fan over an outline that runs along the parent's edge vertices (`junction_cap`), so the
    surfaces share vertices (positions, heights, flow); the parent gets whitewater below
    each confluence (`CONFLUENCE_FOAM`). Tributaries join `JOIN_LEAD` points downstream of
    the nearest parent point (`rivers.rs`), so they meet at a downstream-leaning angle.
    River vertices carry UV0 = flow velocity (world x, z; per side, so it can vary across)
    and UV1.x = whitewater (rapids, below falls and confluences). At a confluence the
    tributary's flow turns towards its parent's and the parent's near bank towards the
    tributary's (`CONFLUENCE_TURN`), so the flow-mapped pattern bends from one into the
    other.
  - **Springs** (`map/springs.rs`): each river source not on a lake, the sea or another
    river gets a spring, by the terrain slope around it (`POND_MAX_SLOPE`): a lumpy
    **pond** (bowl carved like river channels, the river ribbon starting on its outline via
    `junction_cap` with a closed edge; pond surfaces in their own mesh), or a **hillside
    spring** (a notch carved in the slope, a spout row `SPOUT_HEIGHT` above the stream with
    foam, and a rock outcrop placed by `vegetation::Placement::spring_rocks`). Springs are
    part of `TerrainHeights` (carving, wetness, mesh detail).
  - **Sea field** (`map/sea.rs`, `Rgba16Float` per continent cell, binding 101–103): wave
    travel time to the shore (Dijkstra with knight moves, speed ~ √depth, so crests refract
    around headlands and bunch up in the shallows), depth, exposure to the prevailing wind
    (`Continent::wind_angle`, binding 104) and shore distance. Exposure is scaled down where
    little of the surroundings is open water (`OPEN_WATER`), so swell stays out of narrow
    channels.
  - **River ends**: rivers fade out into the sea or lake they flow into (`RIVER_FADE`, UV1.y
    = opacity), extended straight past their last point if needed; fading quads are split at
    the center line so their sides fade too (no hard edge without banks).
- River center-line debug gizmos live in `dev_tools.rs` (F4, off by default).
- **Vegetation** (`map/vegetation.rs`): trees, bushes, rocks, dead wood and ground cover,
  placed deterministically per chunk on jittered grids (0.9 / 0.35 units) with per-biome
  probabilities (`large_rules`, `ground_rules`), forest clearings from noise, and nothing on
  water, steep or edited ground. A chunk's vegetation is re-placed when an edit touches it
  (buildings clear it). Models come from `tools/nature` (metres × `METERS_TO_WORLD` = 0.05;
  buildings use the same 0.05 scale, one unit = 20 m), with toon materials (`ToonMaterial`,
  `toon.wesl`). Levels of detail, drawn with few entities (entity spawning was the lag):
  - far chunks (> 90): one merged mesh of the LOD2 stand-ins (~25 triangles each). It never
    fades, so trees never disappear in the distance;
  - near chunks (< 90): one merged mesh of the LOD1 stand-ins (~120 triangles), built in a
    background task. Each vertex carries its object's origin (UV0 = x, z; UV1.x = y), and
    `LodToonMaterial` (toon.wesl with `LOD_FADE`, no depth prepass) dithers every object out
    exactly where its LOD0 model fades in, with Bevy's visibility range dither pattern;
  - LOD0 models are entities only within 20 of the camera (`VisibilityRange` 18–22),
    spawned and despawned as the camera moves (600 per frame at most); ground cover
    likewise, fading out at 14–18.
  Tall grass grows in patches (a third grid, 0.18, gated by noise to ~30% of the ground:
  `patch_rules`), swaying (`Species::sways`). Deserts get cacti, dry shrubs, red rocks.
  Models: Kenney low-poly trees / rocks / stumps (recoloured to the palette, LODs by
  decimation, ~50–400 triangles) for the bulk, Quaternius textured hero trees (≤ 4000
  triangles, rare), boulders, bushes and ground cover. Species and per-biome probabilities
  are the `SPECIES`, `large_rules` and `ground_rules` tables.
  Placement grids: 0.45 (large) and 0.28 (ground cover). Background tasks (placement, near
  meshes): 4 at once.

### 3.3 Buildings (retired)
Removed on 2026-10-13 with the city builder (placement, zones, tools, selection, build grid).
What remains: the terrain **edit layer** (`map/edits.rs`, `Map::add_edit`), which nothing
uses now but is the path for runtime terrain changes (it re-meshes the touched chunks).

### 3.4 Rendering (`shaders.rs`, `assets/shaders`)
`MapMaterial = ExtendedMaterial<StandardMaterial, TerrainShader>`, loaded from `.mapmat` RON
(`StandardMaterialParams` + a `tint`, hex colours through `deser_color`). `TerrainShader`
binds `tint` (100), the base surface gradient texture + a shared sampler (101, 102) and its
world → uv mapping (103), and two ground textures (104, 105) with their mapping (106).
`map::bind_terrain_textures` sets the textures once the material is loaded (and again after a
hot reload). The shader is WESL (`map_material.wesl`):
- **Normals** per pixel = gradient texture + UV feature gradient (rivers, edits), so lighting keeps the full surface detail at any mesh LOD (vertex normals
  alone alias into a regular pattern on coarse patches).
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
    turns them into a grain). Very close to the camera, real blades (`map/grass.rs`: a mesh
    of blade triangles per 1×1 tile around the point looked at, where the biome has grass,
    `GrassBladeMaterial` with `grass_blades.wesl` bending them with the same wind, gusts and
    sway as the carpet) stand in front of it and fade out with distance. Grass flattens near
    water and on edited ground (`TerrainHeights::grass_clearance`, mesh vertex colour green). Gusts sweep downwind as lighter
    bands at every distance. The wind is the world's prevailing wind (`wind`, binding 108,
    set with the textures). Forest floors get a weaker carpet.
  - **Snow** settles on flat ground (steep rock stays bare), with a ragged noisy edge and
    wind-blown drifts. **Desert** (`ground_c.r`): dune ripples, cracked clay flats, redder
    rock. **Marsh**: reed beds (wind strokes) around still teal pools with a muddy rim.
- **Water** (`water.wesl`, `WaterShader`): crisp depth bands (`BANDS`; sea depth from the
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
- **Weights** come from `map::ground` textures (one texel per continent cell, RGBA8):
  `ground_a` = sand, rock, snow, marsh (from biomes, snow from temperature); `ground_b` =
  dryness (from moisture), deciduous, conifer, alpine (from temperature); `ground_c` =
  desert (binding 107). They are sampled
  at a noise-warped position and sharpened with noise (`edge`), so borders are irregular.
  Rock on steep slopes (1 − N.y from ~0.28 to 0.45) and snow sliding off them are per pixel.
  Mesh vertex colour red is the river/lake wetness, green the grass clearance.
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
- `cargo check` passes with only dead-code warnings: unused data fields, `TestCurve`
  experiments, `ChunkMarker`.
- `bevy_dev_tools` was enabled for the (retired) build grid; check whether anything else
  still needs it. Bevy says not to ship dev tools.
- Tests exist only for `worldgen`.
- Lake dams are discs, so some show up as rounded hills next to their lake.
- Plateau tops can hold small depression lakes (cap + slope limiting leave pits).

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
- `cargo fmt` and `cargo clippy` clean. Remove dead and commented-out code rather than keeping
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
6. **Verify visually without a human:** run
   `UF_SCREENSHOT=<scratch>/shot.png cargo run` (optionally `UF_SCREENSHOT_DELAY=<secs>`). The
   game saves a screenshot and exits, and you can then view the PNG. For shader work, run
   `UF_LAB=target/lab/shots cargo run` in the background once: write lines
   `<name>.png x z [dx dy dz]` to `target/lab/shots/shots.txt`, wait for `done.txt`, view the
   PNGs; shaders hot-reload between requests (Rust changes need a restart). The lab keeps the
   camera above the ground, sets the pan-orbit controller's anchor depth (its near clip plane
   is 5% of it: a stale far anchor clips everything close), and waits until terrain chunks
   are done meshing. For close-ups, prefer a camera a little higher and angled down. `UF_CAMERA=x,z[,dx,dy,dz]`
   points the start view at world (x, z) from offset (dx, dy, dz). Also scan the log for
   `ERROR` / `panicked`. To exercise interactive paths, apply a *temporary* patch (e.g. a
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

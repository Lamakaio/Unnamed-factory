# AGENTS.md — Unnamed Factory: project recap & Bevy guidelines

> Audience: AI agents (and humans) about to work on this repo. Read this first.
> Written 2026-10-09. Bevy facts were checked against the official migration guides,
> release notes and the `v0.20.0` examples on that date. **The code is on Bevy 0.20**
> (upgraded version by version on 2026-10-09; see §4). §3 describes the architecture, which
> the upgrade left largely unchanged. When an API detail here disagrees
> with the compiler or docs.rs, the compiler wins; fix this file.

---

## 1. What this project is

A **3D city builder** written in Rust with Bevy. The crate is called `unnamed-factory`,
but the design is a city builder, not a factory game. Setting: a "perfect world" (no
crime, no war). The game is about **occupations, buildings, zones and resource flows**.

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
  for fast iteration. Typed object tables (one per class), opaque integer handles, and
  "links" between objects. Simple links are linear (`src * delta → dst growth rate`) and
  are meant to batch into a matrix multiply. Advanced links are arbitrary functions.
  *This handle/link design is **not** implemented yet.* The current Rhai scripts use one
  nested map (see §3.5).

### Assets & licensing
- `assets/models/*.glb`, the fonts and some images come from **purchased asset packs**.
  The user has many more packs available.
- `.gitignore` excludes `*.glb`, so models are **not in git**. Never commit or
  redistribute purchased assets.
- Data-driven content lives in `assets/` as RON files with custom extensions (`.bconf`,
  `.mapmat`) and Rhai scripts (`.rhai`).

---

## 2. Repo map (as of the `bevy-upgrade` branch, 2026-10-09)

```
Cargo.toml              bevy 0.20 (+ bevy_remote, file_watcher, pan_orbit_camera,
                        bevy_dev_tools, render_dev_tools), ron 0.12, serde, anyhow,
                        kdtree-collisions (user's own git crate, no bevy dep), rand 0.10, rhai 1.26,
                        foldhash 0.2, worldgen (path). Workspace with crates/worldgen. All versions pinned.
                        Feature `tracy`: `cargo run --features tracy` for Tracy profiling.
Cargo.lock              tracked
rust-toolchain.toml     pinned nightly (nightly-2026-10-08). The user prefers nightly.
.cargo/config.toml      (git-ignored, local) rust-lld linker + `jobs = -4` to keep the PC usable while compiling
notes.md                game design notes (see §1)
src/main.rs             assembles plugins; `AppState { Loading, InGame }`
src/camera.rs           CameraPlugin: PanOrbitCamera + custom input mapping, arrow-key pan, keep above terrain
src/environment.rs      EnvironmentPlugin: sun (F rotates it), atmosphere entity
src/dev_tools.rs        DevToolsPlugin: BRP remote, wireframe, debug gizmo groups, terrain click log, UF_SCREENSHOT
src/map/                MapPlugin, Map resource; heights.rs (height function), rivers.rs, edits.rs, mesh.rs (LOD), water.rs, spatial.rs
crates/worldgen/        world generation crate: noise, erosion, lakes, rivers; examples/preview.rs (PNG previews)
src/build.rs            BuildPlugin: Building asset type, placement (StartPlacement event), picking selection, build grid
src/build_asset.rs      BuildAssetPlugin: RON `.bconf` loader → Building asset
src/shaders.rs          ShadersPlugin: MapMaterial = ExtendedMaterial<StandardMaterial, TerrainShader>, RON `.mapmat` loader
src/sim.rs              SimPlugin: Rhai script asset + loader, init/tick (FixedUpdate), stats screen (BSN)
src/ui.rs               UiPlugin: build menu (BSN), `label()` text scene, PointerOverUi system param
assets/buildings/*.bconf   BuildingFile RON (name, size, typ: Single{model,scale} | Zone{color} | Tool{op,color}, script)
assets/materials/map.mapmat terrain material params (pbr + tint as hex)
assets/scripts/init.rhai, run.rhai  sim data init + one sim tick
assets/shaders/map_material.wesl   terrain fragment shader (WESL): per-pixel normals from a gradient texture, rock on steep slopes
```

### Controls
Camera: right drag orbit · middle drag pan · wheel zoom (all anchored on the point under the
cursor; nothing starts over UI) · arrows pan.
Building: build menu button starts placing · left click places (drag resizes zones) ·
Ctrl+click places and keeps placing · S cycles snapping · hover highlights a placed building ·
click picks it up again.
Sim: Enter (held) runs ticks at 20 Hz · R re-runs init · Tab stats screen.
Debug: F rotate sun · F2 AABBs · F3 wireframe · F4 rivers/lakes · click terrain logs its data.
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
  over `Continent::start_point()` on entering `InGame` (`UF_CAMERA=x,z[,dx,dy,dz]` overrides
  the target and offset, for automated screenshots). The ambient light is a weak sky blue so
  the sun shapes the relief.
- **Environment**: sun `DirectionalLight` (`lux::RAW_SUNLIGHT`, low and from the side of the
  starting view: front lighting makes the terrain look flat), 4 shadow cascades (first bound
  25, max 600) with a 4096 shadow map, and an `Atmosphere` entity
  (`ScatteringMedium::default()`).
- **Dev tools**: `RemotePlugin` + `RemoteHttpPlugin` (BRP inspector), `WireframePlugin`, gizmo
  config groups `AabbGizmos` / `RiverGizmos` toggled with run conditions. Setting
  `UF_SCREENSHOT=<png>` (and optionally `UF_SCREENSHOT_DELAY`, default 20 s) saves a
  screenshot and exits. See §6.

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
  3. **Lakes** (`lakes.rs`): valley sites (enough drainage, inland, spaced apart). A
     moraine-like dam disc 20–40 cells downstream is raised to the lake level, growing until it
     seals the valley; the valley is flooded upstream and the bed deepened. `find_lakes` then
     finds lakes with a Priority-Flood without epsilon (flat surfaces, ≥ 20 cells).
  4. **Rivers** (`rivers.rs`): routing on the epsilon-filled surface; cells with drainage
     ≥ `river_min_area` are rivers, so they always merge. They are traced from mouths (sea or
     lake) upstream, the main stem following the largest donor, and tributaries end exactly on
     their parent's polyline (`RiverEnd::Junction(parent)`, parent index < own index).
     Then Chaikin smoothing and resampling every 0.5 units. Width ∝ √discharge, depth
     ∝ discharge^0.4. Meanders where the valley is flat (wavelength ≈ 12 widths, amplitude in
     widths × flatness, tapered at the ends). Water levels follow the terrain
     (**breaching**, never filling), are never increasing downstream, and are finally clamped
     to the terrain under each point. Floodplains are flattened to water + 0.4 around
     meandering reaches (`lib.rs::flatten_floodplains`, soft edges). The final clamp
     (`clamp_rivers_to_terrain`) uses the lowest fine surface across the channel width.
     River mouths on the sea (`river_deltas`, radius 6 + 5·width cells) get sediment
     (`deposit_sediment`): a low flat deposit plain on land and a shallow fan at sea.
  5. **Climate** (`climate.rs`): moisture = orographic rain on the final terrain + wet valley
     floors (drainage, blurred so single-cell flow lines don't show), temperature from
     altitude (`snow_line`), and a `Biome` per cell (beach, marsh, meadow, steppe, forests,
     alpine, rock, snow; `Biome::color()` is shared by previews and the game). Beach width
     varies along the coast (noise, 1–4.5 cells) and widens on river mouth sediment.
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
    `cargo test -p worldgen --release`.
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
  - **Edit layer** (`map/edits.rs`): `TerrainEdit::Flatten` (building foundations: flat over
    the inner 60% of the radius, smootherstep blend outside) and `Raise` (tools: ±10 units,
    smootherstep bell). Edits apply in insertion order and are found through a spatial grid.
    `Map::add_edit` records the touched chunks for re-meshing. `PatchOp::Smooth` is not
    supported yet (logs a warning).
- **Meshing** (`map/mesh.rs`): each 64-unit chunk is a quadtree of 16×16-quad patches. Cells
  go from 4 units down to `MIN_CELL` = 1/16. A node splits while its cell is larger than
  needed:
  - for the base terrain: curvature (max-curvature pyramid in `BaseGrid`) against an error
    tolerance of 0.0015 × camera distance, never finer than a quarter of the base cell;
  - for rivers / edits overlapping it: river width / 6, or edit falloff width / 8, relaxed to
    0.003 × camera distance.
  Patches don't share vertices across levels. Edge vertices next to a coarser leaf (found by
  re-running the split test from the chunk root, `leaf_at`) are snapped onto the coarser
  edge, so there are no T-junction cracks; skirts (2 cells deep) still hide cracks against
  chunks meshed for another camera position. Vertex colours = biome colour, darkened in
  river beds and under the sea. **UV0 is data**: the feature gradient (slope added by rivers
  and edits), which the shader adds to the gradient texture.
- **Chunks** (`map/mod.rs`): entities with `IsGround(coords)` + `TerrainChunk` LOD state,
  spawned within 6 chunks of the camera (never despawned). `schedule_chunk_meshes` starts
  at most 4 background mesh tasks, edited chunks first and then nearest. A chunk is re-meshed
  when an edit touched it, or when the camera moved by max(4, 25% of its distance) since its
  mesh was built. `finish_chunk_meshes` swaps the mesh asset in place. Mesh build times and
  vertex counts are logged at `debug` (`RUST_LOG=info,unnamed_factory=debug`): up to ~0.1 s /
  170k vertices per chunk on the eroded terrain.
- **Water** (`map/water.rs`): one translucent, double-sided `StandardMaterial` for a sea plane
  at 0, lake surfaces (lake cells plus shore cells above the water), and river ribbons at the
  water line (width + 0.25 per side). Ribbons stop where the water reaches the sea or the lake the
  river flows into (overlapping translucent surfaces look darker). Water has `Pickable::IGNORE` and `NotShadowCaster`.
- River center-line debug gizmos live in `dev_tools.rs` (F4, off by default).

### 3.3 Buildings (`build.rs`, `build_asset.rs`)
- `Building` is an `Asset` loaded from `.bconf` RON. All of them are loaded at startup
  through `load_folder("buildings")`, and the `Buildings` resource keeps the folder handle.
  `BuildingType` is `Single { model: Handle<WorldAsset>, scale }`, `Zone { color }` or
  `Tool { op: PatchOp, color }`. `size` and `script` exist but are unused.
- **Placement:**
  1. The build menu triggers `StartPlacement(handle)`. Its observer despawns whatever is
     being placed and spawns the new building with `SelectedBuild` and its visuals:
     `WorldAssetRoot` for models, a wireframe cuboid for zones, or a `ForwardDecal` for tools.
  2. For models, an entity observer on `WorldInstanceReady` computes the root `Aabb` once,
     from the mesh assets of the spawned glTF hierarchy.
  3. `build_follow_cursor` raycasts (`MeshRayCast`, filtered to `IsGround` chunks), snaps
     (`Snapping::spacing`), and records a left press made while placing and not over UI
     (`PlacementPress`). Zones resize while dragging from that press.
  4. `place_build` (chained after it) runs on release, **only if `PlacementPress` was set**.
     That way the menu click doesn't place anything. It adds a `TerrainEdit` (a flatten
     foundation, radius = 2× the footprint half-diagonal; or the tool's op, then despawns
     the tool), inserts a `BuildingInstance`, and registers it in
     `Map.entities`. With Ctrl held it triggers `StartPlacement` again.
  5. While placing, the `InfiniteGrid` build grid is shown at cursor height + 0.5 with the
     snapping spacing. It is a flat plane, so it disappears where the terrain rises above
     it. Drawing grid lines in the terrain shader would be the proper solution.
- **Selection** uses global picking observers: `PointerEnter` / `PointerLeave` on a
  `BuildingInstance` root toggle `Highlighted`, and a primary `PointerClick` picks the
  building up (removes `BuildingInstance`, inserts `SelectedBuild`). Nothing reacts while
  placing. `On<Add/Remove<Highlighted>>` observers move a single `SpotLight` onto the
  highlighted building.

### 3.4 Rendering (`shaders.rs`, `assets/shaders`)
`MapMaterial = ExtendedMaterial<StandardMaterial, TerrainShader>`, loaded from `.mapmat` RON
(`StandardMaterialParams` + a `tint`, hex colours through `deser_color`). `TerrainShader`
binds `tint` (100), the base surface gradient texture + sampler (101, 102) and its world →
uv mapping (103). `map::bind_gradient_texture` sets the texture once the material is loaded
(and again after a hot reload). The shader is WESL (`map_material.wesl`): base colour = vertex
colour × tint; normal per pixel = texture gradient + UV feature gradient, so lighting keeps
the full surface detail at any mesh LOD (vertex normals alone alias into a regular pattern on
coarse patches); bare rock blends in on steep slopes (1 − N.y from 0.3 to 0.55).

### 3.5 Simulation (`sim.rs`, `assets/scripts`)
- `RhaiScript` asset + loader. The `Sim` resource (`FromWorld` loads the scripts) holds the
  `rhai::Engine`, a `Scope` with one `data` map, and the numeric values keyed by path hash.
- `init.rhai` builds the nested `data` map: `job.<name>.{population, productivity, demand, ...}`,
  `stat.*`, `resource.*`, `aggregates.*`, `building.*`, `aims.*`. `init_sim` runs it as soon
  as it is loaded, and again on R. It then triggers `SimInitialized`.
- `run.rhai` is one tick of the sim (food, materials, migration, natality, per-job growth,
  demand, aggregates). `tick_sim` runs in **`FixedUpdate` at 20 Hz while Enter is held**.
  Script errors are logged, not propagated, since system errors panic in 0.20.
- The stats screen is built from the structure of `data` as nested `bsn!` scenes
  (`Box<dyn SceneBox>` for the recursion) by the `SimInitialized` observer. `update_stats`
  runs only when `Sim` changed.
- Building scripts (`Building.script`) are loaded but never executed.

### 3.6 UI (`ui.rs`)
The build menu is a `bsn!` scene, spawned once all buildings are loaded, with buttons sorted by
name. Buttons are `ui_widgets::Button` with an `on(start_placement)` observer on `Activate`,
styled from `Hovered` / `Pressed`. `label(text)` is the shared text scene; the font is an
asset path in the scene (`UI_FONT`). Scenes with assets are spawned with
`queue_spawn_scene`, which waits for the font. `PointerOverUi` (a `SystemParam`) tells camera
and placement code when the cursor is over UI. Mouse-wheel scrolling of `Overflow::scroll_y`
nodes is still manual (`update_scroll_position`), as in Bevy's own example.

### 3.7 Known issues / smells (fix opportunistically)
- `cargo check` passes with only dead-code warnings: unused data fields, `TestCurve`
  experiments, `ChunkMarker`.
- `bevy_dev_tools` is enabled for the in-game build grid. Bevy says not to ship dev tools;
  revisit (e.g. draw the grid in the terrain shader) before any release.
- Tests exist only for `worldgen`. The sim maths is pure and easy to unit-test too.
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
- `kdtree-collisions`: the user's own crate (github.com/Lamakaio/kdtree-collisions), 2018
  edition, no deps. It's free to modify, or to replace with a simple uniform grid / a
  maintained crate.
- Latest versions (2026-10-09): `rhai 1.26`, `ron 0.12` (Bevy no longer re-exports ron),
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
  src/dev_tools.rs       F-key debug toggles, gizmos (rivers, AABBs), wireframe, BRP — compiled only with `dev` feature
  src/terrain/{mod, chunk, material, streaming}.rs
  src/terrain/gen/{noise, hydrology, rivers}.rs   (pure Rust, unit-tested, no ECS)
  src/building/{mod, asset, placement, selection}.rs
  src/sim/{mod, script, ui}.rs
  src/ui/{mod, build_menu, widgets}.rs
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
- Custom terrain data should go in **custom vertex attributes** (`MeshVertexAttribute`) or a
  data texture, not in UV0 (the terrain mesher still writes height/wetness into UV0 for the
  existing shader).
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
   game saves a screenshot and exits, and you can then view the PNG. `UF_CAMERA=x,z[,dx,dy,dz]`
   points the start view at world (x, z) from offset (dx, dy, dz). Also scan the log for
   `ERROR` / `panicked`. To exercise interactive paths (placement, sim screen), apply a
   *temporary* patch (e.g. a fake cursor at window centre, or trigger `StartPlacement` from
   `auto_screenshot`), take the screenshot, then revert the patch. Never move the user's real
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

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

## 2. Repo map (as of commit `25565f6` + uncommitted river work)

```
Cargo.toml              bevy 0.20 (+ bevy_remote, trace_tracy, file_watcher), ron, serde, anyhow,
                        kdtree-collisions (user's own git crate, no bevy dep), noiz 0.5 (noise,
                        [patch]ed to a local Bevy 0.20 port — see §4.1),
                        rand/rand_distr, rhai 1.21, foldhash, fast_hilbert
rust-toolchain.toml     pinned nightly (nightly-2026-10-08). The user prefers nightly.
.cargo/config.toml      (git-ignored, local) rust-lld linker + `jobs = -4` to keep the PC usable while compiling
.cargo/config.toml      rust-lld linker on windows-msvc
notes.md                game design notes (see §1)
src/main.rs        290  App setup, CameraSettings, orbit camera, sun, debug toggles (F2 AABB, F3 wireframe, F rotate sun)
src/map.rs         563  MapPlugin, Map resource, Chunk (heightmap → mesh), terrain patch ops, chunk streaming, river gizmos
src/mapgen.rs      732  Continent: 2048² noiz heightmap (Hilbert-curve indexed), hydrology, river path tracing → CubicHermite curves, lakes
src/build.rs       520  BuildPlugin: Building asset type, placement following cursor, snapping (S), terraform tools, selection/highlight
src/build_asset.rs  87  BuildAssetPlugin: RON `.bconf` loader → Building asset
src/shaders.rs     231  ShadersPlugin: MapMaterial = ExtendedMaterial<StandardMaterial, TerrainShader>, RON `.mapmat` loader
src/sim.rs         277  SimPlugin: Rhai script asset + loader, runs init/run scripts, auto-generated stat UI (Tab)
src/ui.rs          205  UiPlugin: building list (scrollable), buttons that start a placement
assets/buildings/*.bconf   BuildingFile RON (name, size, typ: Single{model,scale} | Zone{color} | Tool{op,color}, script)
assets/materials/map.mapmat terrain material params (pbr + biome colors as hex)
assets/scripts/init.rhai, run.rhai  sim data init + one sim tick
assets/shaders/map_material.wesl   terrain fragment shader (WESL) (biome colouring from UV; based on bevy_wind_waker_shader)
assets/shaders/build_material.wgsl  unused (BuildMaterial is commented out)
```

### Controls (current)
Right mouse drag: orbit · Arrows: pan · Wheel: zoom · Left click: place / select ·
Ctrl+click: keep placing · S: cycle snapping · F: rotate sun · F2: AABBs · F3: wireframe ·
Tab: sim stats screen · Enter (held): run sim ticks · R: reset sim.

---

## 3. Current architecture (how it actually works)

### 3.1 App (`main.rs`)
`DefaultPlugins` with nearest-neighbour images, plus `WireframePlugin`, TAA, `RemotePlugin`
and `RemoteHttpPlugin` (for the BRP inspector). Feature plugins are `BuildPlugin`, `UiPlugin`,
`MapPlugin { seed }`, `ShadersPlugin`, `BuildAssetPlugin` and `SimPlugin`. The camera
(HDR, Bloom, TAA, `Atmosphere::EARTH`, `DistanceFog`, `AmbientLight` on the camera) and the
sun are spawned in `setup_3d`. The orbit camera is hand-rolled: a `CameraTarget` component
holds the focus point and distance, and the target is clamped to terrain height through
`Map::get_height`.

### 3.2 Terrain (`mapgen.rs`, `map.rs`)
- **`Continent`** is generated **synchronously inside `MapPlugin::build`**, which blocks
  startup. It holds 2048×2048 `TerrainPoint`s (height, gradient) indexed by **Hilbert
  curve** (`fast_hilbert::xy2h/h2xy`).
  - The height comes from a big composed `noiz` noise type (`NoiseT`), multiplied by an
    edge falloff.
  - Hydrology: each cell flows along its gradient. Sources are cells nothing flows into,
    culled with a kd-tree. Paths are traced (`go_through_path`), amounts are propagated,
    estuaries are grouped and forked, and control points become `CubicHermite<Vec3>`
    river curves (`RiverPath`). The terrain is then carved along the rivers
    (`patch_terrain_for_rivers`).
  - The `Hydrologypoint.visit` field reuses magic numbers (1, 2, 3, 12, 13) as per-pass
    markers, and `next == 0` / `tile == 0` stand for "none". This is fragile; prefer
    enums and `Option`.
- **`Chunk`**: 256×256 heights copied from the continent. `WORLD_CHUNK_SIZE = 255 *
  GRID_SQUARE_SIZE`, `SCALE_Y = 100`. Chunk → continent coordinates use
  `(chunk_pos * 255 + 1024).abs() % (2048-256)`, which tiles or mirrors the world past
  the continent. The same formula is duplicated in `build.rs::select_world_part`.
  - Mesh: a `TriangleList` with `u16` indices (65,536 vertices is exactly the u16 limit).
    UV0 is **abused as data**: `u` = height-derived biome coordinate and `v` = river flag
    (100 or 1). The terrain shader reads these values.
  - `Chunk::patch` edits heights in place (Up/Down/Flatten; Smooth is `todo!()`) on both
    the grid and the mesh attributes, and returns which neighbouring chunks also need
    patching.
- **`Map` resource**: material handle, `HashMap<I64Vec2, Chunk>`, a `KdTree<BuildingInstance>`
  of placed buildings, and the `Continent`. `get_chunk_mut` lazily creates chunks using
  hashbrown's `raw_entry_mut`. That still compiles, but the API is deprecated or removed
  in newer hashbrown; switch to `entry` (borrow `&self.continent` separately). `get_height` does bilinear sampling and returns `SCALE_Y` when the chunk is
  not loaded.
- `spawn_chunk` spawns a 4×4 block of chunks around the camera target when `CameraTarget`
  changes. Chunks are never despawned.
- `display_rivers` draws debug gizmos for the river curves and lakes every frame.

### 3.3 Buildings (`build.rs`, `build_asset.rs`)
- `Building` is an `Asset` loaded from `.bconf` RON. All of them are loaded at startup
  through `load_folder("buildings")`. `BuildingType` is `Single { model: Handle<Scene>,
  scale }`, `Zone { color }` or `Tool { op: PatchOp, color }`. `size` and `script` exist
  but are barely used.
- **Placement flow is an implicit state machine keyed on component presence:**
  1. The UI button spawns `(BuildId(handle), Name)`.
  2. `spawn_build_from_part_id` picks up `BuildId` entities `Without<Transform>` and inserts
     the visuals (`WorldAssetRoot`, a wireframe cuboid for zones, or a `ForwardDecal` for tools)
     plus `SelectedBuild`.
  3. `compute_aabb` merges the child AABBs into the root once the scene has spawned.
  4. `build_follow_cursor` raycasts (`MeshRayCast`, filtered to `IsGround` chunks), snaps,
     and handles drag-to-resize for zones.
  5. `place_build` on mouse release flattens the terrain under the building (or applies the
     tool op), inserts a `BuildingInstance` and registers it in `Map.entities`.
- **Selection:** `select_world_part` raycasts, walks `ChildOf` up to the root, highlights
  with a `Highlighted` marker, and observers move a single `SpotLight` ("highlight light")
  onto the target. It also `println!`s terrain and hydrology debug info for ground clicks.

### 3.4 Rendering (`shaders.rs`, `assets/shaders`)
`MapMaterial = ExtendedMaterial<StandardMaterial, TerrainShader>`. Five colours are bound as
separate uniforms (bindings 100–104) and loaded from `.mapmat` RON, which also carries
`StandardMaterialParams` (hex colours through `deser_color`). The shader is WGSL with
naga_oil `#import` / `#ifdef` and `@group(2)`, so it **must be ported to WESL for Bevy 0.20**
(§4).

### 3.5 Simulation (`sim.rs`, `assets/scripts`)
- `RhaiScript` asset + loader. `Sim` resource holds the `rhai::Engine`, a `Scope` with one
  `data` map, and handles to `init.rhai` / `run.rhai`.
- `init.rhai` builds the nested `data` map: `job.<name>.{population, productivity, demand, ...}`,
  `stat.*`, `resource.*`, `aggregates.*`, `building.*`, `aims.*`.
- `run.rhai` is one tick of the sim (food, materials, migration, natality, per-job growth,
  demand, aggregates). **Ticks run every frame while Enter is held**; there is no fixed
  timestep.
- The UI is auto-generated from the map structure (`spawn_on`). Values are refreshed by
  hashing the key path (`foldhash`) into `Sim.values`.
- Building scripts (`Building.script`) are loaded but never executed.

### 3.6 UI (`ui.rs`)
A left-side scrolling list of buildings, populated on `AssetEvent::LoadedWithDependencies`.
Buttons are `ui_widgets::Button` with an `Activate` observer (`start_placement`) and a
`Hovered`/`Pressed` styling system (ported from `Interaction` in the 0.20 upgrade). The font is
stored in a `FontHandle` resource.

### 3.7 Known issues / smells (fix opportunistically)
- On 0.20, `cargo check` passes with only unused-import / dead-code warnings (dead
  `TestCurve`, unread `ToolInstance` fields).
- `trace_tracy` is always on: it logs that memory grows until a Tracy client connects, plus a
  harmless `SymInitialize FAILED` line. Consider moving it behind an opt-in cargo feature.
- `assets/shaders/build_material.wgsl` is unused (BuildMaterial is commented out) and was never
  ported to WESL. Port it or delete it if BuildMaterial comes back.
- Many unused imports, commented-out code blocks and `pub mod` in a binary crate. Prefer
  private `mod` and `pub(crate)`.
- `unwrap()` / `FIXME` in systems (e.g. `buildings.get(&p.0).unwrap()`). Return `Result`
  instead (§5.6).
- `select_world_part` and `display_rivers` take `ResMut<Map>` where `Res` would do. This
  serializes the schedule needlessly.
- Debug output uses `println!` / `dbg!`; use `debug!` / `info!` (bevy `log`).
- Continent generation blocks app startup. Move it to `AsyncComputeTaskPool` behind a
  loading state.
- Sim speed is tied to frame rate. Use `FixedUpdate` with a configurable rate.
- No tests. `mapgen` and the sim math are pure and easy to unit-test.
- `Cargo.lock` is git-ignored. For a binary crate it **should be committed**.
- Dependencies use `"*"` versions. Pin them.

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
- The git-ignored local `Cargo.lock` held stale versions that broke builds (`tracy-client-sys`,
  `gpu-allocator`). Fix them with a targeted `cargo update -p <crate>`. Avoid a blind
  `cargo update`, since `rand = "*"` would jump to 0.10.
- 0.17: `TemporalAntiAliasPlugin` is in `DefaultPlugins`, so adding it again panics.
- 0.18: the WGSL `apply_decal_base_color` was removed (clustered decals now use `apply_decals`).
- 0.20: system errors panic through the fallback handler, so `query.single()?` on a
  possibly-empty query (e.g. a `Changed<..>` filter) crashes. Use `let Ok(..) = .. else`.

### 4.1 Toolchain & dependencies (all deps may be changed)
- Bevy 0.20 needs **rustc ≥ 1.97.1**. The toolchain is pinned to `nightly-2026-10-08` (the
  user prefers nightly). Bump the date deliberately. A recent-nightly lint required
  `#![recursion_limit = "256"]` in `main.rs` for the `AsBindGroup` derive.
- `noiz`: 0.2 → bevy 0.16, 0.3 → 0.17, 0.4 → 0.18, **0.5 → 0.19**. There was no 0.20 release on
  2026-10-09, so `Cargo.toml` uses `[patch.crates-io] noiz = { path = "../noiz-bevy-0.20" }`.
  That is a local clone (branch `bevy-0.20`, one commit) where `Curve`/`Interval`/`derivatives`
  imports moved from `bevy_math` to the new `bevy_curve` crate. **Once upstream publishes a
  Bevy 0.20 release, bump `noiz` and delete the patch.** (`C:/Users/timot/Documents/noiz` is a
  different, older clone of the user's with local edits. Don't touch it.)
  The noise type in `mapgen.rs` is a deep generic composition and may need adjusting
  across noiz versions.
- `kdtree-collisions`: the user's own crate (github.com/Lamakaio/kdtree-collisions), 2018
  edition, no deps. It's free to modify, or to replace with a simple uniform grid / a
  maintained crate.
- Latest versions (2026-10-09): `rhai 1.26`, `ron 0.12` (Bevy no longer re-exports ron),
  `rand 0.10` (`Rng`→`RngExt`, `RngCore`→`Rng`), `rand_distr 0.6`, `foldhash 0.2`,
  `fast_hilbert 2.1`, `thiserror 2` (Bevy examples use it for loader errors).
- Make `trace_tracy` an opt-in cargo feature, and put `file_watcher` and `bevy_remote` in a
  `dev` feature. Consider `bevy/dynamic_linking` for dev builds only.

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
- Keep **pure algorithms** (mapgen, hydrology, sim maths) free of ECS types so they can be
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
  data texture, not in UV0.
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

1. `git status` first. The user often has WIP (e.g. river generation in `mapgen.rs` / `map.rs`).
   Don't discard or reformat their uncommitted work.
2. Build: `cargo run` (dev profile opt-level 1, deps opt-level 3). The first build is slow. The
   game needs `assets/models/*.glb`, which is not in git and must be present locally.
3. When upgrading or changing Bevy APIs, check the **exact version's** docs.rs and the example
   at the matching git tag (`github.com/bevyengine/bevy/tree/v0.X.0/examples`). The Bevy book
   and the unofficial cheatbook lag behind.
4. Keep `notes.md` as the design source of truth. If an implementation decision diverges from
   it, say so to the user.
5. Commit in small steps (one migration step or one refactor per commit).

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

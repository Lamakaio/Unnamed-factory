# Living World: feasibility study and design draft

> Status: **draft, all decisions taken** (2026-10-13): small planet, two clocks, life =
> evolution over deep time, sphere first, plausibility (not Earth data) as the bar, city
> builder retired (§13). It answers: can terrain, erosion and climate be
> simulated *during* play instead of once at generation; can meshes follow; can the world be a
> spherical planet with plate tectonics and a full climate; and what has to be simplified.
> Decisions are collected in §13. Numbers marked *(est.)* are my extrapolations from this
> repo's measurements, not benchmarks.

---

## 0. Verdict (short)

| Wish | Feasible? | How (one line) | Effort |
|---|---|---|---|
| Dynamic climate (seasons, weather, climate change from parameters) | **Yes, cheap** | Reduced climate model on a coarse grid: energy balance + parametrised circulation + moisture advection | Medium |
| Terrain/erosion that keeps evolving | **Yes**, *if* time runs geologically fast (deep-time mode) or the rates are exaggerated | The stream power erosion we already have, stepped incrementally; dirty chunks re-meshed | Medium |
| Meshes updated at runtime | **Yes, already half there** | Edits already trigger background re-meshing of touched chunks; extend that to "heights changed" | Low → Medium |
| Spherical planet | **Yes**, at a **small, stylised planet** scale | Cube-sphere grid (6 square faces), so the existing 2D grid algorithms carry over | High (largest refactor) |
| Plate tectonics | **Yes**, procedural (not physical) | Rigid plates on the sphere, crust samples carried along, subduction / collision / rifting rules → uplift field | Medium |
| "Looks like our world with our parameters" | **Yes** | The bar is "looks right to an inexperienced viewer" (D5): a checklist of Earth's well-known patterns, checked on preview maps (§6.7) | Medium |
| Earth-sized planet with today's ground detail everywhere | **No** (not with a reasonable effort) | Would need multi-level streaming and amplification; see §2.1 option C | Very high |

The key simplifications that make it work:

1. **Separate the time scales** (§1): tectonics, landscape, climate, weather and life run at
   very different rates and are stepped by different clocks. Nothing is simulated at a rate
   where its change would be invisible.
2. **Separate the "physical" scale from the rendered scale** (§2.2): the planet is small in
   render units (curvature visible, float precision a non-issue), but the climate computes as
   if it were Earth-sized (latitudes, seasons, lapse rates behave like Earth's).
3. **Procedural, rule-based physics** everywhere it is cheaper and still produces Earth-like
   patterns: plates are rigid, circulation cells are placed rather than solved, ocean currents
   follow rules, erosion is the stream power law.
4. **One coupling chain**, each layer feeding the next through a few fields:
   tectonics → *uplift* → erosion → *relief* → climate → *rain, temperature* → erosion
   rate, rivers, lakes, ice, biomes → life.

---

## 1. Time scales: the core problem and the core simplification

Real rates (orders of magnitude): plates move centimetres per year (Pacific 7–11 cm/yr,
North America 2–3 cm/yr); mountains rise and erode at around a millimetre per year (Himalaya
≈ 5 mm/yr; denudation 0.05–2.5 mm/yr); landscapes re-equilibrate in ~10⁵ years after a
tectonic change. Seasons are a year, weather days, an animal's life years.

So "erosion during play" at human time scales is invisible. Three ways out (choice **D2**):

| Layer | Real time scale | Deep-time mode (fast-forward) | Normal play |
|---|---|---|---|
| Tectonics | 1–100 Myr | stepped, e.g. 1 step = 1 Myr *(est.)* | frozen |
| Landscape (uplift + erosion) | 10³–10⁶ yr | stepped, 1 step = 1–10 kyr | frozen, or local events only (landslides, river avulsion, coast retreat), exaggerated |
| Ice sheets, sea level | 10³–10⁵ yr | follows climate | slow drift if climate parameters change |
| Climate (means) | years–centuries | equilibrium recomputed per step | follows parameters with a lag |
| Seasons | 1 yr | averaged away | **visible**: snow, leaf colour, lake levels, river flow |
| Weather | days | ignored | **visible**: rain, storms, fog (stochastic on top of the climate) |
| Life | days → Myr | species-level (evolution) | individuals / populations |

**Decided (D2): two clocks.** A *game clock* (days, seasons, years: weather, vegetation,
lake levels, river discharge, life) and a *deep-time clock* that only advances when the
player fast-forwards (or during world generation, which becomes "replaying the planet's
history"). Geology then never needs to be fast, only incremental.

**With evolution as the life simulation (D3), deep time is the main mode, not a side
mode.** Species change over 10⁴–10⁶ years, the same rates as landscapes and climate
shifts, and plate motion over 10⁷ years splits and joins populations. So the player mostly
watches and steers the planet in fast-forward, and the game clock is for zooming in to look
at a moment (a season, a landscape, a species' range). Consequences:

- landscape and climate changes must be **continuous and smooth on screen** during
  fast-forward: heights blended over time in the vertex shader (§9.1, level 2) moves up from
  "later" to part of the core;
- tectonics matters for gameplay, not only for terrain: continents splitting isolate
  populations (allopatric speciation), land bridges and falling sea levels let species
  migrate, mountain ranges and deserts are barriers;
- climate is needed as long-term means and their drift (ice ages, warm periods), more than
  as weather.

This also gives world generation a story for free: generation *is* a deep-time run from a
random initial crust, so the planet has a history (where mountains came from, old eroded
ranges, ancient seas) that a life simulator can show.

---

## 2. Space: what kind of planet

### 2.1 Planet size (choice **D1**)

The current continent is a 512 × 512 coarse grid at 1 unit per cell (fine surface 0.25),
i.e. 262k cells, and one erosion step costs ≈ 45 ms (2.7 s for 60 steps, single-threaded).

On a cube-sphere with N × N cells per face and cell size c, the radius is R ≈ 2Nc/π.

| Option | Grid | Radius | Area vs today | Memory per f32 field | Erosion step *(est.)* | Notes |
|---|---|---|---|---|---|---|
| **A. Small planet** (**decided, D1**) | 6 × 1024², c = 1 | ≈ 650 units | ≈ 20× | 25 MB | ≈ 1.1 s single-threaded, a few hundred ms threaded | Whole planet simulated at one resolution; no streaming; f32 is enough everywhere |
| A′. Smaller | 6 × 512², c = 1 | ≈ 325 | ≈ 5× | 6 MB | ≈ 0.3 s | Fast iteration, strongly curved horizon |
| B. Medium | 6 × 4096², c = 1 | ≈ 2600 | ≈ 320× | 400 MB | many seconds | Needs region streaming for simulation and meshes |
| C. Earth-scale | global ~50 km grid + regional amplification | 6371 km | — | — | — | Two-level world: a coarse global sim and local terrain synthesised on demand (as in *Procedural Tectonic Planets* and *Terrain Amplification*). Consistency across tiles (rivers, lakes) and runtime changes are hard; floating origin required |

Option A keeps everything we have learned on the flat map (erosion at 1 unit, ×4 fine detail,
analytic river channels) and stays in memory. The horizon is close: from 50 units up you see
about √(2Rh) ≈ 250 units, which suits a stylised "little planet" look (and our toon style).

### 2.2 Physical scale vs render scale

Climate and tectonics use a **virtual Earth**: the planet *is* Earth-sized for them
(R = 6371 km, real insolation, real lapse rate ≈ 6.5 K/km, plate speeds in cm/yr).
Rendering uses the small radius. Two conversion factors:

- horizontal: 1 unit ≈ 6371 km / R (≈ 10 km for option A);
- vertical: chosen separately (e.g. 1 unit ≈ 200 m, so today's 38-unit ranges are ≈ 7.6 km
  peaks for the climate). That is a strong vertical exaggeration (~50×), which is exactly
  what a small stylised planet needs anyway.

The only things that must agree with the render scale are those the player sees close up:
valley widths, river widths, erosion features. Those stay in render units, as today.

### 2.3 Grid on the sphere

Options considered:

- **Lat-long grid**: cells collapse at the poles. Rejected.
- **Icosahedral / Fibonacci point cloud + Delaunay (Voronoi cells)**: most uniform, irregular
  neighbourhoods; used by Red Blob Games' planet generator and *World Orogen*. Good for
  plates, awkward for heightfield erosion, LOD and GPU textures.
- **Cube-sphere** (each cube face a square grid, projected): mild distortion with an
  equiangular projection, standard quadtree LOD per face, every algorithm we have works on
  a face unchanged; only neighbour lookups across face edges need special handling.

**Recommendation:** cube-sphere (equiangular) for every *field* (heights, climate, water,
biomes) and for rendering; a **moving point cloud** only for the tectonic crust (§4), which
is rasterised onto the cube grid each tectonic step.

The core new type in `worldgen`: `SphereGrid { n }` with `index(face, x, z)`,
`neighbours(i) -> [i; 8]` (wrapping across face edges and corners), `position(i) -> Vec3`
(unit direction), `cell_area(i)`. Priority-Flood, D8 routing, drainage accumulation, the
implicit stream power solver, diffusion and blurs are all written against "a graph of
cells with neighbours", so they port by swapping the neighbour function. (Corners where three
faces meet have 7 neighbours; D8 handles that if neighbours come from the grid type.)

---

## 3. Layers and data flow

```
            ┌──────────────┐  uplift U, crust type/age/thickness, volcanism
deep time   │  Tectonics   │──────────────────────────────┐
            └──────────────┘                              ▼
            ┌──────────────┐  rain, temperature, ice ┌──────────────┐
            │   Climate    │────────────────────────▶│  Landscape   │  heights (coarse)
            └──────────────┘◀────────────────────────│ uplift+erode │
                  ▲   ▲          heights, sea level   └──────────────┘
                  │   │                                     │
                  │   └─── ice / snow / vegetation albedo   ▼
game time   ┌──────────────┐   discharge, lake levels  ┌──────────────┐
            │  Seasons &   │──────────────────────────▶│  Hydrology   │ rivers, lakes,
            │   weather    │                           │              │ wetlands
            └──────────────┘                           └──────────────┘
                  │                                          │
                  ▼                                          ▼
            ┌──────────────────────────────────────────────────────────┐
            │  Biomes / vegetation (succession, with lag) → Life layer │
            └──────────────────────────────────────────────────────────┘
                                   │
                                   ▼  dirty regions
            ┌──────────────────────────────────────────────────────────┐
            │  Map: fine detail on demand, analytic channels, meshes   │
            └──────────────────────────────────────────────────────────┘
```

All layers live in the `worldgen` crate (pure Rust, no Bevy, testable, previewable as PNG
maps as today). The game owns a `Planet` resource holding the fields, runs the steps on the
`AsyncComputeTaskPool`, and swaps results in with a version number, so that render systems
see consistent snapshots (as `TerrainHeights` already does with `Arc`s).

---

## 4. Tectonics

### 4.1 What the references do

- **Procedural Tectonic Planets** (Cortial, Peytavie, Galin, Guérin, Eurographics 2019):
  procedural, not physical; approximates subduction and collisions deforming continental and
  oceanic crust; users steer plates; produces continents, ridges, ranges and island arcs;
  supports events like plate rifting; the coarse planet is then *amplified* with detail.
- **tectonics.js** (Carl Davidson): tracks oceanic crust age → density; dense old crust is
  marked for subduction; velocities pull crust toward subducting regions; plates are found by
  segmenting the velocity field; overlaps delete the subducting crust; gaps are filled with
  new crust (mid-ocean ridges).
- **PlaTec** (Viitanen; used by WorldEngine): fractal crust split into plates that move until
  friction stops them; collisions fold the crust.
- **World Orogen / Alien Worlds Generator** (browser, 2025–26): no time stepping, but a rich
  *classification* of plate boundaries (convergent / divergent / transform), with distance-
  field relief profiles: ridges, trenches, back-arc basins, island arcs, rift grabens,
  foreland basins, hotspot chains. Tectonics runs on a fixed ~20k-cell grid while the
  relief is computed at up to 2.5M cells.

### 4.2 Proposed model (simplified Cortial/tectonics.js hybrid)

State: ~20k–100k **crust samples** (Fibonacci sphere points), each with plate id, crust type
(oceanic / continental), thickness, age, elevation. Plates: rigid rotations about an Euler
pole (axis + angular speed) on the sphere.

One step (Δt ≈ 1 Myr *(est.)*):

1. Move samples with their plate's rotation.
2. **Divergence:** where a gap opens between plates, spawn new oceanic crust (age 0, ridge
   elevation); ocean floor deepens with age (√age subsidence, the classic ridge profile).
3. **Convergence**, per pair of overlapping samples:
   - oceanic vs continental → the oceanic sample subducts (removed); the overriding side
     gets uplift (volcanic arc / cordillera, like the Andes), a trench on the ocean side;
   - oceanic vs oceanic → the older (denser) one subducts; island arc on the other;
   - continental vs continental → no subduction: collision; crust thickens (suture,
     Himalaya-like uplift), plates slow down and eventually **merge**.
4. **Rifting:** occasionally split a large plate along a random weak line (great circle +
   noise); a rift valley forms, later an ocean. Triggers optional.
5. **Hotspots:** fixed in the mantle frame; plates move over them → island chains.
6. Plate speeds respond to forces in a cheap way: slab pull (more subducting boundary →
   faster), collision drag (slower).

Output to the landscape layer: an **uplift rate field U** on the cube grid (from thickening
and arcs), crust type (sets erodibility and isostatic base level), and a reference elevation
for the ocean floor. Isostasy (Airy) gives the base elevation from crust thickness: thick
continental crust floats high, so eroded mountains rebound a bit. This is a two-line rule.

**Coupling to erosion is the elegant part:** *Large Scale Terrain Generation from Tectonic
Uplift and Fluvial Erosion* (Cordonnier et al. 2016) and Schott et al. 2023 build terrains
from an **uplift map** + stream power erosion. Our `erode` already takes an uplift term `U`;
tectonics simply provides it instead of noise-based `Shape`.

Simplifications kept on purpose: no mantle convection, no plate deformation (rigid plates),
no realistic forces; plate motion is mostly scripted with a few feedbacks.

---

## 5. Landscape: uplift and erosion

### 5.1 What we have (reused as is)

`terrain.rs::erode`: stream power law dh/dt = U − K·A^m·S, solved implicitly upstream from the
outlets (Braun & Willett 2013, O(n) and stable at large time steps), hillslope diffusion,
Priority-Flood (Barnes 2014), stochastic D8, glacial overdeepening, lakes kept as basins,
thermal relaxation to a talus slope. It is *already* a time-stepped simulation; generation
just runs 60 steps.

### 5.2 What changes for a living planet

- **Rain-driven erodibility:** K·A becomes K·Q with Q = accumulated *runoff* (rain from the
  climate), so wet sides of ranges erode faster and rain shadows stay high and dry. That is
  the main climate → landscape feedback.
- **Sea level is a variable** (ice volume, §6.6): base level for erosion moves; coastlines,
  shelves, deltas move with it.
- **Sediment and deposition**: deltas, alluvial fans, plains. Today `mouths.rs` builds deltas
  procedurally; in a time-stepped world, deposition where slope drops (the FastScape
  sediment extension, or a simple capacity rule) builds them over time. Start with the
  procedural deltas, switch when needed.
- **Glaciers from the climate**: glacial erosion where the ice layer (§6.6) is thick, not by
  relative height as now.
- **Incremental updates**: a step touches every cell, but most cells change by less than the
  mesh error. Track |Δh| per chunk; only chunks whose accumulated change exceeds the LOD
  tolerance are re-meshed (§9).
- **Fast-forward variant**: *Physically-based Analytical Erosion* (Tzathas et al. 2024) solves
  the stream power law analytically so time is a parameter, not an iteration count. A good
  fit for "jump 1 Myr ahead". Later, if needed.
- **Detail amplification**: the fine ×4 surface (`detail.rs`) stays procedural and is
  generated per chunk on demand instead of for the whole map (on a planet the fine surface
  would be 100M samples). *Terrain Amplification using Multi-scale Erosion* (Schott et al.
  2024) is the reference if procedural gullies stop being enough.

Cost *(est.)*: option A, ≈ 1 s per step single-threaded. Priority-Flood is the sequential
part; drainage accumulation and the implicit solver parallelise per drainage basin. In deep
time, one step every few seconds of wall time is plenty.

---

## 6. Climate

Goal: Earth-like *patterns* (Köppen map, deserts at 20–30°, rain shadows, monsoons, west-coast
Mediterranean climates, cold eastern boundary currents, ice ages from orbital/CO₂ changes),
not weather forecasting. A real GCM (even ExoPlaSim, a fast intermediate-complexity 3D model)
is far too heavy and too opaque to tune. We use a reduced model in the spirit of energy
balance models (Budyko–Sellers) plus parametrised circulation, which is what *World Orogen*
does and tunes successfully against Earth.

Grid: cube-sphere, coarse: 6 × 128² ≈ 98k cells (≈ 1.4° on the virtual Earth). Time step:
a few days; one year ≈ 100 steps. *(est.)* well under a second per simulated year.

### 6.1 Inputs (the player-facing parameters)

Star luminosity, axial tilt, orbital eccentricity, day length (Coriolis strength), CO₂ /
greenhouse strength (a single "greenhouse" scalar on outgoing longwave), ocean fraction (via
sea level), planet's heat transport efficiency. Earth's values reproduce Earth.

### 6.2 Radiation and temperature

- **Insolation** per cell and day from latitude, declination (tilt, season) and eccentricity
  (standard daily-mean insolation formula).
- **Energy balance per cell:** C·dT/dt = S·(1 − albedo) − (A + B·T)·greenhouse + transport.
  Heat capacity C large over ocean, small over land: seasonal swing, continentality and the
  ~1 month seasonal lag emerge on their own.
- **Transport:** meridional diffusion (Budyko–Sellers) + advection by wind and by ocean
  currents. Coastal heat from currents is what makes western Europe mild.
- **Altitude:** lapse rate ≈ 6.5 K/km (moist lower, dry higher; *World Orogen* uses
  4.5–9.3 °C/km with humidity).
- **Albedo feedback:** snow/ice/desert bright, forest/ocean dark. This gives ice ages and
  "snowball" states (Budyko–Sellers' bistability) when the player turns the knobs.

### 6.3 Pressure and wind (diagnostic, not solved)

- Find the **thermal equator / ITCZ** per longitude (the warmest band; it follows the sun
  with a lag and swings far over continents: monsoons).
- Place pressure bands relative to it: ITCZ low, subtropical highs (~30°), subpolar lows
  (~60°), polar highs: the Hadley / Ferrel / polar cells. Add land–sea thermal pressure
  anomalies (summer lows over hot continents, winter highs over cold ones).
- Wind = pressure gradient, deflected by Coriolis (strength from day length), turned toward
  low pressure by surface friction (more over land and mountains). Result: trades, westerlies,
  polar easterlies, monsoon reversals.

### 6.4 Moisture and precipitation

- Evaporation from water surfaces (and wet ground / vegetation), increasing steeply with
  temperature (Clausius–Clapeyron).
- **Advect** atmospheric moisture along the wind (semi-Lagrangian), rain out when:
  - air rises over terrain (upslope wind · gradient; the *Smith & Barstad* linear theory of
    orographic precipitation is the well-tested version, with a cloud delay so rain falls
    a bit past the crest and the lee side dries: rain shadows);
  - air converges (ITCZ, fronts in the westerlies);
  - air cools below saturation (poleward flow).
- Subsidence in the subtropical highs suppresses rain: desert belts.

This replaces today's `climate.rs` moisture (orographic rain + valley wetness), which becomes
the special case of a fixed wind.

### 6.5 Ocean currents

A rule-based field, as in *World Orogen*: base flow from the wind belts; western boundary
currents intensified and poleward (warm), eastern boundary currents weaker and equatorward
(cold); circumpolar flow where no land blocks a latitude band; smoothed. A step up, if
needed: a Sverdrup/Stommel stream function per ocean basin (wind stress curl + beta effect
gives western intensification), still cheap.

### 6.6 Ice, snow, sea level

- Seasonal snow cover from temperature and precipitation (visible in game).
- Ice sheets: accumulate where snow survives the summer; very slow flow (diffusion of ice
  thickness); they raise the surface, erode (glacial valleys, fjords) and lower **sea level**
  by their volume. Sea level change is cheap to render (the sea is a shell of radius
  R + level) and is one of the most dramatic visible effects of a climate change.

### 6.7 Classification and validation

- Monthly means of temperature and precipitation → **Köppen–Geiger** (thresholds are
  public: e.g. tropical when every month ≥ 18 °C; arid from annual precipitation vs
  20·T + {280, 140, 0} by rain seasonality; polar when the warmest month < 10 °C). Köppen
  drives biomes, and its map is the main preview for checking the climate.
- **Validation (D5): plausibility, not Earth data.** The bar is "looks right to an
  inexperienced viewer", so no Earth datasets and no cell-by-cell scoring. Instead, preview
  maps (Köppen, temperature, rain, wind, currents, per season) are checked against the
  patterns everyone knows, with Earth-like parameters:
  - rainforest along the equator, desert belts around 20–30° (mostly on continents' west
    sides), temperate forests in the mid-latitudes, tundra and ice toward the poles;
  - wet windward and dry leeward sides of mountain ranges (rain shadows);
  - mild, wet west coasts and colder, drier continental interiors in the mid-latitudes;
    Mediterranean (dry summer) climates on west coasts around 30–40°;
  - monsoons where a large continent meets warm ocean;
  - snow lines lower toward the poles; seasonal snow on mid-latitude continents;
  - warm currents along east coasts flowing poleward, cold ones along west coasts.
  Some of these become automatic sanity tests on generated planets (e.g. mean rain by
  latitude band has its maximum at the equator and minima in the subtropics).

---

## 7. Hydrology at game time

- **River discharge** varies by season (snowmelt floods, dry-season low flow): ribbon width
  and depth follow discharge within the carved channel; foam increases in floods.
- **Lake levels** from the water balance (inflow + rain − evaporation − outflow): open lakes
  stay near their spill point, endorheic lakes rise and fall (and dry into salt flats in
  droughts). Today's lake machinery (Priority-Flood spill points, masks, contoured surfaces)
  is exactly what is needed: the level becomes a variable.
- **River network changes** only with the landscape (deep time) or rare events (avulsion,
  capture). Re-extract rivers regionally from the new drainage, and **match** new rivers to
  old ones (by mouth and trunk overlap) so identities, names and meshes are stable.
- Water meshes become per region (today: one mesh for all lakes, one for all rivers), so a
  change re-meshes only a region.

---

## 8. Biomes, vegetation and evolution

Biome = f(Köppen class, elevation, soil, wetness). Vegetation *cover* is a slow state with
succession (bare → grass → shrub → forest over years to decades), so climate change shows up
as moving forest edges and spreading deserts, not as instant recolouring. Vegetation feeds
back into albedo and evaporation (and into erosion: forests protect slopes).

*Status (2026-10-20), static for now (no succession, no ice flow):* biomes from the Köppen
class at each cell's height — rainforest, cloud forest, savanna, mangroves, deciduous and
conifer (taiga) forests, steppe, stony and sandy deserts (ergs with dunes), badlands, tundra,
alpine meadows, marshes and salt flats; landforms from rock and climate — rock types and
layered erosion, volcanoes (arcs, hotspot chains) with geothermal fields, reefs and atolls,
barrier islands, fjords, glaciers and ice caps, sea ice, karst. Details in AGENTS.md §3. Not
yet: tidal flats and salt marshes, loess, permafrost patterning, moving ice.

**Evolution (D3)** sits on top, at the deep-time clock. Designed in `docs/evolution.md`;
what the planet must provide to it:

- per cell and per epoch: climate means (temperature, rain, seasonality), biome, elevation,
  land/sea, ice; their *rate of change* (selection pressure);
- connectivity: which cells a population can reach (land, sea, rivers as corridors,
  mountains and deserts as barriers), recomputed as plates move and sea level changes;
- history: events with dates (rift opened, range uplifted, ice age began) to explain to the
  player why a species split or died out.

A plausible shape, by analogy with the planet layers: populations as fields over the coarse
grid (density per species per cell), traits per species (body plan, size, climate tolerances,
diet), speciation when a population's range splits and stays apart long enough, extinction
when its range shrinks to nothing. Individuals only appear when the player zooms in.

---

## 9. Rendering and runtime updates in Bevy

### 9.1 "Can meshes be modified at runtime, and how?"

Yes. There are three levels, from what exists to what scales best:

1. **Re-mesh dirty chunks on the CPU (exists today).** Terrain edits (`edits.rs`) record the
   chunks they touch; `schedule_chunk_meshes` re-meshes them in background tasks (≤ 4 at a
   time, edited first) and `finish_chunk_meshes` swaps the mesh asset in place. A chunk
   build is ≤ ~0.1 s. A dynamic landscape plugs into the same path: when the height field
   version changes, compare per chunk and mark as dirty only those whose change exceeds the
   mesh error at their LOD. Good for local, occasional changes (landslides, a river cutting
   a new bend, a season's snow line is shader-only anyway).
2. **Displace a flat grid in the vertex shader from a height texture (recommended once
   changes become continuous).** Meshes become flat LOD patches; heights live in textures
   (one per cube face or per region). Updating terrain = updating texels (an `Image` asset
   change, or a write from a compute pass); no CPU meshing, and the shader can **blend two
   height states over time**, so deep-time erosion animates smoothly instead of popping.
   We already bake the base gradient into a texture for normals, so half of this exists.
   The CPU keeps its own copy of the heights for gameplay (placement, picking, pathing).
3. **Simulate on the GPU** (compute shaders; Bevy's `gpu_readback` example shows reading
   storage buffers back with the `Readback` component). 10–100× faster for grid-parallel
   steps (diffusion, climate, thermal erosion); the Priority-Flood / drainage tree is not
   GPU-friendly. Only for proven hot spots, later.

Notes: in Bevy 0.20 `Assets::get_mut` returns an `AssetMut` that only marks the asset
modified when written through, so swapping meshes does not cause spurious re-uploads.
Meshes that must stay editable on the CPU keep `RenderAssetUsages::MAIN_WORLD`.

### 9.2 Planet rendering

- **Cube-sphere chunk quadtrees**: each face is today's chunk grid; a vertex at face (u, v)
  with height h goes to `direction(face, u, v) * (R + h)`. The LOD error metric, skirts and
  T-junction snapping carry over; face edges need the neighbour face's LOD.
- **Precision**: with R ≈ 650 units, f32 has sub-millimetre precision at the surface: no
  floating origin needed (it would be for option C; `big_space` is the Bevy crate for that).
- **Local frames**: "up" becomes the radial direction. Camera (pan-orbit around a point on
  the surface, aligned to local up), placement, vegetation and buildings use a local tangent
  frame. Lighting: the sun direction is global, so day/night and seasons fall out naturally.
- **Water**: the sea becomes a sphere shell at R + sea level (the depth-based shore foam
  keeps working); rivers and lakes as today, with positions lifted onto the sphere.
- **From orbit**: a low-detail whole-planet mesh (or the coarsest quadtree levels) with the
  biome/climate colours; this is also where climate overlays (temperature, rain, Köppen,
  plates) are shown.

### 9.3 What happens to the current analytic features

Analytic river channels, lake contours, springs, deltas and the fine detail are evaluated
per sample, so they keep working on a sphere if they are expressed in a face's local 2D
coordinates. They are rebuilt per region when hydrology changes, not every frame.

---

## 10. Performance budget *(est.)*

| Work | Grid | Cost | Frequency |
|---|---|---|---|
| Climate step | 6 × 128² | ms | 100 per simulated year; in play: a few per in-game day |
| Seasonal visuals (snow, colours, lake levels) | per chunk | shader uniforms / small texture updates | continuous |
| Tectonic step | 20k–100k samples | tens of ms | deep time only |
| Erosion step | 6 × 1024² | ~1 s single-threaded, less threaded | deep time only (+ local events) |
| River/lake re-extraction (regional) | region | 0.1–1 s | after landscape changes |
| Chunk re-mesh (CPU path) | chunk | ≤ 0.1 s | per dirty chunk, 4 in parallel |
| Generation (history replay) | all | tens of seconds to minutes | once, with a progress screen |

Memory: ~10–20 coarse fields at 25 MB (f32; f16 where precision allows) for option A.

---

## 11. Migration path (milestones)

Each step leaves the game running and previewable.

1. **Sphere grid in `worldgen`**: `SphereGrid`, port Priority-Flood / routing / erosion /
   lakes / rivers to graph form; the `preview` example writes equirectangular and per-face
   PNGs. Noise-based uplift (today's `Shape`) on the sphere first.
   Also: retire the city builder (D6) so the planet work does not have to carry it.
2. **Planet in the game**: cube-sphere chunks, radial heights, local frames, camera, sea
   shell, rivers/lakes lifted onto the sphere, vegetation and buildings on local frames.
   (Largest refactor: the `map/` module and everything assuming y-up flat ground.)
3. **Climate v1**: insolation, energy balance, ITCZ + pressure bands, wind, moisture
   advection, orographic rain, rule-based currents, monthly means, Köppen, biomes.
   Plausibility checklist and latitude-band sanity tests (§6.7).
4. **Tectonics v1**: plates on the sphere, crust samples, boundaries → uplift; generation =
   deep-time history replay (tectonics + erosion + climate).
5. **Deep-time mode** (the main mode, D3): fast-forward with continuous display,
   vertex-shader heights blended between states, player knobs (CO₂, tilt, luminosity), ice
   sheets and sea level, per-region water meshes, event history.
6. **Evolution v1**: species as population fields with traits, dispersal over the
   connectivity graph, speciation and extinction (own design document).
7. **Game-time zoom**: game clock, seasons (snow, foliage, lake levels, discharge), weather,
   individuals of the species present.

The flat continent can stay as a "region" mode during steps 1–3, since a cube face *is*
a flat grid.

---

## 12. Risks

- **Scope**: each layer can absorb unlimited polishing. Mitigation: "looks right in the
  preview PNGs" gates, with the plausibility checklist (§6.7) for the climate.
- **Sphere seams**: off-by-one errors at cube edges and corners (routing, rivers crossing
  faces, LOD). Mitigation: all neighbour logic in one grid type, tested with a "drop water
  everywhere, it must reach the sea" test.
- **Stability of identities under change**: rivers, lakes, biomes flickering when the world
  updates. Mitigation: hysteresis and matching (§7).
- **Gameplay vs simulation time**: a life sim needs the world to change at a pace the player
  perceives. The two-clock design is the answer, but the right "exaggerations" will need play
  testing.
- **The city-builder code** (`build.rs`, `sim.rs`, buildings, zones) assumes a flat y-up
  world. Retired for now (D6): it stays in git history and can come back as something in
  the life sim later.

---

## 13. Decisions

Decided on 2026-10-13:

- **D1. Planet size**: small planet, option A (6 × 1024², R ≈ 650 units).
- **D2. Time model**: two clocks (game clock; deep-time clock for geology).
- **D3. Life simulation**: evolution over deep time (species, populations, speciation),
  which makes deep time the main mode (§1).
- **D4. Order of work**: sphere first (milestones 1–2), then climate, tectonics.

- **D5. Validation**: no Earth datasets. Exact Earth likeness is not a goal; the bar is
  "looks right to an inexperienced viewer" (§6.7).
- **D6. City-builder code**: retired for now (buildings, zones, Rhai city sim, build tools,
  UI). Removed from the app at the start of milestone 1; it stays in git history.

---

## 14. References

Tectonics and planets
- Cortial, Peytavie, Galin, Guérin. *Procedural Tectonic Planets*. CGF 38(2), 2019.
  https://diglib.eg.org/handle/10.1111/cgf13614
- tectonics.js (Carl Davidson). https://github.com/davidson16807/tectonics.js
- PlaTec / PyPlatec (Lauri Viitanen). https://pypi.org/project/PyPlatec/
- WorldEngine (plates via pyplatec, erosion, precipitation, Holdridge biomes).
  https://www.smashingmagazine.com/2016/03/procedural-content-generation-introduction
- Red Blob Games, *Planet generation* and *Delaunay + Voronoi on a sphere*.
  https://www.redblobgames.com/x/1843-planet-generation/ ·
  https://www.redblobgames.com/x/1842-delaunay-voronoi-sphere/
- World Orogen (tectonics, erosion, climate, Köppen, Earth-tuned).
  https://github.com/raguilar011095/planet_heightmap_generation
- Alien Worlds Generator (same family, other planet types).
  https://github.com/bartsr94/Alien-Worlds-Generator
- Cube vs icosahedral planets: https://acko.net/blog/making-worlds-1-of-spheres-and-cubes/

Erosion and terrain
- Braun & Willett 2013, implicit O(n) stream power (FastScape).
  https://doi.org/10.1016/j.geomorph.2012.10.008
- Cordonnier et al. 2016, *Large Scale Terrain Generation from Tectonic Uplift and Fluvial
  Erosion*. https://hal.archives-ouvertes.fr/hal-01262376
- Schott et al. 2023, *Large-scale Terrain Authoring Through Interactive Erosion
  Simulation*. https://github.com/H-Schott/StreamPowerErosion
- Schott et al. 2024, *Terrain Amplification using Multi-scale Erosion*.
  https://research.adobe.com/publication/terrain-amplification-using-multi-scale-erosion
- Tzathas, Gailleton, Steer, Cordonnier 2024, *Physically-based Analytical Erosion for fast
  Terrain Generation*. https://diglib.eg.org/handle/10.1111/cgf15033
- Mei, Decaudin, Hu 2007, *Fast Hydraulic Erosion Simulation and Visualization on GPU*
  (virtual pipes). https://hgpu.org/?p=2256
- Musgrave, Kolb, Mace 1989, thermal erosion / talus. https://history.siggraph.org/?p=119354
- Beyer's droplet erosion (as implemented by Sebastian Lague and erodr).
  https://github.com/henrikglass/erodr

Climate
- Budyko–Sellers energy balance models: https://arxiv.org/pdf/1105.4918
- Smith & Barstad, linear theory of orographic precipitation; Python implementation:
  https://github.com/fastscape-lem/orographic-precipitation
- ExoPlaSim (fast intermediate-complexity GCM; too heavy for us, useful for comparison).
  https://arxiv.org/pdf/2107.07685
- Ocean gyres and western intensification: https://en.wikipedia.org/wiki/Ocean_gyre
- Atmospheric circulation (Hadley/Ferrel/polar cells, ITCZ):
  https://geo.libretexts.org/Bookshelves/Oceanography/Oceanography_101_(Miracosta)/08%3A_Atmospheric_Circulation/8.12%3A_Earth's_Atmospheric_Circulation_System
- Köppen classification thresholds: https://simple.wikipedia.com/wiki/K%C3%B6ppen

Rates
- Plate speeds, uplift and erosion rates, landscape response times: see the sources collected
  in the research notes of 2026-10-13 (USGS figures; Southern Alps, Himalaya, Alps studies).

Bevy
- GPU readback example: `examples/shader/gpu_readback.rs` in the Bevy repository.
- `big_space` (floating origin; only needed for an Earth-scale option).

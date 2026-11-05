# Continuous world: a planet that changes while you watch

> Status: **plan for discussion** (2026-11-06). How the planet can evolve continuously in
> deep time (plates drifting, ranges rising and wearing down, seas rising and falling, ice
> advancing, volcanoes and craters coming and going, climate and biomes moving) instead of
> being regenerated at each age. Nothing done so far is treated as fixed: the plan says what
> changes, and what is lost or simplified. Open choices in §9.

---

## 0. Summary

- **Regenerating is out of the question**: generation takes 24 s at the game's resolution
  (6 × 1024² cells), and most of it is global work (flow routing over the whole planet:
  2.4–3 s a pass, lakes 2.6 s, surface fields 3 s, climate 1.9 s, sea waves 1.4 s).
- **Split the world into a slow *state* and a fast *look***. The state is coarse (about
  6 × 256² cells, 16× fewer) and is what the simulation steps: crust and plates, elevation,
  sediment, ice, sea level, drainage, climate, features as objects with lifetimes. The look is
  derived from it at full resolution by cheap, local, parallel operations: upsampling, detail
  carried with the crust, procedural detail, feature stamps.
- **The renderer draws from textures, not meshes**: heights and ground materials per cube
  face in GPU textures, displacing fixed flat patches in the vertex shader. Two *keyframes*
  of the look are kept and blended by time on the GPU, so the planet changes smoothly every
  frame while the next keyframe is computed in the background (as the clouds already do).
- **Two qualities**: while time runs, keyframes are coarse in their hydrology and details
  (rivers and lakes as fields, little vegetation); when time stops, a background *refine*
  pass rebuilds today's full detail (river meshes, deltas, vegetation, sea waves) for the
  frozen state, in a few seconds, and swaps it in.
- **Generation stays** as the way to make the starting planet (always a virgin one, V4) and
  previews at an age; at the start the generated world is decomposed into state + detail.
  An evolved world and one generated at the same age differ (accepted).

---

## 1. What changes at which rate

The deep-time clock (living-world §1) advances by **ticks** of 0.1–1 Myr at a speed the
player chooses. Per tick:

| Process | Real rate | Over one tick (1 Myr) | Visible? |
|---|---|---|---|
| Plate motion | 2–10 cm/yr | 20–100 km: 2–10 world units | over tens of ticks |
| Uplift of ranges | ~1 mm/yr | 1 km: 10 height units | yes |
| Erosion of relief | 0.05–1 mm/yr | 50 m–1 km | slow |
| Sea level | ±100 m in 100 kyr (ice ages) | many cycles | flickers unless averaged |
| Ice sheets | 10⁴–10⁵ yr | many cycles | follow the climate's slow trend |
| Climate means | follows the gases and the star | changes | yes |
| Volcanoes, craters | events | appear, age, vanish | yes |

So a tick is the natural step for tectonics, uplift and erosion; sub-tick cycles (ice ages,
seasons) are shown as their envelope, or only when time is slowed down.

---

## 2. The state (what the simulation steps)

A coarse cube-sphere grid, **6 × 256²** (393 k cells, ~4 world units each), plus objects:

| Field / object | Today | In the state |
|---|---|---|
| Crust: plate of each cell, thickness, age, orogeny | `tectonics.rs` (6 × 96², run for 50 steps at generation only) | kept and **stepped forever**, at 6 × 128–256²; plates' own rasters as today (crust rides with its plate) |
| Elevation (bedrock + sediment) | shaped, then eroded at 1024 | coarse elevation, stepped by uplift and erosion |
| Rock type, strata | per cell, from the provinces | carried in the plate rasters (rock rides with its crust); new crust gets rock by its origin |
| Detail (see §3) | baked into the 1024 heights | a separate field carried with the crust, wearing down |
| Ice thickness | glaciers from the climate, static | a coarse ice field grown and melted by the climate each tick |
| Sea level | fixed 0 | a global level from the water amount, ice volume and ridge volume |
| Drainage, lakes | 1024 priority flood + routing | coarse routing and lake levels each tick |
| Climate | `climate.rs` at 6 × 48², full spin-up | warm-started from the last tick (1–2 simulated years, not 7): ~0.3 s, run every few ticks in the background, blended |
| World parameters | `World` | stepped by `history.rs` (CO₂, events) |
| Volcanoes, craters, reefs | lists / fields made once | **objects with a birth date**, ageing, carried by their plate, removed when worn away; new ones from the history's events (eruptions, impacts) |

Costs per tick *(est.)*, background threads: tectonics step and plate rasters at 6 × 128²:
tens of ms; uplift + implicit stream-power erosion at 6 × 256² (routing ~1/16 of today's
2.5 s): ~0.2 s; coarse lakes ~0.1 s; climate every 5 ticks ~0.3 s. **A tick in about
0.3–0.5 s** on the cores we have; a keyframe per tick or every few ticks.

### Changes to existing code

- `tectonics.rs` becomes a stepper (`TectonicState::step`) used both by generation (its
  current 50 steps) and by the running world; its rasters also carry rock and detail.
- `terrain.rs` erosion gets an incremental `step(state, uplift, dt)` on the coarse grid (it
  already is a stepped implicit stream power: the change is running it on a persistent state
  instead of a fresh one, and with uplift from the tectonics instead of the shaping noise).
- `planet/shape.rs` (provinces, plateaus, belts from the history) is used for the start; in
  the running world uplift comes only from the tectonics (collisions, subduction, rifts,
  hotspots), which is how the Cortial et al. model works anyway.
- `volcanoes.rs`, `craters.rs`, `reefs.rs`: their shapes become functions of an object and
  an age (they mostly already are: `Volcano::age`, `Crater::freshness`), evaluated when the
  look is composed, not baked into the heights.
- `history.rs` drives the gases, snowballs and events tick by tick (it already steps at
  0.1 Myr).

---

## 3. The look (what the renderer draws), composed each keyframe

Full resolution (6 × 1024²), every operation local and parallel, no global passes:

1. **Coarse elevation upsampled** (bicubic, as `FaceField` already does).
2. **Carried detail**: at the start, the difference between the generated 1024 heights and
   the coarse ones (valleys, ridges, the fine erosion's work) is stored in each plate's
   raster at full resolution and moves with its plate. It is scaled down as erosion wears
   the relief (where the coarse elevation drops, the detail fades) and scaled up where young
   uplift sharpens it. This keeps today's look at the start and lets it age plausibly.
3. **Grown detail** where none was carried (new crust, new ranges): procedural ridges and
   valleys oriented by the coarse drainage (flow-aligned ridged noise, the "amplification"
   idea of Schott et al. 2024), so new mountains look eroded without running the fine
   erosion.
4. **Feature stamps**: volcanoes, craters, reefs evaluated from their objects and ages.
5. **Hydrology as fields**: lake levels and river widths from the coarse drainage, carved as
   shallow channels into the composed heights (a cheap version of `channels.rs`).
6. **Surface fields** (the inputs of today's terrain shader: dryness, forest, snow, marsh,
   rock, ice, ...) from the climate and the composed heights: the per-cell parts of
   `PlanetSurface::new`, which are local; the global ones (riparian distance, sea-ice blur,
   ergs) are approximated or computed coarse and upsampled.

*(est.)* 6 × 1024² cells × a few operations: ~50–100 ms in parallel. The output goes
straight into textures.

---

## 4. Rendering: from meshes to textures

Today: chunk meshes built on the CPU from `PlanetSurface` (1536 chunks × LOD levels, ≤0.1 s
each), vertex attributes carrying the ground materials; water meshes (rivers as ribbons,
lake contours, sea shells per face) built once.

Proposed (living-world §9.1, level 2, which it already recommended once changes are
continuous):

- **Terrain**: per cube face, a height texture (R32F, 1024² + a border) and material textures
  (2 × RGBA8: the current vertex colour and cover channels), for two keyframes. The chunk
  quadtree keeps its LOD selection but every chunk is the same flat grid mesh, displaced in
  the vertex shader by `mix(height_a, height_b, t)`; normals from the height texture's
  gradient. No CPU meshing at all, so a keyframe costs only the texture upload (~60 MB for
  both keyframes and all faces).
- **Shadows**: the shadow pass uses the same vertex shader (Bevy's prepass with our vertex
  shader).
- **Sea**: one sphere at the sea level (a uniform: it moves with the state). The sea's wave
  field (swell travel time, 1.4 s) is recomputed only by the refine pass; while time runs,
  waves without the shore refraction.
- **Lakes and rivers** while time runs: drawn by the terrain shader from a water texture
  (depth, flow direction) instead of meshes. On refine, today's river and lake meshes come
  back.
- **Vegetation**: instances placed per chunk from the material textures, as today, but
  re-placed only on refine; while time runs, trees fade (dither) out where the forest weight
  falls and the ground colour carries the biome. Later: GPU-placed instances.
- **Picking, camera clearance, click reports**: read the CPU copy of the composed heights.

### Lost or simplified (while time runs)

- Analytic river channels, meanders, anabranches, deltas and estuaries (`mouths.rs`):
  coarse channels only; full detail on refine.
- Fine erosion re-running on the changed land: replaced by carried + grown detail. New
  valleys don't cut into old detail; old detail fades instead.
- Today's exact LOD meshes with skirts and baked attributes: replaced by displaced patches
  (cracks at LOD borders handled by skirts in the shader or by stitching).
- Per-vertex features baked from global passes (riparian distances, blurred fields):
  coarser.
- Determinism between "generated at age X" and "evolved to age X" (accepted).

---

## 5. Keyframes, blending, refine

- The simulation thread advances ticks; every keyframe interval (e.g. 1 tick) it composes the
  look and hands it to the render thread, which uploads it as keyframe B while A is shown;
  the GPU blends A → B over the interval (like the clouds' two snapshots).
- Things that appear or vanish (a volcano, a lake) fade in or out with the blend.
- Identity and flicker (living-world §12): sea level and ice from the history's slow trend,
  not its fast cycles; lakes and rivers matched from keyframe to keyframe (a lake keeps its
  id while its outlet stays) so they don't pop.
- **Refine on pause**: when the clock stops, a background job runs today's full pipeline
  steps that are affordable for a frozen state (rivers and lakes at 1024 from the composed
  heights, mouths, surface fields, sea waves, vegetation: ~8–10 s), then swaps in.
- Speeds: from 0.1 Myr per keyframe (watching a volcano grow) to 10 Myr per keyframe (plates
  visibly drifting), with the tick size adapting.

---

## 6. Generation in this scheme

- A new game = generation (as today, possibly shorter: §9 V8) → **decompose** into state
  (coarse elevation, plate rasters with rock and detail, feature objects) → run.
- The tectonic history at generation (`TectonicSettings::steps`) and the running world use
  the same stepper, so the start and the continuation agree.
- Previews (`planet_preview`, `climate_preview`) keep working on generated worlds; a new
  `evolve_preview` writes map frames of a running world (a PNG per keyframe) for headless
  checks before the renderer exists.

---

## 7. Milestones

Each leaves the game runnable.

1. **C1 – State and stepper, headless.** Coarse state, persistent tectonics stepper, uplift
   from tectonics, coarse erosion step, sea level, features as aged objects, history-driven
   gases and events. `evolve_preview` frames (maps every N ticks) to check stability and
   motion. No renderer change.
2. **C2 – Composition.** The look from the state: upsampling, carried detail in the plate
   rasters (from the generated world), grown detail, stamps, coarse hydrology, surface fields.
   Check: at tick 0 the composed look is close to the generated planet.
3. **C3 – Texture terrain.** Heights and materials in textures, displaced patches,
   normals from the texture, shadows. Parity with today at a frozen state (screenshots).
4. **C4 – Keyframes.** Simulation thread, two-keyframe blend, sea level uniform, water from a
   texture while running, vegetation fade; a deep-time clock (speed, pause) in the game.
5. **C5 – Refine on pause.** Today's full rivers, lakes, mouths, vegetation, waves rebuilt for
   the frozen state.
6. **C6 – Climate and ice running.** Warm-started climate every few ticks, ice sheets grown
   and melted, biomes moving; clouds' sky field rebuilt with it.
7. **Later**: life's fields on the same ticks (E3's life part), GPU erosion (below).

---

## 8. Alternatives considered

- **GPU erosion at full resolution** (Schott et al. 2023 run stream power interactively on
  the GPU with a parallel flow approximation): would keep fine erosion running, so new
  valleys cut properly. More effort (compute shaders, readback for the CPU copy), and the
  flow approximation differs from our priority-flood routing. A good later upgrade to C2's
  grown detail; not needed to start.
- **Regional regeneration** (re-run today's pipeline only where things changed): everything
  changes a little everywhere in deep time, and the global passes (routing, lakes) don't
  localise well. Rejected.
- **Keeping CPU meshes and re-meshing on each keyframe**: 1536 chunks × LOD × ≤0.1 s each
  is far too slow for continuous change; only viable for rare local edits.
- **Lower resolution while running** (render 6 × 512² during fast-forward): a cheap fallback
  if C3/C4 run short of budget.

---

## 9. Decisions to take

- **V7. Coarse state resolution**: 6 × 128² (cheapest, ~16 units a cell) or **6 × 256²
  (recommended)**: ranges and coasts still readable at the coarse level.
- **V8. Generation**: keep today's full generator for the start (**recommended**: the start
  looks as good as today), or replace it by running the stepper from a blank crust (simpler,
  one code path, but the start would look like the running world).
- **V9. While time runs**: accept coarse rivers and lakes and sparse vegetation, full detail
  on pause (**recommended**), or keep vegetation and rivers updated continuously (much more
  work: GPU-placed vegetation, river fields in the terrain shader at full quality).
- **V10. Fast-cycle display**: show ice ages and sea-level swings as they happen when slowed
  down (**recommended**), or only their long-term trend.

---

## References

- Cortial, Peytavie, Galin, Guérin 2019, *Procedural Tectonic Planets* (uplift from plate
  interactions, continuous): https://diglib.eg.org/handle/10.1111/cgf13614
- Cordonnier et al. 2016, *Large Scale Terrain Generation from Tectonic Uplift and Fluvial
  Erosion*: https://hal.archives-ouvertes.fr/hal-01262376
- Schott et al. 2023, *Large-scale Terrain Authoring Through Interactive Erosion Simulation*
  (GPU stream power): https://github.com/H-Schott/StreamPowerErosion
- Schott et al. 2024, *Terrain Amplification using Multi-scale Erosion*:
  https://research.adobe.com/publication/terrain-amplification-using-multi-scale-erosion
- Braun & Willett 2013 (implicit stream power, the erosion step):
  https://doi.org/10.1016/j.geomorph.2012.10.008
- Vertex-shader terrain from height textures with chunked LOD: e.g. Strugar, *Continuous
  Distance-Dependent Level of Detail for Rendering Heightmaps* (CDLOD), 2009.

# Credits and inspiration

Techniques, papers and code this project builds on. When adding a technique inspired by
someone else's work, add it here (and mention it in the code comment where it is used).

## World generation (`crates/worldgen`)

- **Stream power erosion, implicit solver**: J. Braun and S. D. Willett, *A very efficient
  O(n), implicit and parallel method to solve the stream power equation governing fluvial
  incision and landscape evolution*, Geomorphology 180–181, 2013. (`terrain.rs::erode`)
- **Uplift + fluvial erosion for large-scale terrain**: G. Cordonnier, J. Braun, M.-P. Cani,
  B. Benes, É. Galin, A. Peytavie, É. Guérin, *Large Scale Terrain Generation from Tectonic
  Uplift and Fluvial Erosion*, Computer Graphics Forum (Eurographics) 2016. The overall
  approach of shaping uplift (here from tectonic provinces) and letting erosion carve it.
- **Priority-Flood** depression filling: R. Barnes, C. Lehman, D. Mulla, *Priority-flood: An
  optimal depression-filling and watershed-labeling algorithm for digital elevation models*,
  Computers & Geosciences 62, 2014. (`flow.rs`)
- **Stochastic D8 flow routing** (against grid-aligned valleys): J. Fairfield and
  P. Leymarie, *Drainage networks from grid digital elevation models* (the Rho8 method),
  Water Resources Research 27(5), 1991.
- **Thermal erosion / talus slopes**: F. K. Musgrave, C. E. Kolb, R. S. Mace, *The synthesis
  and rendering of eroded fractal terrains*, SIGGRAPH 1989. (`terrain.rs::limit_slopes`)
- **Gully detail ("erosion noise")**: Clay John's eroded terrain noise and Fewes' erosion
  shader (Shadertoy), and runevision's (Rune Skovbo Johansen) erosion filter write-up, which
  refines them. (`detail.rs`)
- **Chaikin corner cutting** for river polylines: G. Chaikin, *An algorithm for high speed
  curve generation*, Computer Graphics and Image Processing 3, 1974. (`rivers.rs`)
- **Meander wavelength** of about 10–14 channel widths: L. B. Leopold and M. G. Wolman,
  *River meanders*, GSA Bulletin 71, 1960.
- **Orographic rain and rain shadows**: a simplified moisture advection, in the spirit of
  R. B. Smith and I. Barstad, *A linear theory of orographic precipitation*, Journal of the
  Atmospheric Sciences 61, 2004. (`terrain.rs::rain_from`)
- **Deltas and estuaries**: the river / wave / tide-dominated classification of
  W. E. Galloway, *Process framework for describing the morphologic and stratigraphic
  evolution of deltaic depositional systems*, 1975, and real deltas (Mississippi bird's foot,
  Nile, Lena) as visual references. (`mouths.rs`)
- **Plate tectonics as province generator** (Voronoi plates, ranges on converging
  boundaries): a common procedural-worldbuilding technique, e.g. as popularized by Martin
  O'Leary's *Generating fantasy maps* and many planet generators.
- **Noise**: Perlin-style gradient noise, fBm, ridged multifractal (F. K. Musgrave, *Texturing
  and Modeling: A Procedural Approach*), domain warping (Inigo Quilez, *Domain warping*
  article). Integer hash finalizer constants from MurmurHash3 (Austin Appleby).
- **Smooth minimum** (coast ramp): Inigo Quilez, *Smooth minimum* article.

## Terrain rendering (`src/map`, `assets/shaders`)

- **Chunked quadtree LOD with skirts**: T. Ulrich, *Rendering Massive Terrains using Chunked
  Level of Detail Control*, SIGGRAPH 2002 course notes. (`mesh.rs`)
- **Toon terrain lighting** (cel steps, rim light): janhohenheim's
  [bevy_wind_waker_shader](https://github.com/janhohenheim/bevy_wind_waker_shader) (MIT /
  Apache 2.0), which this project's first terrain shader was taken from; cel shading after
  Wind Waker ([video](https://www.youtube.com/watch?v=mnxs6CR6Zrk)) and rim lights after
  Breath of the Wild ([video](https://www.youtube.com/watch?v=By7qcgaqGI4)).
- **Wind over grass** (gust bands sweeping across meadows, swaying vegetation): the common
  stylized technique popularized by Breath of the Wild and Ghost of Tsushima.
- **Grass carpet** (the view ray marched through a layer of noise-peak blades, tips displaced
  by the wind, bases shaded by depth): after the Shadertoy grass shader MdsGzS
  (shadertoy.com/view/MdsGzS), adapted to a parallax layer over the terrain.
  (`map_material.wesl::grass_carpet`)
- **Swell refraction** (crests along the travel time of waves to the shore, wave speed
  ~ √depth): the eikonal / fast marching view of wave fronts, J. A. Sethian, *Level Set
  Methods and Fast Marching Methods*, 1999, and the shallow-water dispersion of swell
  (refraction, shoaling). (`map/sea.rs`, `water.wesl`)
- **Step-pool streams** (steep rivers as pools between small falls): D. R. Montgomery and
  J. M. Buffington, *Channel-reach morphology in mountain drainage basins*, GSA Bulletin
  109, 1997. (`map/water.rs::river_profile`)
- **Flow along travel time** (river patterns scrolled along the water's travel time, so they
  stretch where fast without distorting): a variant of flow mapping, after Valve's water
  flow (A. Vlachos, *Water Flow in Portal 2*, SIGGRAPH 2010).
- **Toon water** (colour from the depth behind the surface, shoreline foam): Roystan's
  *Toon Water Shader* tutorial (roystan.net). (`water.wesl`)
- **Inverse-size noise fading** (octaves fade below a pixel): standard procedural
  antialiasing, as in *Texturing and Modeling: A Procedural Approach*.

## Engine and examples

- [Bevy](https://bevy.org) and its examples: the camera input mapping is adapted from
  `pan_orbit_camera_custom_input_plugin`; the material extension follows
  `shader/extended_material`.
- Purchased asset packs (building models in `assets/models`, not redistributed).
- Nature models (`assets/models/nature`, built by `tools/nature`), CC0:
  [Kenney](https://kenney.nl) Nature Kit, and [Quaternius](https://quaternius.com) Stylized
  Nature MegaKit (free Standard version).
- Level-of-detail stand-ins built from convex hulls of foliage clusters (`tools/nature`), and
  the dithered LOD cross-fade of Bevy's `VisibilityRange`.

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
- **Toon water** (colour from the depth behind the surface, shoreline foam): Roystan's
  *Toon Water Shader* tutorial (roystan.net). (`water.wesl`)
- **Inverse-size noise fading** (octaves fade below a pixel): standard procedural
  antialiasing, as in *Texturing and Modeling: A Procedural Approach*.

## Engine and examples

- [Bevy](https://bevy.org) and its examples: the camera input mapping is adapted from
  `pan_orbit_camera_custom_input_plugin`; the material extension follows
  `shader/extended_material`.
- Purchased asset packs (models in `assets/models`, not redistributed).

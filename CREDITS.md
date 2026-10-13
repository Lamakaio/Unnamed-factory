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
- **Lakes in landscape evolution** (basins kept through erosion, filled by sediment, flow
  routed across them to their outlet): after the depression handling of FastScape-type models,
  G. Cordonnier, B. Bovy, J. Braun, *A versatile, linear complexity algorithm for flow routing
  in topographies with depressions*, Earth Surface Dynamics 7, 2019. (`terrain.rs::erode`)
- **Rift valleys** (sinking grabens between uplifted shoulders, holding chains of lakes, as in
  the East African Rift) and **glacial overdeepening** (troughs dug unevenly, holding ribbon
  lakes): S. J. Cook and D. A. Swift, *Subglacial basins: their origin and importance in
  glacial systems and landscapes*, Earth-Science Reviews 115, 2012. **Endorheic lakes**: lake
  area set by the balance of inflow and evaporation (closed-basin hydrology). (`terrain.rs`,
  `lakes.rs`)
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
- **Equiangular cube-sphere** (the planet's grid): C. Ronchi, R. Iacono, P. S. Paolucci, *The
  "Cubed Sphere": A New Method for the Solution of Partial Differential Equations in
  Spherical Geometry*, Journal of Computational Physics 124, 1996. (`sphere.rs`)
- **Solid angle of a spherical triangle** (cell areas): A. Van Oosterom, J. Strackee, *The
  Solid Angle of a Plane Triangle*, IEEE Trans. Biomedical Engineering 30, 1983.
- **3D gradient noise with cube-edge gradients**: K. Perlin, *Improving Noise*, SIGGRAPH 2002.
  (`noise.rs::get3`)
- **Planet design** (`docs/living-world.md`, `planet.rs`): rigid plates with Euler-pole
  rotations and boundary classification after Y. Cortial, A. Peytavie, É. Galin, É. Guérin,
  *Procedural Tectonic Planets*, Computer Graphics Forum 2019; tectonics.js (Carl Davidson);
  Red Blob Games, *Planet generation*; World Orogen (R. Aguilar).

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
- **Flow maps** (river patterns pushed along a per-vertex velocity in two cross-faded
  phases, continuous where rivers meet): A. Vlachos, *Water Flow in Portal 2*, SIGGRAPH
  2010. (`water.wesl`, `map/water.rs`)
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

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
  in topographies with depressions*, Earth Surface Dynamics 7, 2019: basin graph, minimum
  spanning tree over the passes and carving, used directly for erosion's routing
  (`flow.rs::FlowGraph::with_depressions`, `terrain.rs::erode`).
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
- **Planet climate** (`climate.rs`): energy balance model after M. I. Budyko (1969),
  W. D. Sellers (1969) and G. R. North, R. F. Cahalan, J. A. Coakley, *Energy balance climate
  models*, Rev. Geophys. 19, 1981 (outgoing longwave `A + B T`, diffusive heat transport,
  ice-albedo feedback); daily mean insolation (standard formula, e.g. Wikipedia,
  *Insolation*); circulation cells and ITCZ placement and rule-based ocean currents in the
  spirit of World Orogen (R. Aguilar) and the worldbuilding literature; Köppen–Geiger
  classification thresholds (Wikipedia, *Köppen climate classification*). Wind strength:
  surface winds calibrated on Earth's zonal mean winds (trades, westerlies, polar easterlies:
  climatology as in any meteorology textbook, e.g. Wallace & Hobbs, *Atmospheric Science*),
  the storm spread tied to the north–south temperature gradient (baroclinic instability,
  E. T. Eady 1949, as the measure of where mid-latitude storms grow), land friction, and
  terrain blocking and speed-up on exposed ground (the idea behind terrain-following wind
  models such as WindNinja, simplified to a ring of heights and the local slope).
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
- **Rock types and layered erosion** (`geology.rs`, `terrain.rs`): erodibility and hillslope
  diffusion varying with the bed at each cell's height along dipping strata surfaces, so the
  stream-power erosion carves steps, mesas, cuestas and hogbacks: after A. M. Forte, K. X.
  Whipple, A. Bookhagen, M. W. Rossi, *Complexities of landscape evolution during incision
  through layered stratigraphy with contrasts in rock strength*, JGR Earth Surface 121, 2016;
  rock types from plate setting as in introductory geology (cratons, orogens, arcs, basins).
- **Volcanoes** (`volcanoes.rs`): arcs above subduction zones and hotspot chains carried by
  the plates over fixed mantle plumes, ageing and sinking downstream (J. T. Wilson, *A possible
  origin of the Hawaiian Islands*, Can. J. Phys. 41, 1963; W. J. Morgan, *Convection plumes in
  the lower mantle*, Nature 230, 1971); strato cone and shield profiles as in volcanology
  textbooks.
- **Fjords** (`fjords.rs`): glacial troughs with a sill at the mouth, flooded by the sea, as
  described in glacial geomorphology (e.g. D. Benn and D. Evans, *Glaciers and Glaciation*,
  2010).
- **Glaciers** (`glaciers.rs`): ice where summers stay below freezing (the equilibrium line
  idea of glaciology, e.g. Benn and Evans, *Glaciers and Glaciation*), tongues down the
  valleys, moraines at the margins.
- **Desert types** (`surface/features.rs::deserts`): ergs, regs and badlands as classified in desert
  geomorphology (e.g. A. Goudie, *Arid and Semi-Arid Geomorphology*, 2013); transverse dunes
  with a gentle stoss side and a steep slip face (R. A. Bagnold, *The Physics of Blown Sand
  and Desert Dunes*, 1941).
- **Karst** (`surface/features.rs::karst`): tower karst in humid tropics, dolines elsewhere (D. Ford and
  P. Williams, *Karst Hydrogeology and Geomorphology*, 2007); towers placed with cellular
  (Worley) noise (S. Worley, *A Cellular Texture Basis Function*, SIGGRAPH 1996).
- **Barrier islands** (`barriers.rs`): sand bars on low, gently sloping coasts with lagoons
  and tidal inlets behind them, as described in coastal geomorphology (e.g. R. Davis and
  D. FitzGerald, *Beaches and Coasts*, 2004).
- **Reefs and atolls** (`reefs.rs`): fringing reefs, barrier reefs round sinking volcanic
  islands and atolls on sunk ones, after C. Darwin, *The Structure and Distribution of Coral
  Reefs*, 1842 (the subsidence theory); corals in warm tropical seas.
- **New nature models** (`tools/nature/build.sh`: palms, acacia, jungle trees, mangroves,
  bamboo, tundra shrub): Kenney Nature Kit (CC0), recoloured and rescaled.
- **World parameters in the climate** (`world.rs`, `climate.rs`): the star's brightening with
  age (D. O. Gough, *Solar interior structure and luminosity variations*, Solar Physics 74,
  1981); CO₂ and CH₄ forcing (G. Myhre et al., *New estimates of radiative forcing due to well
  mixed greenhouse gases*, GRL 25, 1998), with CO₂'s growth at high levels fitted to
  B. Byrne and C. Goldblatt, *Radiative forcing at high concentrations of well-mixed
  greenhouse gases*, GRL 41, 2014; organic haze above CH₄/CO₂ ≈ 0.1 (as in studies of the
  Archean Earth, e.g. Catling and Zahnle, *The Archean atmosphere*, Science Advances 2020);
  Kepler's equation for the orbit; fewer, wider circulation cells and more poleward heat
  transport on slowly spinning planets (Hadley width ∝ rotation^-1/3: S. Hill et al. 2019;
  Y. Kaspi and A. Showman, ApJ 804, 2015).
- **Planets at other stages** (`world.rs`, terrain and water shaders): star colours from
  T. Helland's fit of black-body colours (*How to Convert Temperature (K) to RGB*, 2012); red
  beds and grey pre-oxygen ground, iron-rich seas before oxygen (the Great Oxidation Event's
  record: banded iron formations, red beds); most limestone laid down by life; sea water's
  freezing point about −0.054 °C per ‰ of salt.
- **Geology from the world's age** (`world.rs`, `craters.rs`, `tectonics.rs`): impact flux
  from the lunar chronology (G. Neukum, B. Ivanov and W. Hartmann, *Cratering records in the
  inner Solar System*, Space Science Reviews 96, 2001); crater shapes (bowls, rims, ejecta,
  complex craters' flat floors and central peaks) after H. J. Melosh, *Impact Cratering: A
  Geologic Process*, 1989; continental growth fast early (B. Dhuime et al., *A change in the
  geodynamics of continental growth 3 billion years ago*, Science 335, 2012); a supercontinent
  as continents gathered on slow plates (the supercontinent cycle: J. B. Murphy and R. D.
  Nance, e.g. *Do supercontinents introvert or extrovert?*, Geology 31, 2003).
- **Moons and tides** (`world.rs`, `surface/features.rs`): tide-raising force as mass /
  distance³, the star's through Kepler's third law; tides amplified on wide shelves and in
  funnelling bays, as described in D. Pugh and P. Woodworth, *Sea-Level Science*, 2014.
- **Plants and skies under other stars** (`world.rs`, `toon.wesl`, `planet_terrain.wesl`,
  `atmosphere.wesl`): foliage colours from the star's light (N. Kiang et al., *Spectral
  signatures of photosynthesis*, Astrobiology 7, 2007: green under the Sun, red to near black
  under cooler stars, blue unlikely); greens turned in YIQ hue; braided rivers before land
  plants (M. Gibling and N. Davies, *Palaeozoic landscapes shaped by plant evolution*, Nature
  Geoscience 5, 2012).

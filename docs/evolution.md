# Evolution: world age, life and the player

> Status: **draft for discussion** (2026-11-04). The "own design document" that
> `docs/living-world.md` §8 announces for D3 (life = evolution over deep time). It answers:
> what does a planet look like at each stage of its life, which of today's features need
> life, how are species simulated, how does life change the world back, and what does the
> player do. Open choices are collected in §9; my recommendation is given for each. Dates are
> Earth's, rounded, and serve as a template, not a script.

---

## 0. Summary

- **Life is a geological agent, and that is the game.** On Earth, life made the oxygen, the
  red soils, most limestone and reefs, the stable rivers, the soils, the coal, the grass and
  the forests, and twice nearly froze the planet. A player who steers evolution should see
  the planet change because of it: the colour of the land and sky, the rivers, the climate.
- **World age and life stage become generation inputs.** A virgin planet is bare rock, sand
  and sea: no trees, no grass, no reefs, no peat, wide braided rivers, more dunes, another
  sky. Each feature we render today gets a life requirement (§3), so the same generator
  gives a planet at any stage.
- **Species are population fields over a coarse grid** with niches and traits (the
  `gen3sis` style, §5), not individual creatures. Speciation comes from isolation (plates,
  sea level, mountains, deserts), extinction from shrinking ranges. Individuals only appear
  when the player zooms in.
- **The player steers, they don't design**: they unlock key innovations (photosynthesis,
  multicellularity, life on land, wood, seeds…) and push selection, with consequences they
  have to manage (oxygen crisis, snowball, CO₂ drawdown, a brightening sun) (§7).

---

## 1. Earth as the template

The planet's history follows Earth's broad sequence; each step needs the previous ones and
changes the world in a visible way. Dates are approximate (sources differ, especially for
life on land: fossils vs molecular clocks).

| Ga (billion years ago) | Stage | What it changes on the planet |
|---|---|---|
| 4.5–4.0 | **Abiotic.** Hot young planet, faint young sun (~70 % of today's light), CO₂/N₂ atmosphere | Bare rock and sea; lots of volcanism; smaller, scattered continents; impacts |
| ~3.8–3.5 | **First microbes**, then **stromatolites** (microbial mats building carbonate mounds in shallow seas) | Mat-covered shallows; the first biogenic carbonate; methane warms the planet (orange haze possible) |
| ~2.7–2.4 | **Oxygenic photosynthesis** (cyanobacteria) → **Great Oxidation Event** (~2.4–2.1) | Oxygen in the air; methane gone → first global glaciations; iron rusts out of the sea (banded iron formations end ~1.85); **red beds**: land turns from grey/beige/green to red |
| ~1.8–1.0 | **Eukaryotes**, sex, algae ("boring billion": stable, low oxygen) | Little visible; seas greener with algae |
| 0.72–0.635 | **Snowball Earth** (Cryogenian) | Ice to the tropics; ended by volcanic CO₂ |
| 0.58–0.5 | **Ediacaran biota, Cambrian explosion**: animals, shells, skeletons | Shell-built limestone, the first animal reefs; burrowing stirs the sea floor |
| ~0.47–0.43 | **Plants on land** (mosses, liverworts), fungi | Thin soils start; land still mostly bare |
| ~0.42–0.36 | **Vascular plants, roots, the first forests** (Devonian); insects; tetrapods walk out | Roots stabilise banks (rivers meander more widely, mud floodplains appear); deeper weathering draws CO₂ down → late-Devonian ice age; fires (O₂ high enough) |
| 0.36–0.30 | **Coal forests** (Carboniferous), seeds; O₂ peaks | Swamp forests, peat → coal; giant insects |
| 0.25 | **Great Dying** (Permian–Triassic, huge volcanism) | Most species gone; recovery over ~10 Myr |
| 0.24–0.066 | Dinosaurs, modern corals (~0.24), **flowering plants** (~0.13), mangroves (late) | Modern-style reefs; flowers; mangrove coasts |
| 0.066 | **Impact extinction** | — |
| 0.06–0 | Mammals, **grasses** (grasslands spread from ~0.02; savannas ~0.008), ice ages | Grasslands, savanna, steppe as we know them |

Consequences for this project:

- **Grass is recent.** Meadows, steppe and savanna (most of today's ground cover) only exist in
  the last ~2 % of the planet's life. Before that, open ground is bare, mossy, or covered by
  ferns and shrubs.
- **Before plants, rivers and land behave differently.** Rock records before plants show wide,
  sheet-like, shifting sandy rivers, little mud and many wind-blown sands. Recent work (2019,
  2025) shows unvegetated rivers can meander too, but plants made stable meanders and muddy
  floodplains common. For us: wide braided rivers and more dunes on bare land, narrower
  stable channels and floodplains under vegetation.
- **Most limestone is biological** (shells, reefs, microbial mats). Before life, carbonate
  rock is rare. Karst, limestone cliffs and reefs are life features.
- **The sun brightens with age** (~1 % per 100 Myr). Life's CO₂ drawdown is what keeps a
  planet temperate as it brightens; a planet that loses its life overheats in the end. This
  is the long-term tension SimEarth was built on (Lovelock's Gaia and Daisyworld).

---

## 2. World parameters: age and life

Generation takes two new inputs besides the seed:

- **Planet age** (Gyr): the sun's brightness, how hot the interior is (more volcanism,
  smaller and more numerous continents, thinner crust when young), how worn old ranges are.
- **Biosphere state**: which innovations exist (§5.2) and the atmosphere they made (O₂, CO₂,
  CH₄ as 0..1 levels, not chemistry).

Presets for previews and new games: *virgin* (abiotic, ~0.5 Gyr), *microbial* (~3 Gyr, low
O₂), *oxygenated* (~2 Gyr after the oxidation, no land life), *green* (land plants, no
grass), *modern* (today's generator). In a game, the deep-time clock moves the planet
through these by itself; the presets are just snapshots.

What changes on screen per stage (all already-existing machinery, gated):

| | Virgin / microbial | Oxygenated | Plants | Modern |
|---|---|---|---|---|
| Land colour | grey, beige, black basalt, pale sand; greenish mats on wet shores | **red beds** (hematite), ochre | soils: browns, greens | as today |
| Sky | hazy, orange-tinted with methane | blue | blue | blue |
| Sea | darker, greener (iron, no plankton bloom colours) | blue-green; plankton | as today | as today |
| Vegetation | none (mats: a ground tint) | none on land | mosses, ferns, then forests; no grass | as today |
| Rivers | wide, braided, shifting | same | narrower, meandering, floodplains | as today |
| Dunes | far more (no plants pin sand) | same | fewer | as today |
| Reefs | stromatolite mounds only | same | ancient reefs (sponge, coral) | coral reefs |
| Limestone, karst | rare | more (microbial) | common | as today |
| Marsh, peat | salt flats and mudflats only | same | swamp forests, peat | as today |

---

## 3. Today's features and what life they need

Every feature in `worldgen` and the game gets a life requirement. Most are a single gate.

| Feature (today) | Needs | Without it |
|---|---|---|
| Forests (deciduous, conifer, rainforest, cloud forest), riparian forests, oases | trees (wood, ~0.385 Ga); flowering trees for broadleaf | ferns and early trees, then nothing |
| Meadow, steppe, savanna, alpine meadow | grasses | bare ground, mosses, ferns, shrubs depending on stage |
| Tundra | mosses and shrubs | bare rock and frost-shattered stones |
| Marsh | wetland plants | mudflats, salt flats |
| Mangrove | mangroves (late) | mudflats |
| Coral reefs, atolls | reef builders (corals; earlier: sponges, stromatolites) | volcanic islands sink without leaving atolls |
| Limestone (rock), karst | shell-building life (and microbial carbonate) | sandstone/shale take its share |
| Tropical beaches (coral sand) | reefs | basalt or quartz sand |
| Badlands, ergs, deserts | — (abiotic, more extensive without plants) | — |
| Glaciers, ice sheets, sea ice | — (but climate depends on life's gases) | — |
| Hot springs, volcanoes | — (the colourful mats in hot springs are life) | plain mineral colours |
| River style (narrow, meandering; floodplains) | rooted plants on the banks | wide, braided, sandy |
| Soil (ground colour, grass texture) | plants + microbes | regolith: rock colour |

Two things follow for the code:

1. `Biome` is today *both* the physical setting and the vegetation on it. Split it: a
   **setting** (sea, shore, wetland, dry plain, mountain, ice, …, from climate and terrain,
   abiotic) and a **cover** (what lives there: none, mats, mosses, ferns, forest, grass…,
   from the biosphere). The ground colour comes from rock, soil and cover.
2. Life-built landforms (reefs, atolls, limestone) need the biosphere's history, not only
   its present: an atoll exists if reefs grew while the volcano sank.

---

## 4. What life does to the world (feedbacks)

These are what make evolution matter to the planet, and they are mostly cheap to model as
fields and global levels:

| Life does | Effect on the world | Modelled as |
|---|---|---|
| Photosynthesis (microbes, then plants) | O₂ rises (after sinks are full: iron, volcanic gases), CH₄ falls → cooling, possible snowball | global O₂, CH₄, CO₂ levels; climate's greenhouse term |
| Roots, soils, weathering | CO₂ drawdown → cooling; soils form | weathering rate × vegetation; soil depth field |
| Vegetation cover | slower erosion on slopes, stable banks, fewer dunes, darker ground (albedo), more evaporation inland (rain further from the coast) | erosion's erodibility and diffusion; channel width; dune mask; albedo and moisture recycling in the climate |
| Shells and reefs | limestone, reefs, atolls; carbon buried | reef growth field; rock type of new sediment |
| Swamp forests | peat, coal: carbon buried, O₂ up | burial rate in wetlands |
| High O₂ | wildfires (above ~15 %), giant insects; below, no fires | fire frequency → savanna/forest balance |
| Grazers, burrowers | grassland kept open; sea floor stirred | modifiers on cover and sediment |
| Mass extinction | cover collapses → erosion spikes, carbon released | transient fields |

And from the world to life (selection): climate means and seasonality, land/sea, connectivity
(continents splitting and joining, land bridges at low sea level, mountain ranges and deserts
as barriers), ice, and how fast all this changes.

---

## 5. Simulation model

### 5.1 Grid and clock

- Runs on the **deep-time clock** (living-world §1): one step = 10⁴–10⁶ years depending on
  the epoch (earlier eras pass faster, as in SimEarth, so the player spends their time where
  life is interesting).
- On a **coarse grid** (6 × 64² ≈ 25 k cells, or 6 × 128²), like the climate. Fields are
  upsampled for display.

### 5.2 Innovations (the "tech tree" of life)

A small set of gates that change what life can do, each with prerequisites and conditions
(e.g. life on land needs an ozone layer, so O₂ above a level):

> self-replication → microbial mats / stromatolites → oxygenic photosynthesis → eukaryotes
> → multicellularity → shells and skeletons → (sea) reef builders · (land) mosses → roots
> and vascular plants → wood (trees) → seeds → flowers → grasses
> animals: → bilaterians → burrowers → fish → land animals → flight → warm blood → …

Innovations are what the player mostly acts on (§7). They are global (once photosynthesis
exists, it exists), the species that carry them are not.

### 5.3 Species

Following eco-evolution engines (gen3sis: populations with niche traits, dispersal,
speciation after isolation; carrying capacity from temperature and aridity):

- A **species** (or clade, at our scale) is: a functional type (mat, plankton, reef builder,
  moss, fern, tree, grass; grazer, predator, burrower, …), traits (temperature optimum and
  tolerance, moisture need, salt tolerance, size, dispersal distance, growth rate), and a
  **range**: abundance per coarse cell.
- Each step: **growth** toward the cell's capacity for that niche (climate × substrate ×
  competition with species of the same type), **dispersal** to neighbouring cells through the
  connectivity graph (land species over land, sea species over sea, rivers as corridors,
  barriers costly), **trait drift** toward local conditions (selection), and the cell's
  population fading where it doesn't fit.
- **Speciation**: when a species' range splits into parts that stay apart for N steps, each
  part becomes a species (the parts' traits then drift apart). **Extinction**: range gone.
  This gives a **phylogeny** (clade tree) for free, which is a great thing to show the player.
- **Cover** (§3) per cell is the sum of the plant types' abundance, which is what the
  renderer uses (trees, grass density, ground colour).

Budget *(est.)*: 25 k cells × a few hundred species × a sparse range is a few million
updates per step: milliseconds to tens of milliseconds with `par.rs`.

### 5.4 Coupling with the planet

- **World → life** each step: climate means (temperature, rain, seasonality), land/sea,
  ice, slope, rock/soil, and their rate of change.
- **Life → world** each step: global gas levels (to the climate), cover (to erosion,
  albedo, moisture recycling, river style, dunes), reef growth and carbonate (to geology),
  burial (to O₂).
- **History**: events with dates (oxidation, first forests, extinctions, a range split by a
  rift) for the player's timeline.

---

## 6. What the generator does in the meantime

Before the full simulation exists, the generator can already produce a planet at a given
stage (milestone E1, §8): biosphere preset → gates on vegetation, reefs, limestone, river
style, dunes, ground and sky colours. This alone answers "a virgin planet shouldn't have
trees and grass", and every later step reuses the gates.

---

## 7. The player

Prior art:

- **SimEarth** (Maxis, 1990, with James Lovelock): geological epochs that pass faster early
  on, Gaia feedbacks, Daisyworld; players tune greenhouse, mutation rates, drift. Admired more
  than played: hard to read, no goals in sandbox.
- **Species: Artificial Life, Real Evolution**: genes, mutation, natural selection, all
  emergent; player tinkers in an isolated area, triggers catastrophes; clade diagram. Strong
  simulation, weak player agency.
- **Spore** (cell stage onward): direct creature design; fun, but no real evolution.
- **Thrive** (open source): scientifically minded Spore-like; microbe stage done.
- **Plague Inc.**: the "spend points on traits, watch the world respond" loop works well.

Proposal: **the player is the planet's evolution, not a god.** They act on *pressure and
opportunity*, and the simulation decides outcomes:

1. **Innovations**: spend evolution points (earned from biodiversity and time) to make an
   innovation appear in a chosen lineage once its conditions are met. They don't pick
   where it succeeds.
2. **Selection nudges**: favour a trait in a region (cold tolerance in the north, larger
   size on an island) — a bias on drift, not a change.
3. **Planet levers** (late or as scenarios): a few SimEarth-like knobs (volcanism, an
   impact, a cometary delivery of water) at a cost.
4. **Goals**: scenarios and milestones (reach land; survive the oxidation without a
   snowball; keep the planet habitable as the sun brightens; a given biodiversity; recover
   from an extinction; evolve intelligence as a far goal), plus sandbox.

The player's main view is the planet in fast-forward with overlays (ranges, oxygen,
temperature, the clade tree), and the game-clock zoom to look at a landscape and its species.

---

## 8. Milestones

Each leaves the game runnable. E1 and E2 fit before the deep-time mode (living-world
milestone 5); E3 onwards needs it.

1. **E1 – Life stage at generation.** `PlanetSettings` gets age and a biosphere preset;
   gates from §2/§3 on vegetation, grass, reefs, limestone, mangroves, marsh, river width and
   style, dunes; ground, sky and sea colours per stage. Previews for each preset.
2. **E2 – Setting and cover.** Split `Biome` into setting (abiotic) and cover (life);
   cover as fractions per plant type from climate and the available innovations; the
   renderer reads cover. Sun brightness from age in the climate.
3. **E3 – Biosphere fields at deep time.** Innovations, gas levels, cover evolving on the
   coarse grid with feedbacks to climate and erosion; no species yet (functional types only).
4. **E4 – Species.** Ranges, dispersal, speciation, extinction, clade tree, history events.
5. **E5 – Player.** Points, innovations, nudges, scenarios, overlays.
6. **E6 – Animals at game time.** Individuals of the present species when zoomed in.

---

## 9. Decisions to take

- **V1. Unit of simulation**: (a) functional types only, (b) **species with traits and
  ranges (recommended)**, (c) individuals. (b) gives speciation and a clade tree at a cost we
  can afford; (c) can't reach deep time.
- **V2. How Earth-like the history is**: (a) Earth's sequence scripted, (b) **the same
  innovations and their conditions, outcomes and timing emergent (recommended)**, (c) free,
  alien biochemistries. (b) keeps the planet plausible (the bar of D5) while each game differs.
- **V3. Player role**: (a) watcher with catastrophes (Species), (b) **steering through
  innovations and selection nudges (recommended)**, (c) direct designer (Spore).
- **V4. Start**: (a) **always a virgin planet; earlier eras pass fast (recommended)**, (b)
  choose a starting stage (presets) as well. Presets are needed anyway for previews.
- **V5. Animals on screen**: when, and how many kinds (they are art-heavy: toon style
  helps).

---

## 10. References

Earth history
- Timeline of life (Wikipedia): https://en.wikipedia.org/wiki/Timeline_of_life
- Great Oxidation Event, banded iron formations and red beds:
  https://en.wikipedia.org/wiki/Great_Oxidation_Event ·
  https://www.nhm.ac.uk/discover/the-heavy-metal-rock-bands-charting-life-on-early-earth.html
- Snowball Earth and oxygen: https://www.lunduniversity.lu.se/article/oxygen-content-increased-when-earth-was-covered-ice

Life shaping landscapes
- Gibling & Davies 2012, *Palaeozoic landscapes shaped by plant evolution*, Nature Geoscience:
  https://sseh.uchicago.edu/doc/Gibling_and_Davies_2012.pdf
- Pre-vegetation alluvium (Cambridge): https://www.repository.cam.ac.uk/handle/1810/276277
- Ganti et al. 2019, *Ain't no river wide enough* (single-channel rivers before plants):
  https://news.ucsb.edu/2019/019475/aint-no-river-wide-enough
- Stanford 2025, unvegetated rivers can meander:
  https://news.stanford.edu/stories/2025/08/rise-plant-life-changed-how-rivers-move-study-shows
- Nature Reviews Earth & Environment 2021 review on rivers and vegetation:
  https://www.nature.com/articles/s43017-021-00249-6

Simulation
- Hagen et al. 2021, *gen3sis: a general engine for eco-evolutionary simulations*:
  https://www.ncbi.nlm.nih.gov/pmc/articles/PMC8384074/
- Watson & Lovelock 1983, Daisyworld (in SimEarth's manual).

Games
- SimEarth: https://en.wikipedia.org/wiki/SimEarth
- Species: Artificial Life, Real Evolution: https://store.steampowered.com/app/774541
- Thrive: https://revolutionarygamesstudio.com/

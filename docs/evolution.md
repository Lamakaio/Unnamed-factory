# Evolution: world age, atmosphere and life

> Status: **draft, first decisions taken** (2026-11-04, §9). The "own design document" that
> `docs/living-world.md` §8 announces for D3 (life = evolution over deep time). It answers:
> what a planet looks like as it ages and as life changes it, which of today's features need
> life, how the world state evolves, and (later) how species and the player fit in.
>
> **Direction (V1–V4):** the world evolves from a few global parameters (age, star, CO₂,
> O₂, CH₄, how much life there is and what forms it takes), not from a detailed species
> simulation. Not Earth-focused: Earth is one example of the processes, not a script. Every
> game starts from a virgin planet. Species and the player come later: when a species
> splits, the player designs the new one.

---

## 0. Summary

- **Life is a geological agent, and that is the game.** On Earth, life made the oxygen, the
  red soils, most limestone and reefs, the stable rivers, the soils, the coal and the
  forests, and twice nearly froze the planet. Steering life should visibly change the
  planet: the colour of the land, sea and sky, the rivers, the ice, the climate.
- **The world state is a handful of global numbers** (§2): age, star brightness, CO₂, O₂,
  CH₄, and the biosphere as levels of *forms* of life (sea mats, plankton, reef builders,
  land crusts, low plants, tall plants…). They evolve through a small box model of the
  carbon and oxygen cycles (§4), the kind geochemists use for deep time (Walker–Hays–
  Kasting's weathering thermostat, COPSE).
- **The generator renders any state** (§3): every feature we have gets a condition on the
  state, so the same seed gives a virgin planet, a microbial one, a red oxygenated one, a
  green one. A virgin planet: bare rock and sand, wide braided rivers, more dunes, a hazy
  sky, no trees, grass, reefs, limestone or peat.
- **Not Earth's history.** Forms of life are defined by what they do (photosynthesis, where
  they live, how tall, whether they build shells or roots), not by Earth's lineages, and
  their colours follow the star (§5). Timing and outcomes differ every game.
- **Later:** species as a light layer on top of the forms, and the player designing a new
  species each time one splits off (§7).

---

## 1. Earth as one example

Earth's history shows the processes and their order of magnitude; our planets follow the
same physics and chemistry with their own timing and outcomes. Dates are approximate
(sources differ, especially for life on land: fossils vs molecular clocks).

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

What carries over to any planet:

- **Ground cover is late and contingent.** Earth's grasslands are only ~2 % of its history;
  before them open ground was bare, crusted, mossy or ferny. Our open ground is whatever low
  form the planet evolved, or nothing.
- **Before land plants, rivers and land behave differently.** Rock records before plants
  show wide, sheet-like, shifting sandy rivers, little mud and many wind-blown sands. Recent
  work (2019, 2025) shows unvegetated rivers can meander too, but plants made stable meanders
  and muddy floodplains common. For us: wide braided rivers and more dunes on bare land,
  narrower stable channels and floodplains under vegetation.
- **Most limestone is biological** (shells, reefs, microbial mats). Before life, carbonate
  rock is rare. Karst, limestone cliffs and reefs are life features.
- **The star brightens with age** (a Sun-like star by ~1 % per 100 Myr). Weathering draws
  CO₂ down faster when it is warm, which keeps a planet temperate as its star brightens; life
  speeds weathering up. Push it too far and the planet freezes; lose it and the planet
  overheats in the end. This is the long-term tension SimEarth was built on (Lovelock's Gaia
  and Daisyworld).
- **Oxygen is a threshold, not a slope.** Photosynthesis makes O₂ for a long time before it
  shows: iron in the sea and volcanic gases eat it. When the sinks are full, O₂ rises fast,
  methane collapses (cooling, maybe a snowball), the land rusts red, an ozone layer forms
  and land life becomes possible.

---

## 2. The world state

### 2.1 Planet parameters (chosen 2026-11-04)

Picked from a longer list (moons as tides, star as light colour, …). Life and events
parameters are left for later. "Exists" means a setting already covers it.

| # | Parameter | Changes | Where it plugs in |
|---|---|---|---|
| 1 | **Star type** (temperature) | sunlight colour, sky tint; later foliage colour (§5) | game lighting and sky; colour palette |
| 2 | **Star brightness** now and its brightening with age | overall temperature; the long-term pressure on the climate | climate `solar` (exists) from star + age |
| 4 | **Year length** (orbital distance and period) | seasons' length: a long year gives land time to heat and freeze (stronger seasons, more continental); a short one evens them out | climate's month length (`dt`) |
| 5 | **Eccentricity** (and where perihelion falls) | one hemisphere's summer hotter than the other's; seasons of unequal length | climate insolation × 1/r², orbit timing |
| 6 | **Axial tilt** (exists) | seasons, ice caps, tropics; extremes (0°, > 54°) | climate `tilt` |
| 7 | **Tilt and orbit cycles** (wobble) | ice ages coming and going | deep time (E3); a snapshot uses the current phase |
| 8 | **Day length** (rotation) | climate belts: a slow spin widens the tropical cells (fewer, wider bands, dry belts further out), a fast one narrows them (more bands, stronger jets); day–night temperature range | climate pressure belts and Coriolis turning |
| 9 | **Moons** (count, size, distance) | tides: tidal flats, wide estuaries, tidal range on coasts; a big moon keeps the tilt steady (without one, #7 swings much wider); the night sky | new: tidal range per coast → shore features; game sky, tide animation |
| 13 | **Age** | star brightness (#2); interior heat → volcano count, outgassing; how worn old ranges are; crater count (#20) | volcanoes, tectonic history, erosion settings |
| 17 | **Supercontinent phase** | assembled (one big land, dry interior, monsoons) ↔ dispersed (many coasts, wetter) | land/continent placement in the shape stage |
| 20 | **Impact rate** | craters, more and fresher on young planets and dry, slow-eroding ones | new feature stage (like volcanoes) |
| 21 | **Water amount** | land share (an ocean world ↔ a desert world with small seas) | `land_fraction` (exists) derived from water volume and the relief |
| 23 | **Salinity** | sea-ice freezing point, salt flats and evaporites, brackish lakes | sea ice, deserts' salt flats, lake colours |
| 24 | **Ocean chemistry** (iron, sulphur) | sea colour (green, brown, purple-tinted), banded rock on shores | water shader colour; rock colour |
| 25 | **Ocean current strength** | how much coasts are warmed or chilled (mild west coasts, fog deserts) | climate `current_anomaly` scale |
| 27 | **CO₂** | greenhouse, sea acidity (later: reefs), weathering (later) | climate `greenhouse` (exists) from the gases |
| 28 | **CH₄** | strong greenhouse; an orange haze above a level (sky colour, cooling) | climate; sky |
| 29 | **O₂** | red (oxidised) ground; blue sky; later land life and fires | ground colour; sky |
| 32 | **Dust and aerosols** | hazy sky, red sunsets, cooling | climate albedo; sky and fog |
| 34 | **Albedo of ground types** | bright deserts and ice cool, dark rock and forests warm; ice–albedo runaway (snowball) | climate albedo per cell (today: ice only) |
| 35 | **Soil depth and type** | ground colour (rock showing through thin soil), how easily it erodes | ground colours; erosion's erodibility |
| 36 | **Erosion rates** (exist) | sharp young ↔ soft old landscapes | `ErosionSettings` |

Until the life parameters are designed, life is a single switch (none ↔ today's
vegetation), so a virgin planet can already be shown.

### 2.2 Everything together

The world at a given moment is:

| Parameter | Range (indicative) | Drives |
|---|---|---|
| **Age** (Gyr) | 0 → ~10 | star brightness; interior heat → volcanism, outgassing, continent growth; how worn old ranges are |
| **Star** (type, brightness now) | per planet | light colour and amount → temperature, the colour of photosynthetic life (§5) |
| **CO₂** (bar) | 10⁻⁴ → 10 | greenhouse; ocean acidity (reefs); weathering |
| **CH₄** | 0 → a few % | greenhouse; above a level, an orange haze (sky colour, cooling) |
| **O₂** | 0 → ~35 % | ozone (land life possible), rust (red ground), fires (above ~15 %), large animals |
| **Water** (sea level, ice) | per planet | follows climate (exists) |
| **Biosphere** | a level 0..1 per *form* (§5) | everything in §3; feeds back on the gases (§4) |

Everything else (temperature, rain, biomes, ice, cover) is derived from these and the
terrain by the existing climate and surface code, with a few new inputs (greenhouse from
CO₂ and CH₄, sky colour, light colour).

**Presets** for previews, debugging and the opening of a game: *virgin* (young, CO₂-rich, no
life), *microbial* (sea mats, CH₄), *oxidised* (O₂ up, red land, no land life), *crusted*
(low land forms), *green* (tall land forms), plus anything in between from sliders. A game
always starts at *virgin* (V4); the presets are snapshots of states the deep-time run can
reach, not a fixed sequence.

---

## 3. What the planet shows for a given state

Each feature we render today gets a condition on the state. Most are a single gate or a
blend, so this is the first thing to build (milestone E1).

| Feature (today) | Needs | Without it |
|---|---|---|
| Forests (all kinds), riparian forests, oases | tall land forms | low forms, or nothing |
| Meadow, steppe, savanna, alpine meadow | low land forms (grass-like) | crusts, or bare ground |
| Tundra | low cold-tolerant forms | frost-shattered stones |
| Marsh | wetland forms | mudflats, salt flats |
| Mangrove | tall forms tolerating salt | mudflats |
| Reefs, atolls, coral sand | reef builders (low CO₂ helps: acid seas dissolve shells) | volcanic islands sink without leaving atolls; basalt or quartz sand |
| Limestone (rock), karst | shell builders or sea mats (history: how much carbonate was laid down) | sandstone and shale take its share |
| Badlands, ergs, deserts | — (abiotic; far more extensive without land life) | — |
| Glaciers, ice sheets, sea ice | — (climate from the gases) | — |
| Hot springs, volcanoes | — (bright mats round hot springs are life) | plain mineral colours |
| River style | rooted land forms on the banks | wide, braided, sandy |
| Soil (ground colour) | land forms + O₂ | regolith: rock colour; red where O₂ is up |
| Sky | CH₄ haze; O₂ | orange haze / blue |
| Sea colour | plankton; dissolved iron (no O₂) | green-brown, dark |

Two changes to the code follow:

1. `Biome` today is *both* the physical setting and the life on it. Split it into a
   **setting** (sea, shore, wetland, dry plain, mountain, ice, …, abiotic, from climate and
   terrain) and a **cover** (fractions of each land form, from the biosphere and the cell's
   climate). Ground colour comes from rock, soil and cover; vegetation from cover.
2. Life-built landforms (reefs, atolls, limestone) depend on the biosphere's *history*, not
   only its present: a running total of carbonate laid down, and reef growth while
   volcanoes sank.

---

## 4. How the state evolves

A box model stepped on the deep-time clock. Global numbers, with a few per-cell fields where
geography matters (weathering, cover). Deliberately simple: fluxes with plausible shapes and
time constants, tuned to look right, in the spirit of geochemical models of deep time
(Walker–Hays–Kasting 1981 for the weathering thermostat; COPSE, Bergman–Lenton–Watson 2004,
for carbon, oxygen and biota together).

**Carbon (CO₂):**
- *Source*: volcanic outgassing, high when young, decreasing with age (interior cooling);
  bursts from large volcanic provinces (events).
- *Sink*: silicate weathering, faster when warm, wet, with more exposed fresh rock (young
  mountains, basalt) and with land life (roots, soils: ×several). The carbon ends as
  carbonate on the sea floor (limestone).
- *Sink*: burial of organic carbon (plankton, swamps), proportional to productivity.
- The result is the thermostat: a brighter star → warmer → faster weathering → less CO₂.

**Methane (CH₄):** made by anaerobic microbes (sea mats, swamps); destroyed fast once O₂ is
up. Strong greenhouse; a haze above a level.

**Oxygen (O₂):**
- *Source*: photosynthesis × organic burial (O₂ is left over only when organic carbon is
  buried instead of rotting).
- *Sinks*: reduced volcanic gases and dissolved iron (large at first, shrinking with age and
  as they are used up), weathering of old organic carbon, fires.
- So O₂ stays near zero while the sinks win, then rises fast: the oxidation event emerges.

**Biosphere:** each form's level grows toward a capacity set by the state and the planet
(light, temperature, water, land area, nutrients, O₂ for land and large forms, CO₂ for
photosynthesis), and declines when conditions leave its range. New forms appear when their
prerequisites hold for long enough (§5), with randomness: not every planet gets every form,
and not in the same order.

**Climate** reads the gases as a greenhouse term and the star's brightness, and the cover as
albedo and moisture recycling (inland rain). Erosion reads cover (slopes protected, banks
stable, dunes pinned).

**Time step:** 1–10 Myr per step at first, finer when things change fast (an oxidation,
a snowball, a mass extinction). Cheap: the global part is a few numbers; the per-cell parts
(weathering, cover) run on the coarse climate grid.

**Events:** snowball (ice past a latitude: albedo runaway, ended by CO₂ building up under
the ice), oxidation, large volcanic provinces, impacts, mass extinctions (a form's level
crashing). Each is dated for the player's history.

---

## 5. Forms of life (not Earth's lineages)

A form is a way of living, defined by capabilities, not by ancestry. Indicative list:

| Form | Where | Needs (appears when) | Does to the world |
|---|---|---|---|
| Sea mats | shallow sea, shores | life started (warm, wet, early) | CH₄; microbial carbonate; mat-coloured shallows |
| Plankton (photosynthetic) | open sea | photosynthesis | O₂ (via burial); sea colour; organic burial |
| Shell and reef builders | warm shallow sea | O₂ above a level, moderate CO₂ | limestone, reefs, atolls |
| Land crusts | any land | ozone (O₂), mats nearby | thin soil; slight weathering boost; ground tint |
| Low land forms | land with water | crusts, soil | soil; weathering ×; dunes pinned; ground cover |
| Tall land forms | wet land | low forms, structure (wood-like) | weathering ××; albedo; inland rain; stable rivers; swamp burial; fires with high O₂ |
| Animals (later) | sea, then land | O₂ above a level | grazing keeps land open; burrowing; mostly what the player cares about |

Forms come with colours from the star's light and the atmosphere: photosynthesis absorbs the
most useful light, so foliage is green under a Sun-like star, orange or red under hotter or
bluer-filtered light, near black under a dim red dwarf; blue is unlikely (Kiang et al.
2007). For our toon style: a palette per planet derived from the star, so each planet looks
its own.

---

## 6. What to build first

E1 doesn't need the box model: the generator takes a state (preset or sliders) and renders
it (§3). This alone answers "a virgin planet shouldn't have trees and grass", and every
later step reuses the gates. The box model (E3) then makes the state move.

---

## 7. Species and the player (later)

Not a priority now (V3). The direction:

- **Species** are a light layer on top of the forms: each form is populated by a few species
  with ranges over the coarse grid and climate tolerances. When a range splits and stays
  apart (a rift, a sea, a mountain range, a desert), the species splits (as in eco-evolution
  engines like gen3sis).
- **The player designs species**, not choices from a list: when a species splits, the
  player gets a design screen for the new one (body, size, tolerances, diet, colours…). The
  design decides how it fares and what it does to the world. The world's evolution (§4)
  keeps running regardless.
- Prior art for contrast: SimEarth (knobs, Gaia, hard to read), Species: ALRE (emergent
  evolution, little player agency), Spore (design, no real evolution), Thrive (scientific
  Spore-like). Our angle: the player designs, the world selects, and the world changes.
- Animals on screen: to be decided (V5).

---

## 8. Milestones

Each leaves the game runnable.

1. **E1 – State-driven planet.** The parameters of §2.1, in steps:
   - **E1a – Star, orbit, atmosphere in the climate**: a `World` settings struct with
     presets; brightness from star and age, year length, eccentricity, day length (belts),
     CO₂ and CH₄ → greenhouse, dust, albedo per ground type, current strength.
   - **E1b – Colours**: sunlight from the star; sky from gases and dust; sea from its
     chemistry; ground from O₂ and soil; salinity in sea ice and salt flats; the life switch
     (§2.1) gating vegetation, reefs, limestone, mangroves, marsh, rivers, dunes (§3).
   - **E1c – Geology from age**: volcanism, worn ranges, craters (#20), supercontinent
     phase, water amount → land share, soil and erosion.
   - **E1d – Moons and tides**: tidal range per coast, tidal flats and estuaries; moons in
     the game's sky.
   Previews and screenshots for each preset.
2. **E2 – Setting and cover.** Split `Biome` into setting and cover; cover as fractions of
   the land forms from the state and the cell's climate; foliage palette from the star.
3. **E3 – Evolving state.** The box model (§4) on the deep-time clock; forms appearing and
   fading; events; the planet re-rendered as the state moves (with living-world
   milestone 5, deep-time mode).
4. **E4 – Feedbacks into the terrain.** Cover in erosion (slopes, banks, dunes), carbonate
   history into the rock types, reef growth into atolls.
5. **E5 – Species and design screen.** (§7)

---

## 9. Decisions

Taken on 2026-11-04:

- **V1. What to simulate**: the world state from global parameters (age, star, CO₂, O₂,
  CH₄, forms of life); species kept light, later.
- **V2. Earth likeness**: not Earth-focused. Same physics and chemistry, forms defined by
  capabilities, own timing, outcomes and colours.
- **V3. Player role**: not a priority. Later: designing each new species when one splits
  off, not picking from simple choices.
- **V4. Start**: always a virgin planet.

Open:

- **V5. Animals on screen**: when, and how many kinds.
- **V6. Star types**: Sun-like only at first, or a choice (red dwarf, hotter stars) from the
  start of E2.

---

## 10. References

Earth history
- Timeline of life (Wikipedia): https://en.wikipedia.org/wiki/Timeline_of_life
- Great Oxidation Event, banded iron formations and red beds:
  https://en.wikipedia.org/wiki/Great_Oxidation_Event ·
  https://www.nhm.ac.uk/discover/the-heavy-metal-rock-bands-charting-life-on-early-earth.html
- Snowball Earth and oxygen: https://www.lunduniversity.lu.se/article/oxygen-content-increased-when-earth-was-covered-ice

Deep-time geochemistry
- Walker, Hays & Kasting 1981, *A negative feedback mechanism for the long-term
  stabilization of Earth's surface temperature*, JGR 86:
  https://sseh.uchicago.edu/doc/Walker_et_al-1981-Journal_of_Geophysical_Research-_Solid_Earth_(1978-2012).pdf
- Bergman, Lenton & Watson 2004, *COPSE: a new model of biogeochemical cycling over
  Phanerozoic time*, AJS 304: https://ajsonline.org/article/61725.pdf · code:
  https://github.com/sjdaines/COPSE
- Lenton, Daines & Mills 2018, *COPSE reloaded*:
  https://eprints.whiterose.ac.uk/125894/1/Lenton_etal_2018_ESR.pdf
- Catling & Zahnle, *Atmospheric evolution* (review): https://arxiv.org/pdf/1710.10557

Life shaping landscapes
- Gibling & Davies 2012, *Palaeozoic landscapes shaped by plant evolution*, Nature Geoscience:
  https://sseh.uchicago.edu/doc/Gibling_and_Davies_2012.pdf
- Pre-vegetation alluvium (Cambridge): https://www.repository.cam.ac.uk/handle/1810/276277
- Ganti et al. 2019 (single-channel rivers before plants):
  https://news.ucsb.edu/2019/019475/aint-no-river-wide-enough
- Stanford 2025, unvegetated rivers can meander:
  https://news.stanford.edu/stories/2025/08/rise-plant-life-changed-how-rivers-move-study-shows
- Nature Reviews Earth & Environment 2021 review on rivers and vegetation:
  https://www.nature.com/articles/s43017-021-00249-6

Life elsewhere
- Kiang et al. 2007, foliage colours under other stars (*Astrobiology*); summary:
  https://www.space.com/3680-colorful-worlds-plants-planets-green.html

Simulation and games
- Hagen et al. 2021, *gen3sis: a general engine for eco-evolutionary simulations*:
  https://www.ncbi.nlm.nih.gov/pmc/articles/PMC8384074/
- Watson & Lovelock 1983, Daisyworld (in SimEarth's manual).
- SimEarth: https://en.wikipedia.org/wiki/SimEarth
- Species: Artificial Life, Real Evolution: https://store.steampowered.com/app/774541
- Thrive: https://revolutionarygamesstudio.com/

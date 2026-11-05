# Polish backlog

Later improvements, not blocking the current milestones. Newest requests first.

## Clouds as weather patterns (requested 2026-11-05)

Make the clouds look like actual weather: high and low pressure systems, not climate-mean
cover broken up by noise.

Today (`crates/worldgen/src/sky.rs`): cloud cover is the annual mean from the climate (rain,
storm-track wind strength, cold-sea stratus), shaped by fBm noise drifting with the zonal
wind (two noise phases cross-fading over `PERIOD`). Plausible from orbit, but no structure:
no cyclones, fronts, clear anticyclones.

What it could become:
- **Moving pressure systems**: lows and highs spawned along the climate's belts (lows on the
  westerlies' storm tracks and the subpolar lows, highs in the subtropics and over cold
  continents in winter), carried east by the westerlies (west in the trades), growing and
  decaying over a few days of game time.
- **Mid-latitude cyclones**: a comma of cloud round each low, the warm front's wide shield
  ahead, the cold front's narrow band trailing toward the equator, clear behind it; spiral
  turning with the hemisphere (anticlockwise in the north).
- **Anticyclones**: clear skies, with low stratus or fog at their edges over cold seas.
- **The tropics**: the ITCZ as clusters of thunderstorms (not a continuous band), easterly
  waves drifting west, tropical cyclones over warm seas in late summer (eye, spiral bands)
  recurving poleward into the westerlies.
- **Seasons**: systems from the month's climate, not the annual mean (the ITCZ and storm
  tracks shifting, monsoons).
- Possibly a coarse pressure field (from `climate.rs`'s belts + the systems) rendered into the
  sky map, the cloud density derived from it (rising air cloudy, sinking air clear), so the
  wind, clouds and later weather (rain on the ground) share one cause.

References to look at then: mid-latitude cyclone models (the Norwegian cyclone model, Bjerknes
1919), satellite imagery of cyclones and fronts, game skies with weather systems.

## Smaller items left from the world parameters (E1, E2)

- Salinity in salt flats (saltier seas: more evaporites on dry coasts).
- Estuaries widening with large tides (`mouths.rs`; the tides are computed later, in the
  surface: move them earlier).
- The sea surface rising and falling with the tide.
- Braided rivers drawn as braids (today: wider, shallower, more bars).
- Sunlight reddened through the air on the ground at the day–night line (the atmosphere's
  transmittance: Bevy's `ATMOSPHERE` bindings, as the water's `through_air`).

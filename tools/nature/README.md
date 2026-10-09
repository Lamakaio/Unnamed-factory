# Nature model pipeline

Builds `assets/models/nature/<name>_lod{0,1,2}.glb` (git-ignored) for
`src/map/vegetation.rs`, with `build.sh` (Blender 5.2 in background mode).

## Sources (CC0)

- **Kenney Nature Kit** (<https://kenney.nl>, `D:\3d\Kenney_Nature_Kit`): low-poly,
  flat-coloured trees, rocks, stumps. The bulk of the forests and rocks.
- **Quaternius Stylized Nature MegaKit**, free Standard version (<https://quaternius.com>,
  `D:\3d\Quaternius_Stylized_Nature`): textured hero trees, boulders, bushes, ground cover.

## `make_nature.py`

For every `name=kind=source[=leaf colour][=scale]` entry:

- **Low-poly kinds** (`lowtree`, `lowrock`, `lowwood`): scaled to metres, recoloured by
  material name to the game's palette (`PALETTE`, leaves from the entry's colour) and baked
  to flat per-face vertex colours as one mesh with one material. LOD0 is the model, LOD1 and
  LOD2 the same model decimated (~90 / ~28 faces for trees).
- **Textured kinds** (`tree`, `bush`, `rock`, `wood`, `ground`): LOD0 is the textured model
  (decimated above a budget; Quaternius `*_C` leaf textures replaced by their white version
  tinted with the leaf colour). LOD1 / LOD2 are toon stand-ins: convex hulls of k-means
  clusters of the foliage plus a trunk prism (trees, bushes), or of the whole model (rocks,
  wood), with flat vertex colours. `ground` has LOD0 only.

Models are in metres; the game scales them by `METERS_TO_WORLD` (0.05, one unit = 20 m).

`thumbs.py` renders previews of any `.glb` / `.gltf` / `.fbx` (contact sheets).

## Unreal packs (not used at the moment)

`unreal_list.py` / `unreal_export.py` export meshes from the purchased Unreal packs headless
(`UnrealEditor-Cmd.exe <project> -run=pythonscript -script=... -EnablePlugins=PythonScriptPlugin
-unattended -nop4 -nosplash -NullRHI`): FBX + textures + a material mapping (the glTF
exporter crashes in the commandlet). `rewire.py` rebuilds their materials in Blender. Those
models go through `make_nature.py` like the textured kinds above.

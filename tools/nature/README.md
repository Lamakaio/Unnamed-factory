# Nature model pipeline

Builds `assets/models/nature/<name>_lod{0,1,2}.glb` (git-ignored, from purchased packs) for
`src/map/vegetation.rs`.

1. **Unreal export** (headless, Unreal 5.5 at `D:\Games\UE_5.5`, project
   `D:\3d\Unreal\Test`): `unreal_list.py` lists every static mesh from the asset registry
   (triangles, size; no loading) into `D:/3d/_export/meshes.tsv`. `unreal_export.py` exports
   the candidate meshes as FBX, their material textures as PNG and a material → textures
   mapping (`materials.json`). It records each attempt, so a crash skips that mesh on the next
   run. The glTF exporter crashes in the commandlet, hence FBX.

   ```
   UnrealEditor-Cmd.exe D:/3d/Unreal/Test/Test.uproject -run=pythonscript
     -script=tools/nature/unreal_export.py -EnablePlugins=PythonScriptPlugin
     -unattended -nop4 -nosplash -NullRHI
   ```

2. **Blender** (`build.sh`, Blender 5.2 in background mode):
   - `rewire.py` rebuilds each material from the mapping (base colour, alpha mask for leaf
     cards, flat colour when there is no texture) and saves `.glb`;
   - `make_nature.py` writes, per model: LOD0 (textured, decimated above a budget, base at
     y = 0), LOD1 (~120 triangles) and LOD2 (~25 triangles) toon stand-ins: convex hulls of
     k-means clusters of the foliage plus a trunk prism for trees and bushes, the decimated
     hull for rocks; flat per-face vertex colours from the texture averages (or a leaf colour
     override), one mesh and one material each.
   - `thumbs.py` renders previews (contact sheets) of any `.glb`/`.fbx`.

Models are in metres; the game scales them by `METERS_TO_WORLD` (0.05, one unit = 20 m).

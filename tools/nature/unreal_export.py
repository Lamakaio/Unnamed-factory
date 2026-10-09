# Run inside Unreal (pythonscript commandlet): export candidate nature meshes to glTF binary
# (.glb, materials baked to textures) into D:/3d/_export/candidates/<pack>/.
import os

import unreal

CANDIDATES = """
StylizedProvencal/Meshes/SM_Tree_01 SM_Tree_02 SM_Tree_03 SM_Tree_Cypress SM_Bush_01 SM_Bush_02
  SM_Bush_03 SM_Bush_04 SM_Rock_Large_01 SM_Rock_Large_02 SM_Rock_Small_01 SM_Rock_Small_02
  SM_Rock_Small_03 SM_Rock_Cliff_01 SM_GrassField_01_a SM_GrassField_02_a SM_Grass_Small_01_a
  SM_Flower_01_c SM_Flower_02_a SM_Flower_03_a
NordicVillage/Meshes/Trees/SM_TreeSmall
NordicVillage/Meshes/Foliage/SM_Bush SM_Fern
NordicVillage/Meshes/RocksAndPebbles/SM_Rock01 SM_Rock02 SM_Rock03 SM_Rock04 SM_Rock05 SM_RockSmall01
NordicVillage/Meshes/Grass/SM_Grass01
Fantastic_Hills/Meshes/SM_Tree_01 SM_Tree_02 SM_Tree_0_2 SM_TreeCascade_NM_01 SM_PineTree05
  SM_Bushes SM_GrassFoliage2 SM_grassCluster_01 SM_Rock_01 SM_Rock_05 SM_Rock_10 SM_GroupedRock_1
Whispering_Grove/Meshes/SM_Tree_05 SM_Tree_03 SM_Small_Tree_01 SM_Fern_01 SM_Plant_02 SM_Rock_01
Taiga_Biome/Meshes/SM_Small_Pine_01 SM_Fir_Tree_05 SM_Dead_Pine_01 SM_Pine_Stump_01 SM_Log_Set_02
  SM_MID_Rock_01
HouseAlongRiver/Meshes/SM_Tree_Leaf_01 SM_Bush_Parrot SM_Rock
"""

registry = unreal.AssetRegistryHelpers.get_asset_registry()
registry.search_all_assets(True)


def find(folder, name):
    """Package path of a mesh, searching under the pack folder (the listing dropped subfolders)."""
    pack = folder.split("/")[0]
    for data in registry.get_assets_by_path(f"/Game/{pack}", recursive=True):
        if str(data.asset_name) == name and str(data.asset_class_path.asset_name) == "StaticMesh":
            return str(data.package_name)
    return None


jobs = []
folder = None
for line in CANDIDATES.split("\n"):
    words = line.split()
    if not words:
        continue
    if "/" in words[0]:
        folder = words[0]
        words = words[1:]
    jobs += [(folder, name) for name in words]

import json

# Mesh geometry as FBX (the glTF exporter crashes in the commandlet): materials keep their names, and
# their textures are exported separately with a JSON mapping, to be rewired in Blender.
fbx_options = unreal.FbxExportOption()
fbx_options.collision = False
fbx_options.level_of_detail = False
attempted_path = "D:/3d/_export/attempted.txt"
attempted = set(open(attempted_path).read().split()) if os.path.exists(attempted_path) else set()
mapping_path = "D:/3d/_export/materials.json"
mapping = json.load(open(mapping_path)) if os.path.exists(mapping_path) else {}
texture_dir = "D:/3d/_export/textures"
os.makedirs(texture_dir, exist_ok=True)


def export_texture(texture):
    out = f"{texture_dir}/{texture.get_name()}.png"
    if not os.path.exists(out):
        task = unreal.AssetExportTask()
        task.object = texture
        task.filename = out
        task.automated = True
        task.prompt = False
        task.replace_identical = True
        task.exporter = unreal.TextureExporterPNG()
        unreal.Exporter.run_asset_export_task(task)
    return os.path.basename(out)


def describe(material):
    """Texture and scalar/vector parameters of a material (instance), following parents."""
    textures, vectors = {}, {}
    current = material
    while current is not None:
        if isinstance(current, unreal.MaterialInstance):
            for value in current.get_editor_property("texture_parameter_values"):
                name = str(value.parameter_info.name)
                if value.parameter_value is not None and name not in textures:
                    textures[name] = export_texture(value.parameter_value)
            for value in current.get_editor_property("vector_parameter_values"):
                name = str(value.parameter_info.name)
                if name not in vectors:
                    c = value.parameter_value
                    vectors[name] = [c.r, c.g, c.b, c.a]
            current = current.get_editor_property("parent")
        else:
            break
    blend = str(material.get_blend_mode()) if hasattr(material, "get_blend_mode") else ""
    return {"textures": textures, "vectors": vectors, "blend": blend}


exported = 0
for folder, name in jobs:
    key = f"{folder.split('/')[0]}/{name}"
    if key in attempted:
        continue
    with open(attempted_path, "a") as f:
        f.write(key + chr(10))
    path = find(folder, name)
    if path is None:
        unreal.log_warning(f"EXPORT missing {key}")
        continue
    mesh = unreal.load_asset(path)
    pack = folder.split("/")[0]
    out_dir = f"D:/3d/_export/candidates/{pack}"
    os.makedirs(out_dir, exist_ok=True)
    task = unreal.AssetExportTask()
    task.object = mesh
    task.filename = f"{out_dir}/{name}.fbx"
    task.automated = True
    task.prompt = False
    task.replace_identical = True
    task.options = fbx_options
    task.exporter = unreal.StaticMeshExporterFBX()
    ok = unreal.Exporter.run_asset_export_task(task)
    for slot in mesh.static_materials:
        material = slot.material_interface
        if material is not None and material.get_name() not in mapping:
            mapping[material.get_name()] = describe(material)
    with open(mapping_path, "w") as f:
        json.dump(mapping, f, indent=1)
    unreal.log(f"EXPORT {'ok' if ok else 'FAILED'} {key}")
    exported += 1 if ok else 0
unreal.log(f"EXPORTED {exported}")

# Run inside Unreal (pythonscript commandlet): list every static mesh of the project from the
# asset registry tags (no asset loading, so no mesh builds) into D:/3d/_export/meshes.tsv.
import unreal

registry = unreal.AssetRegistryHelpers.get_asset_registry()
registry.search_all_assets(True)
filter = unreal.ARFilter(
    class_paths=[unreal.TopLevelAssetPath("/Script/Engine", "StaticMesh")],
    package_paths=["/Game"],
    recursive_paths=True,
)
lines = ["path\ttriangles\tvertices\tsize\tmaterials"]
for data in registry.get_assets(filter):
    tag = lambda name: str(data.get_tag_value(name) or "")
    lines.append(
        f"{data.package_name}\t{tag('Triangles')}\t{tag('Vertices')}\t{tag('ApproxSize')}\t{tag('Materials')}"
    )
with open("D:/3d/_export/meshes.tsv", "w", encoding="utf-8") as f:
    f.write("\n".join(lines))
unreal.log(f"LISTED {len(lines) - 1} meshes")

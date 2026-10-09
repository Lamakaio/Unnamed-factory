# Blender (background): rebuild the materials of glTF meshes exported from Unreal without
# textures, from the material -> textures mapping exported alongside, and save new .glb files.
# Usage: blender -b -P rewire.py -- <materials.json> <texture_dir> <out_dir> <model.glb>...
import json
import os
import sys

import bpy

args = sys.argv[sys.argv.index("--") + 1 :]
mapping_path, texture_dir, out_dir = (os.path.abspath(a) for a in args[:3])
models = [os.path.abspath(m) for m in args[3:]]
mapping = json.load(open(mapping_path))
os.makedirs(out_dir, exist_ok=True)

BASE_TOKENS = {"base", "bc", "albedo", "diffuse", "basecolor", "color", "colour", "albedoalpha"}
OPACITY_TOKENS = {"opacity", "alpha", "o"}
LEAF_MASK = {"leaf_mask"}
SKIP_TOKENS = {"sss", "moss", "wind", "gradient", "normal", "n", "nrm", "orm", "aor", "r", "rough",
               "roughness", "ao", "spec", "height", "emissive"}


def tokens(name):
    import re
    return [t for t in re.split(r"[^a-z0-9]+", name.lower()) if t]


def pick_base(textures):
    """The colour texture: a parameter with a base/colour token and nothing that rules it out."""
    best = None
    for name, file in textures.items():
        words = tokens(name)
        if any(w in SKIP_TOKENS for w in words) and not any(w in BASE_TOKENS for w in words):
            continue
        if any(w in ("sss", "moss", "gradient") for w in words):
            continue
        if any(w in BASE_TOKENS for w in words) or name.lower().startswith("bc"):
            # Prefer the rock/main layer over secondary ones.
            if best is None or "rock" in name.lower() or name.lower().startswith("zl"):
                best = file
    return best


def pick_opacity(textures):
    for name, file in textures.items():
        low = name.lower()
        if any(k in low for k in ("sss", "wind")):
            continue
        if low in LEAF_MASK or any(w in OPACITY_TOKENS for w in tokens(name)):
            return file
    return None


def load_image(file, color):
    path = os.path.join(texture_dir, file)
    if not os.path.exists(path):
        return None
    image = bpy.data.images.load(path, check_existing=True)
    image.colorspace_settings.name = "sRGB" if color else "Non-Color"
    return image


def rebuild(material, info):
    textures = info.get("textures", {})
    base_file = pick_base(textures)
    opacity_file = pick_opacity(textures)
    masked = "MASKED" in info.get("blend", "").upper() or opacity_file is not None
    material.use_nodes = True
    nodes, links = material.node_tree.nodes, material.node_tree.links
    nodes.clear()
    output = nodes.new("ShaderNodeOutputMaterial")
    bsdf = nodes.new("ShaderNodeBsdfPrincipled")
    bsdf.inputs["Roughness"].default_value = 0.9
    links.new(bsdf.outputs["BSDF"], output.inputs["Surface"])
    base = load_image(base_file, True) if base_file else None
    if base is not None:
        tex = nodes.new("ShaderNodeTexImage")
        tex.image = base
        links.new(tex.outputs["Color"], bsdf.inputs["Base Color"])
        alpha_source = tex.outputs["Alpha"]
    else:
        vectors = info.get("vectors", {})
        named = [v for k, v in vectors.items() if "color" in k.lower() or "tint" in k.lower()]
        color = (named or list(vectors.values()) or [[0.5, 0.5, 0.5, 1.0]])[0]
        bsdf.inputs["Base Color"].default_value = color
        alpha_source = None
    if opacity_file:
        opacity = load_image(opacity_file, False)
        if opacity is not None:
            tex = nodes.new("ShaderNodeTexImage")
            tex.image = opacity
            alpha_source = tex.outputs["Color"]
    if masked and alpha_source is not None:
        # Rounded alpha is exported as glTF alphaMode MASK.
        clip = nodes.new("ShaderNodeMath")
        clip.operation = "ROUND"
        links.new(alpha_source, clip.inputs[0])
        links.new(clip.outputs[0], bsdf.inputs["Alpha"])
        material.surface_render_method = "DITHERED"
    return base_file, opacity_file


for model in models:
    bpy.ops.wm.read_factory_settings(use_empty=True)
    if model.lower().endswith(".fbx"):
        bpy.ops.import_scene.fbx(filepath=model)
    else:
        bpy.ops.import_scene.gltf(filepath=model)
    report = []
    for material in bpy.data.materials:
        info = mapping.get(material.name) or mapping.get(material.name.split(".")[0])
        if info is None:
            report.append(f"{material.name}: no mapping")
            continue
        base, opacity = rebuild(material, info)
        report.append(f"{material.name}: base={base} opacity={opacity}")
    out = os.path.join(out_dir, os.path.splitext(os.path.basename(model))[0] + ".glb")
    bpy.ops.export_scene.gltf(filepath=out, export_format="GLB", export_yup=True)
    print(f"REWIRED {os.path.basename(model)}: " + "; ".join(report))

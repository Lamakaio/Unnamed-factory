# Blender (background): build the game's nature models with their levels of detail.
#
# For every entry `name=kind=source.glb`, writes into <out_dir>:
# - <name>_lod0.glb: the textured model (decimated above a triangle budget), base at z = 0;
# - <name>_lod1.glb, <name>_lod2.glb (trees, bushes, rocks): low-poly toon stand-ins with flat
#   per-face vertex colours averaged from the textures. Trees and bushes: convex hull of the
#   foliage + a trunk prism; rocks: the decimated rock.
#
# Usage: blender -b -P make_nature.py -- <out_dir> name=kind=path ...
import os
import random
import sys

import bmesh
import bpy
import numpy as np
from mathutils import Vector

args = sys.argv[sys.argv.index("--") + 1 :]
out_dir = os.path.abspath(args[0])
os.makedirs(out_dir, exist_ok=True)
# name=kind=path[=leaf colour as sRGB hex, overriding the texture average]
entries = [(a.split("=") + [""])[:4] for a in args[1:]]

LOD0_BUDGET = {"tree": 4000, "bush": 2000, "rock": 1500, "ground": 600, "wood": 2000}
# Faces of the stand-ins: (lod1, lod2) for the foliage hull, and trunk sides.
HULL_FACES = {"tree": (36, 18), "bush": (24, 10), "rock": (80, 14), "wood": (60, 12)}
# Foliage clusters (separate hulls) of the stand-ins: lobed crowns, tiered pines.
CLUSTERS = {"tree": (3, 1), "bush": (2, 1)}
TRUNK_SIDES = (5, 3)


def reset():
    bpy.ops.wm.read_factory_settings(use_empty=True)


def triangles(obj):
    return sum(len(p.vertices) - 2 for p in obj.data.polygons)


def load(path):
    bpy.ops.import_scene.gltf(filepath=os.path.abspath(path))
    meshes = [o for o in bpy.context.scene.objects if o.type == "MESH"]
    bpy.ops.object.select_all(action="DESELECT")
    for o in meshes:
        o.select_set(True)
    bpy.context.view_layer.objects.active = meshes[0]
    if len(meshes) > 1:
        bpy.ops.object.join()
    obj = bpy.context.view_layer.objects.active
    for o in list(bpy.context.scene.objects):
        if o != obj and o.type != "MESH":
            bpy.data.objects.remove(o, do_unlink=True)
    obj.parent = None
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
    # Base at z = 0 (keep the pivot's x, y: the trunk).
    lowest = min(v.co.z for v in obj.data.vertices)
    for v in obj.data.vertices:
        v.co.z -= lowest
    return obj


def decimate(obj, target):
    count = triangles(obj)
    if count <= target:
        return
    modifier = obj.modifiers.new("decimate", "DECIMATE")
    modifier.ratio = target / count
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.modifier_apply(modifier=modifier.name)


def is_foliage(material):
    if material is None:
        return False
    if getattr(material, "surface_render_method", "") == "DITHERED":
        return True
    name = material.name.lower()
    return any(k in name for k in ("leaf", "leaves", "flora", "bush", "tree", "foliage", "branch"))


def srgb_to_linear(c):
    return [x / 12.92 if x <= 0.04045 else ((x + 0.055) / 1.055) ** 2.4 for x in c]


def mean_color(material):
    """Average linear colour of a material's base colour texture (opaque texels only)."""
    if material is None or not material.use_nodes:
        return [0.4, 0.4, 0.4]
    for node in material.node_tree.nodes:
        if node.type == "TEX_IMAGE" and node.image is not None and node.image.size[0] > 0:
            image = node.image
            if image.colorspace_settings.name != "sRGB":
                continue
            pixels = np.empty(image.size[0] * image.size[1] * 4, dtype=np.float32)
            image.pixels.foreach_get(pixels)
            pixels = pixels.reshape(-1, 4)[::7]
            opaque = pixels[pixels[:, 3] > 0.5]
            if len(opaque) == 0:
                opaque = pixels
            return srgb_to_linear(list(opaque[:, :3].mean(axis=0)))
    for node in material.node_tree.nodes:
        if node.type == "BSDF_PRINCIPLED":
            return list(node.inputs["Base Color"].default_value)[:3]
    return [0.4, 0.4, 0.4]


def export(objects, path, vertex_colors):
    bpy.ops.object.select_all(action="DESELECT")
    for o in objects:
        o.select_set(True)
    bpy.ops.export_scene.gltf(
        filepath=path,
        export_format="GLB",
        use_selection=True,
        export_apply=True,
        export_yup=True,
        export_vertex_color="ACTIVE" if vertex_colors else "MATERIAL",
        export_all_vertex_colors=False,
    )


def hull_object(name, points, faces):
    mesh = bpy.data.meshes.new(name)
    bm = bmesh.new()
    for p in points:
        bm.verts.new(p)
    bmesh.ops.convex_hull(bm, input=bm.verts)
    loose = [v for v in bm.verts if not v.link_faces]
    bmesh.ops.delete(bm, geom=loose, context="VERTS")
    bm.to_mesh(mesh)
    bm.free()
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.scene.collection.objects.link(obj)
    decimate(obj, faces)
    return obj


def clusters(points, k, seed=1):
    """k-means of the foliage points (height counts double, so pines split into tiers)."""
    data = np.array([(p.x, p.y, p.z * 2.0) for p in points])
    if k <= 1 or len(data) < k * 4:
        return [points]
    rng = np.random.default_rng(seed)
    centers = data[rng.choice(len(data), k, replace=False)]
    for _ in range(20):
        labels = np.argmin(((data[:, None, :] - centers[None]) ** 2).sum(-1), axis=1)
        for c in range(k):
            if (labels == c).any():
                centers[c] = data[labels == c].mean(axis=0)
    return [[points[i] for i in np.nonzero(labels == c)[0]] for c in range(k) if (labels == c).sum() >= 4]


def prism(name, base_radius, top_radius, top, sides):
    mesh = bpy.data.meshes.new(name)
    bm = bmesh.new()
    bmesh.ops.create_cone(
        bm, cap_ends=True, segments=sides, radius1=base_radius, radius2=top_radius, depth=top
    )
    bmesh.ops.translate(bm, verts=bm.verts, vec=Vector((0, 0, top / 2)))
    bm.to_mesh(mesh)
    bm.free()
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.scene.collection.objects.link(obj)
    return obj


def paint(obj, color, height_range, seed):
    """Flat per-face colours: the base colour, darker toward the bottom, slightly varied."""
    rng = random.Random(seed)
    mesh = obj.data
    attribute = mesh.color_attributes.new("Col", "FLOAT_COLOR", "CORNER")
    mesh.color_attributes.active_color = attribute
    low, high = height_range
    for polygon in mesh.polygons:
        z = sum(mesh.vertices[v].co.z for v in polygon.vertices) / len(polygon.vertices)
        t = 0.0 if high <= low else (z - low) / (high - low)
        shade = (0.72 + 0.32 * max(0.0, min(1.0, t))) * (1 + rng.uniform(-0.06, 0.06))
        c = [min(1.0, x * shade) for x in color] + [1.0]
        for loop in polygon.loop_indices:
            attribute.data[loop].color = c
    for polygon in mesh.polygons:
        polygon.use_smooth = False
    material = bpy.data.materials.get("proxy")
    if material is None:
        # One shared material, so the joined stand-in is a single primitive.
        material = bpy.data.materials.new("proxy")
        material.use_nodes = True
        nodes, links = material.node_tree.nodes, material.node_tree.links
        bsdf = nodes.get("Principled BSDF")
        attr = nodes.new("ShaderNodeVertexColor")
        attr.layer_name = "Col"
        links.new(attr.outputs["Color"], bsdf.inputs["Base Color"])
        bsdf.inputs["Roughness"].default_value = 1.0
    mesh.materials.clear()
    mesh.materials.append(material)


def hex_color(text):
    return srgb_to_linear([int(text[i : i + 2], 16) / 255 for i in (0, 2, 4)])


for name, kind, path, leaf_hex in entries:
    reset()
    obj = load(path)
    source_tris = triangles(obj)
    vertices = [obj.matrix_world @ v.co for v in obj.data.vertices]
    height = max(v.z for v in vertices)
    radius = max((Vector((v.x, v.y)).length for v in vertices), default=0.0)

    # Foliage vs trunk points, and their colours.
    foliage, trunk = [], []
    foliage_colors, trunk_colors = [], []
    for polygon in obj.data.polygons:
        material = obj.material_slots[polygon.material_index].material if obj.material_slots else None
        points = [vertices[v] for v in polygon.vertices]
        (foliage if is_foliage(material) else trunk).extend(points)
    for slot in obj.material_slots:
        (foliage_colors if is_foliage(slot.material) else trunk_colors).append(mean_color(slot.material))

    lod0 = obj.copy()
    lod0.data = obj.data.copy()
    bpy.context.scene.collection.objects.link(lod0)
    decimate(lod0, LOD0_BUDGET[kind])
    export([lod0], os.path.join(out_dir, f"{name}_lod0.glb"), vertex_colors=False)
    report = f"NATURE {name} {kind} height {height:.2f} radius {radius:.2f} tris {source_tris}->{triangles(lod0)}"

    if kind in HULL_FACES:
        average = lambda colors, fallback: (
            [sum(c[k] for c in colors) / len(colors) for k in range(3)] if colors else fallback
        )
        leaf_color = hex_color(leaf_hex) if leaf_hex else average(foliage_colors, [0.15, 0.3, 0.08])
        bark_color = average(trunk_colors, [0.2, 0.13, 0.08])
        for level in (0, 1):
            parts = []
            if kind in ("tree", "bush") and foliage:
                # Ignore the lowest stray leaves (they would pull the crown down the trunk).
                cut = np.percentile([p.z for p in foliage], 6 if kind == "tree" else 0)
                crown = [p for p in foliage if p.z >= cut]
                low = min(p.z for p in crown)
                for c, cluster in enumerate(clusters(crown, CLUSTERS[kind][level])):
                    canopy = hull_object(f"canopy{level}_{c}", cluster, HULL_FACES[kind][level])
                    paint(canopy, leaf_color, (low, height), hash(name) + level * 10 + c)
                    parts.append(canopy)
                if kind == "tree":
                    trunk_radius = max(0.15, radius * 0.06)
                    stem = prism(f"trunk{level}", trunk_radius, trunk_radius * 0.6,
                                 low + (height - low) * 0.3, TRUNK_SIDES[level])
                    paint(stem, bark_color, (0, height), 7)
                    parts.append(stem)
            else:
                points = trunk + foliage
                rock = hull_object(f"hull{level}", points, HULL_FACES[kind][level])
                color = average(trunk_colors + foliage_colors, [0.4, 0.38, 0.36])
                paint(rock, color, (0, height), hash(name) + level)
                parts.append(rock)
            # One mesh, one material: a single instance to draw.
            bpy.ops.object.select_all(action="DESELECT")
            for part in parts:
                part.select_set(True)
            bpy.context.view_layer.objects.active = parts[0]
            if len(parts) > 1:
                bpy.ops.object.join()
            parts = [bpy.context.view_layer.objects.active]
            export(parts, os.path.join(out_dir, f"{name}_lod{level + 1}.glb"), vertex_colors=True)
            report += f" lod{level + 1} {sum(triangles(p) for p in parts)}"
    print(report)

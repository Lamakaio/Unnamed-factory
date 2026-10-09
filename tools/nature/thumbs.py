# Blender (background): render a shaded thumbnail of every FBX/GLB given after "--".
# Usage: blender -b -P thumbs.py -- <out_dir> <model>...
import math
import os
import sys

import bpy
from mathutils import Vector

args = sys.argv[sys.argv.index("--") + 1 :]
out_dir, models = os.path.abspath(args[0]), [os.path.abspath(m) for m in args[1:]]
os.makedirs(out_dir, exist_ok=True)

scene = bpy.context.scene
scene.render.engine = "BLENDER_WORKBENCH"
scene.render.resolution_x = 320
scene.render.resolution_y = 320
scene.display.shading.light = "STUDIO"
scene.display.shading.color_type = "TEXTURE"
scene.display.shading.show_cavity = True
scene.render.film_transparent = False
scene.world = scene.world or bpy.data.worlds.new("World")

for model in models:
    for obj in list(bpy.data.objects):
        bpy.data.objects.remove(obj, do_unlink=True)
    try:
        if model.lower().endswith(".fbx"):
            bpy.ops.import_scene.fbx(filepath=model)
        else:
            bpy.ops.import_scene.gltf(filepath=model)
    except Exception as error:
        print(f"FAILED {model}: {error}")
        continue
    meshes = [o for o in scene.objects if o.type == "MESH"]
    has_colors = any(o.data.color_attributes for o in meshes)
    scene.display.shading.color_type = "VERTEX" if has_colors else "TEXTURE"
    if not meshes:
        print(f"NOMESH {model}")
        continue
    points = [o.matrix_world @ Vector(c) for o in meshes for c in o.bound_box]
    lo = Vector((min(p.x for p in points), min(p.y for p in points), min(p.z for p in points)))
    hi = Vector((max(p.x for p in points), max(p.y for p in points), max(p.z for p in points)))
    center = (lo + hi) / 2
    radius = max((hi - lo).length / 2, 1e-3)
    camera_data = bpy.data.cameras.new("cam")
    camera_data.lens = 50
    camera = bpy.data.objects.new("cam", camera_data)
    scene.collection.objects.link(camera)
    direction = Vector((1.0, -1.2, 0.6)).normalized()
    camera.location = center + direction * radius * 2.6
    camera.rotation_euler = (center - camera.location).to_track_quat("-Z", "Y").to_euler()
    camera_data.clip_end = radius * 20
    scene.camera = camera
    name = os.path.splitext(os.path.basename(model))[0]
    scene.render.filepath = os.path.join(out_dir, name + ".png")
    bpy.ops.render.render(write_still=True)
    size = hi - lo
    tris = sum(len(p.vertices) - 2 for o in meshes for p in o.data.polygons)
    print(f"THUMB {name} size {size.x:.2f} {size.y:.2f} {size.z:.2f} tris {tris}")

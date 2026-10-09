#!/usr/bin/env bash
# Build assets/models/nature from the source packs (see tools/nature/README.md).
#
# 1. Unreal (headless): export the candidate meshes as FBX and their textures + a material
#    mapping:   UnrealEditor-Cmd.exe <Test.uproject> -run=pythonscript
#                 -script=tools/nature/unreal_export.py -EnablePlugins=PythonScriptPlugin
#                 -unattended -nop4 -nosplash -NullRHI
# 2. Blender: rebuild the materials (rewire.py), then the LODs (make_nature.py, below).
set -euo pipefail

BLENDER=${BLENDER:-"/c/Program Files/Blender Foundation/Blender 5.2/blender.exe"}
EXPORT=${EXPORT:-D:/3d/_export}
OUT=${OUT:-assets/models/nature}
HERE=$(dirname "$0")

for pack in "$EXPORT"/candidates/*/; do
    pack=$(basename "$pack")
    "$BLENDER" -b -P "$HERE/rewire.py" -- "$EXPORT/materials.json" "$EXPORT/textures" \
        "$EXPORT/glb/$pack" "$EXPORT/candidates/$pack"/*.fbx
done

G=$EXPORT/glb
# name=kind=source[=leaf colour override, sRGB hex]
"$BLENDER" -b -P "$HERE/make_nature.py" -- "$OUT" \
    broadleaf_round=tree=$G/StylizedProvencal/SM_Tree_02.glb=4a6c22 \
    broadleaf_tall=tree=$G/Fantastic_Hills/SM_Tree_0_2.glb=3e631f \
    broadleaf_wide=tree=$G/Fantastic_Hills/SM_Tree_02.glb=456524 \
    cypress=tree=$G/StylizedProvencal/SM_Tree_Cypress.glb=2f491e \
    pine=tree=$G/Fantastic_Hills/SM_PineTree05.glb=254225 \
    dead_pine=tree=$G/Taiga_Biome/SM_Dead_Pine_01.glb \
    bush_round=bush=$G/StylizedProvencal/SM_Bush_01.glb=486825 \
    bush_flowers=bush=$G/StylizedProvencal/SM_Bush_04.glb=536c2d \
    bush_leafy=bush=$G/Fantastic_Hills/SM_Bushes.glb=3d5f21 \
    rock_a=rock=$G/NordicVillage/SM_Rock02.glb \
    rock_b=rock=$G/NordicVillage/SM_Rock03.glb \
    rock_c=rock=$G/NordicVillage/SM_Rock04.glb \
    rock_d=rock=$G/NordicVillage/SM_Rock05.glb \
    rock_large_a=rock=$G/StylizedProvencal/SM_Rock_Large_01.glb \
    rock_large_b=rock=$G/StylizedProvencal/SM_Rock_Large_02.glb \
    rock_small_a=rock=$G/StylizedProvencal/SM_Rock_Small_01.glb \
    rock_small_b=rock=$G/StylizedProvencal/SM_Rock_Small_02.glb \
    rock_small_c=rock=$G/StylizedProvencal/SM_Rock_Small_03.glb \
    cliff=rock=$G/StylizedProvencal/SM_Rock_Cliff_01.glb \
    logs=wood=$G/Taiga_Biome/SM_Log_Set_02.glb \
    stump=wood=$G/Taiga_Biome/SM_Pine_Stump_01.glb \
    fern=ground=$G/NordicVillage/SM_Fern.glb \
    flowers_a=ground=$G/StylizedProvencal/SM_Flower_01_c.glb \
    flowers_b=ground=$G/StylizedProvencal/SM_Flower_02_a.glb \
    flowers_c=ground=$G/StylizedProvencal/SM_Flower_03_a.glb \
    grass_a=ground=$G/StylizedProvencal/SM_GrassField_01_a.glb \
    grass_b=ground=$G/StylizedProvencal/SM_GrassField_02_a.glb

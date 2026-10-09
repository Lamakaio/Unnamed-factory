#!/usr/bin/env bash
# Build assets/models/nature (see tools/nature/README.md) from the CC0 packs:
# - Kenney Nature Kit (https://kenney.nl): low-poly trees, rocks, stumps (recoloured, LODs by
#   decimation);
# - Quaternius Stylized Nature MegaKit (https://quaternius.com): textured hero trees, rocks,
#   bushes and ground cover (LOD stand-ins from convex hulls).
set -euo pipefail

BLENDER=${BLENDER:-"/c/Program Files/Blender Foundation/Blender 5.2/blender.exe"}
KENNEY=${KENNEY:-"D:/3d/Kenney_Nature_Kit/Models/GLTF format"}
QUATERNIUS=${QUATERNIUS:-D:/3d/Quaternius_Stylized_Nature/glTF}
OUT=${OUT:-assets/models/nature}
HERE=$(dirname "$0")
K=$KENNEY
Q=$QUATERNIUS

# name=kind=source[=leaf colour, sRGB hex][=scale to metres]
"$BLENDER" -b -P "$HERE/make_nature.py" -- "$OUT" \
    "oak_round=lowtree=$K/tree_default.glb=4a6c22=7" \
    "oak_detailed=lowtree=$K/tree_detailed.glb=456524=7" \
    "oak=lowtree=$K/tree_oak.glb=3e631f=7" \
    "oak_fat=lowtree=$K/tree_fat.glb=4f6f25=7" \
    "poplar=lowtree=$K/tree_tall.glb=3c5e27=7" \
    "pine_round_a=lowtree=$K/tree_pineRoundA.glb=254225=7" \
    "pine_round_c=lowtree=$K/tree_pineRoundC.glb=2b4a2a=7" \
    "pine_round_e=lowtree=$K/tree_pineRoundE.glb=23402a=7" \
    "pine_tall_a=lowtree=$K/tree_pineTallA_detailed.glb=254225=7" \
    "pine_tall_c=lowtree=$K/tree_pineTallC_detailed.glb=2b4a2a=7" \
    "pine_small=lowtree=$K/tree_pineSmallA.glb=2b4a2a=7" \
    "cypress=lowtree=$K/tree_cone.glb=2f491e=7" \
    "umbrella_pine=lowtree=$K/tree_plateau.glb=4a5f28=7" \
    "rock_a=lowrock=$K/rock_largeA.glb==3" \
    "rock_b=lowrock=$K/rock_largeB.glb==3" \
    "rock_c=lowrock=$K/rock_largeC.glb==3" \
    "rock_d=lowrock=$K/rock_largeD.glb==3" \
    "rock_e=lowrock=$K/rock_largeE.glb==3" \
    "rock_f=lowrock=$K/rock_largeF.glb==3" \
    "rock_tall_a=lowrock=$K/rock_tallA.glb==4" \
    "rock_tall_b=lowrock=$K/rock_tallB.glb==4" \
    "rock_tall_c=lowrock=$K/rock_tallC.glb==4" \
    "rock_tall_d=lowrock=$K/rock_tallD.glb==4" \
    "stone_a=lowrock=$K/stone_largeA.glb==3" \
    "stone_c=lowrock=$K/stone_largeC.glb==3" \
    "stone_e=lowrock=$K/stone_largeE.glb==3" \
    "cactus_tall=lowtree=$K/cactus_tall.glb=5d7f3c=8" \
    "cactus_short=lowtree=$K/cactus_short.glb=5d7f3c=6" \
    "desert_shrub=lowwood=$K/plant_bushSmall.glb=8a8a4a=5" \
    "tall_grass=lowground=$K/grass_large.glb=5f8a2c=3" \
    "dry_tall_grass=lowground=$K/grass_large.glb=b8a35a=3" \
    "reeds=lowground=$K/plant_flatTall.glb=4f6b30=4" \
    "stump=lowwood=$K/stump_old.glb==2.5" \
    "logs=lowwood=$K/log_stack.glb==2.5" \
    "common_tree_a=tree=$Q/CommonTree_1.gltf=4a6c22" \
    "common_tree_b=tree=$Q/CommonTree_3.gltf=456524" \
    "hero_pine=tree=$Q/Pine_2.gltf=254225" \
    "twisted_tree=tree=$Q/TwistedTree_1.gltf=456524" \
    "dead_tree_a=wood=$Q/DeadTree_2.gltf" \
    "dead_tree_b=wood=$Q/DeadTree_4.gltf" \
    "boulder_a=rock=$Q/Rock_Medium_1.gltf" \
    "boulder_b=rock=$Q/Rock_Medium_2.gltf" \
    "boulder_c=rock=$Q/Rock_Medium_3.gltf" \
    "bush_common=bush=$Q/Bush_Common.gltf=486825" \
    "bush_flowers=bush=$Q/Bush_Common_Flowers.gltf=536c2d" \
    "pebbles_a=ground=$Q/Pebble_Round_1.gltf" \
    "pebbles_b=ground=$Q/Pebble_Round_2.gltf" \
    "fern=ground=$Q/Fern_1.gltf" \
    "flowers_a=ground=$Q/Flower_3_Group.gltf" \
    "flowers_b=ground=$Q/Flower_4_Group.gltf" \
    "grass_a=ground=$Q/Grass_Common_Short.gltf" \
    "grass_b=ground=$Q/Grass_Wispy_Short.gltf" \
    "clover=ground=$Q/Clover_1.gltf" \
    "mushrooms=ground=$Q/Mushroom_Common.gltf" \
    "plant=ground=$Q/Plant_1.gltf" \
    "wispy_grass=ground=$Q/Grass_Wispy_Tall.gltf"

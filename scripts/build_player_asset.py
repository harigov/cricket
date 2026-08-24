#!/usr/bin/env python3
"""Build MakeHuman (MPFB2) player character GLBs for the cricket match.

Generates a matrix of body archetypes — height x build x ancestry — each rigged
to MPFB's built-in **Mixamo** skeleton so the existing procedural quaternion
animation in `src/render/player.rs` drives them unchanged, and each wearing
shirt/trousers/shoes shells derived from the body's own bone weight groups.

    scripts/build_player_asset.py --install     # fetch + install MPFB into target/
    scripts/build_player_asset.py --list        # show the archetype matrix
    scripts/build_player_asset.py               # build every archetype
    scripts/build_player_asset.py --only medium_regular_south_asian

The script drives Blender headlessly: it re-executes itself inside
`blender --background --python` so it can be run as an ordinary command.

Requires Blender >= 4.2 on PATH. MPFB2 is downloaded from the Blender extensions
registry into `target/mpfb/` on `--install`; nothing is written outside the repo
and the user's own Blender configuration is left untouched.
"""

from __future__ import annotations

import argparse
import json
import os
import struct
import subprocess
import sys
import urllib.request
from dataclasses import dataclass
from pathlib import Path

sys.dont_write_bytecode = True

REPO_ROOT = Path(__file__).resolve().parents[1]
OUT_DIR = REPO_ROOT / "assets" / "characters" / "players"
WORK_DIR = REPO_ROOT / "target" / "mpfb"
BLENDER_RESOURCES = WORK_DIR / "blender"

# MPFB 2.0.17 from the Blender extensions registry (GPL-3.0-or-later; the base
# mesh, morph targets and rigs it ships are CC0 — see assets/characters/ATTRIBUTION.md).
MPFB_VERSION = "2.0.17"
MPFB_SHA256 = "4f0a879d64a39bf646fbf5f53601ac678855da329d650617dca5737548239a87"
MPFB_URL = (
    f"https://extensions.blender.org/download/sha256:{MPFB_SHA256}/"
    f"add-on-mpfb-v{MPFB_VERSION}.zip"
)

# ---------------------------------------------------------------------------
# Archetype matrix
# ---------------------------------------------------------------------------

# MakeHuman macro sliders run 0..1 around a 0.5 midpoint. Cricketers are adults
# in athletic condition, so `age` and `muscle` stay in a narrow sporting band
# and the visible variation comes from height and weight.
# Calibrated against measured stature, not guessed: 0.42/0.53/0.64 produce
# roughly 1.68 m / 1.78 m / 1.92 m, a believable spread for a cricket XI.
HEIGHTS: dict[str, float] = {"short": 0.42, "medium": 0.53, "tall": 0.64}

# MPFB does not bundle MakeHuman's `macrodetails/universal` targets, so the
# `muscle` and `weight` macro sliders are silently inert — setting them changes
# nothing. Build variation therefore comes from the per-limb detail targets that
# *are* bundled, which gives more direct control anyway.
#
# Names beginning `l-` are mirrored to `r-` automatically. Weights are 0..1.
BUILD_TARGETS: dict[str, list[tuple[str, float]]] = {
    "thin": [
        ("l-upperarm-fat-decr", 0.98),
        ("l-lowerarm-fat-decr", 0.92),
        ("l-upperleg-fat-decr", 0.98),
        ("l-lowerleg-fat-decr", 0.92),
        ("l-upperarm-scale-depth-decr", 0.4),
        ("l-upperleg-scale-depth-decr", 0.4),
        ("l-upperleg-scale-horiz-decr", 0.34),
        ("measure-bust-circ-decr", 0.69),
        ("measure-hips-circ-decr", 0.63),
        ("measure-waist-circ-decr", 0.75),
        ("stomach-tone-incr", 0.92),
    ],
    "regular": [
        ("l-upperarm-muscle-incr", 0.45),
        ("l-lowerarm-muscle-incr", 0.35),
        ("l-upperleg-muscle-incr", 0.50),
        ("l-lowerleg-muscle-incr", 0.40),
        ("l-upperarm-shoulder-muscle-incr", 0.40),
        ("stomach-tone-incr", 0.35),
    ],
    "heavy": [
        ("l-upperarm-fat-incr", 0.9),
        ("l-lowerarm-fat-incr", 0.72),
        ("l-upperleg-fat-incr", 0.8),
        ("l-lowerleg-fat-incr", 0.65),
        ("l-upperarm-scale-depth-incr", 0.51),
        ("l-upperleg-scale-depth-incr", 0.43),
        ("l-upperleg-scale-horiz-incr", 0.36),
        ("measure-bust-circ-incr", 0.8),
        ("measure-hips-circ-incr", 0.72),
        ("measure-waist-circ-incr", 0.94),
        ("stomach-pregnant-incr", 0.55),
        ("stomach-tone-decr", 0.94),
    ],
}

# MakeHuman only ships asian/caucasian/african morph axes. South Asian faces and
# builds are approximated by blending; the visible skin tone is applied at
# runtime from the Rust palette, not baked here.
ANCESTRIES: dict[str, dict[str, float]] = {
    "caucasian": {"caucasian": 1.0, "asian": 0.0, "african": 0.0},
    "south_asian": {"caucasian": 0.40, "asian": 0.45, "african": 0.15},
    "african": {"caucasian": 0.0, "asian": 0.0, "african": 1.0},
}

# Cricket kit: shirt covers torso and upper arms, trousers run to the ankle.
# Each entry is a list of Mixamo bone names whose weighted vertices form the
# garment shell. Adding LeftForeArm/RightForeArm turns the shirt long-sleeved.
# `Neck` is deliberately absent: including it pulls the shell up the throat and
# the shirt reads as a turtleneck. `Hips` is present so the shirt hangs untucked
# over the waistband — it is trimmed to length by `SHIRT_DROP_M`.
SHIRT_BONES_SHORT_SLEEVE = [
    "mixamorig:Hips",
    "mixamorig:Spine",
    "mixamorig:Spine1",
    "mixamorig:Spine2",
    "mixamorig:LeftShoulder",
    "mixamorig:RightShoulder",
    "mixamorig:LeftArm",
    "mixamorig:RightArm",
]
SHIRT_BONES_LONG_SLEEVE = SHIRT_BONES_SHORT_SLEEVE + [
    "mixamorig:LeftForeArm",
    "mixamorig:RightForeArm",
]
TROUSER_BONES = [
    "mixamorig:Hips",
    "mixamorig:LeftUpLeg",
    "mixamorig:RightUpLeg",
    "mixamorig:LeftLeg",
    "mixamorig:RightLeg",
]
SHOE_BONES = [
    "mixamorig:LeftFoot",
    "mixamorig:LeftToeBase",
    "mixamorig:RightFoot",
    "mixamorig:RightToeBase",
]

# Garment shells are pushed this far off the skin (metres). Big enough to stop
# the body poking through during the bowling windmill, small enough to read as
# cloth rather than armour.
GARMENT_OFFSET_M = 0.012
# Extra clearance around the shoulders and elbows, where deformation is worst.
GARMENT_OFFSET_SLEEVE_M = 0.019
# Cricket trousers are loose, not leggings — they need visibly more standoff
# than a shirt, or the anatomy reads straight through them.
TROUSER_OFFSET_M = 0.030
SHOE_OFFSET_M = 0.017
HAIR_OFFSET_M = 0.009
# How far below the hip joint the untucked shirt hem falls (metres).
SHIRT_DROP_M = 0.155
# Fraction of a vertex's own weight that a garment's bones must own for that
# vertex to join the shell. Relative rather than absolute — see `extract_shell`.
GARMENT_SHARE = 0.45

# Shirt UV layout, in the exported glTF's coordinates (glTF flips V relative to
# Blender, so this is what `src/render/kit.rs` actually samples):
#     u = 0.25  back centre    (squad number + player name)
#     u = 0.75  front centre   (team crest)
#     u = 0.0 / 1.0            seam, under the character's left arm
#     v = 0     shoulder line  (top of the texture image)
#     v = 1     hem            (bottom of the texture image)
# The texture must therefore tile horizontally: its left and right edges are
# the two halves of the same underarm seam.
SHIRT_SEAM_TURNS = 0.25


@dataclass(frozen=True)
class Archetype:
    height: str
    build: str
    ancestry: str
    long_sleeve: bool = False

    @property
    def name(self) -> str:
        sleeve = "_ls" if self.long_sleeve else ""
        return f"{self.height}_{self.build}_{self.ancestry}{sleeve}"

    def macro_dict(self) -> dict:
        return {
            "gender": 0.92,
            "age": 0.55,
            "muscle": 0.5,
            "weight": 0.5,
            "proportions": 0.62,
            "height": HEIGHTS[self.height],
            "cupsize": 0.5,
            "firmness": 0.5,
            "race": dict(ANCESTRIES[self.ancestry]),
        }


def default_matrix() -> list[Archetype]:
    """Full height x build x ancestry matrix, short-sleeved."""
    return [
        Archetype(height=h, build=b, ancestry=a)
        for h in HEIGHTS
        for b in BUILD_TARGETS
        for a in ANCESTRIES
    ]


# ---------------------------------------------------------------------------
# Host side: MPFB install and Blender dispatch
# ---------------------------------------------------------------------------


def install_mpfb() -> None:
    BLENDER_RESOURCES.mkdir(parents=True, exist_ok=True)
    archive = WORK_DIR / f"mpfb-{MPFB_VERSION}.zip"
    if not archive.exists():
        print(f"==> downloading MPFB {MPFB_VERSION}")
        with urllib.request.urlopen(MPFB_URL) as response:
            archive.write_bytes(response.read())
    print("==> installing MPFB into", BLENDER_RESOURCES)
    subprocess.run(
        [
            "blender",
            "--background",
            "--command",
            "extension",
            "install-file",
            "-r",
            "user_default",
            "-e",
            str(archive),
        ],
        check=True,
        env=blender_env(),
    )


def blender_env() -> dict[str, str]:
    env = dict(os.environ)
    env["BLENDER_USER_RESOURCES"] = str(BLENDER_RESOURCES)
    return env


def run_in_blender(archetypes: list[Archetype]) -> None:
    payload = json.dumps(
        [
            {
                "name": a.name,
                "macro": a.macro_dict(),
                "build": a.build,
                "long_sleeve": a.long_sleeve,
            }
            for a in archetypes
        ]
    )
    subprocess.run(
        [
            "blender",
            "--background",
            "--python",
            str(Path(__file__).resolve()),
            "--",
            "--blender-child",
            payload,
        ],
        check=True,
        env=blender_env(),
    )


# ---------------------------------------------------------------------------
# Blender side
# ---------------------------------------------------------------------------


def blender_main(archetypes: list[dict]) -> None:
    import bpy  # noqa: PLC0415  (only importable inside Blender)

    OUT_DIR.mkdir(parents=True, exist_ok=True)
    report: list[dict] = []
    for spec in archetypes:
        print(f"\n==> building {spec['name']}")
        clear_scene(bpy)
        body, armature = build_character(bpy, spec["macro"], spec["build"])
        tpose(bpy, armature)
        strip_helpers(bpy, body)
        garments = build_garments(bpy, body, armature, spec["long_sleeve"])
        assign_materials(bpy, body, garments)
        add_bind_action(bpy, armature)
        strip_stray_objects(bpy, [body, *garments])
        out = OUT_DIR / f"{spec['name']}.glb"
        export_glb(bpy, out)
        report.append(measure(out, spec["name"]))

    print("\n==> summary")
    for row in report:
        print(
            f"  {row['name']:34} {row['mb']:5.2f} MB  {row['tris']:6} tris  "
            f"foot_y={row['foot_y']:.4f}  hips_y={row['hips_y']:.4f}  "
            f"head_y={row['head_y']:.3f}  min_y={row['mesh_min_y']:+.4f}  "
            f"{'/'.join(row['materials'])}"
        )
    print(
        "\n  Rust constants (src/render/player.rs): FOOT_BIND_Y and "
        "HIPS_BIND_TRANSLATION must match the archetype you spawn; "
        "BONE_UNITS_PER_METRE = 1.0 for these assets (MPFB exports metres, "
        "unlike Xbot's centimetre armature)."
    )


def clear_scene(bpy) -> None:
    """Wipe the scene *and* its orphaned datablocks between archetypes.

    Blender uniquifies names against everything still in the file, so leaving
    stale datablocks behind silently renames the second archetype's materials to
    `Skin.001` / `Shirt.001` — and the runtime matches slots by exact name.
    """
    for obj in list(bpy.data.objects):
        bpy.data.objects.remove(obj, do_unlink=True)
    for collection in (
        bpy.data.meshes,
        bpy.data.materials,
        bpy.data.armatures,
        bpy.data.actions,
    ):
        for datablock in list(collection):
            collection.remove(datablock)


def mpfb_service(name: str):
    """Import an MPFB service regardless of extension vs legacy addon install."""
    import importlib

    for root in ("bl_ext.user_default.mpfb", "mpfb"):
        try:
            return importlib.import_module(f"{root}.services.{name.lower()}")
        except ModuleNotFoundError:
            continue
    raise RuntimeError(
        "MPFB is not installed for this Blender. Run with --install first."
    )


def build_character(bpy, macro: dict, build: str):
    human_service = mpfb_service("humanservice").HumanService
    body = human_service.create_human(macro_detail_dict=macro)
    apply_build_targets(body, build)
    human_service.add_builtin_rig(body, "mixamo")
    armature = next(o for o in bpy.data.objects if o.type == "ARMATURE")
    body.name = "Body"
    # MUST stay "Armature", matching Xbot.glb's root node name.
    #
    # Bevy's glTF loader builds each animation target's id from the node-name
    # path starting at the *scene root*, root name included (`collect_path` in
    # bevy_gltf). The bundled idle/run clips come from Xbot, so a different root
    # name silently fails to bind every channel and figures freeze in bind pose.
    armature.name = "Armature"
    return body, armature


def apply_build_targets(body, build: str) -> None:
    """Shape the physique with per-limb detail targets.

    Each is loaded as a weighted shape key; `apply_pose_as_rest_pose` later
    bakes them into the mesh, so nothing survives into the exported glTF as a
    morph target.
    """
    target_service = mpfb_service("targetservice").TargetService
    for name, weight in BUILD_TARGETS[build]:
        names = [name, "r-" + name[2:]] if name.startswith("l-") else [name]
        for target_name in names:
            path = target_service.target_full_path(target_name)
            if not path:
                print(f"   ! target not found, skipped: {target_name}")
                continue
            target_service.load_target(body, path, weight=weight, name=target_name)


def tpose(bpy, armature) -> None:
    """Bake a T-pose as the rest pose.

    MakeHuman rests in an A-pose (arms ~42 degrees below horizontal). Every
    procedural pose in `src/render/player.rs` is authored as a bone-local delta
    onto a **T-pose** bind — `arms_bind_neutral` in particular — so the arms
    must be levelled and the result baked into the rest pose before export.
    """
    from mathutils import Vector  # noqa: PLC0415

    rig_service = mpfb_service("rigservice").RigService

    bpy.context.view_layer.objects.active = armature
    bpy.ops.object.mode_set(mode="POSE")
    # Aim each arm bone down the world X axis, parents before children so the
    # child's corrected direction is measured against an already-levelled parent.
    chains = [
        (["mixamorig:LeftArm", "mixamorig:LeftForeArm", "mixamorig:LeftHand"], 1.0),
        (["mixamorig:RightArm", "mixamorig:RightForeArm", "mixamorig:RightHand"], -1.0),
    ]
    for bone_names, sign in chains:
        for bone_name in bone_names:
            aim_bone(bpy, armature, bone_name, Vector((sign, 0.0, 0.0)))
    bpy.ops.object.mode_set(mode="OBJECT")

    # Bakes morph targets, applies the armature modifier on every child mesh,
    # applies the pose as the new rest pose, then re-binds the meshes.
    rig_service.apply_pose_as_rest_pose(armature)


def aim_bone(bpy, armature, bone_name: str, target_dir) -> None:
    pose_bone = armature.pose.bones.get(bone_name)
    if pose_bone is None:
        return
    current = (pose_bone.tail - pose_bone.head).normalized()
    rotation = current.rotation_difference(target_dir.normalized())
    pose_bone.matrix = rotation.to_matrix().to_4x4() @ pose_bone.matrix
    bpy.context.view_layer.update()


def strip_helpers(bpy, body) -> None:
    """Apply the helper mask so hair/eye/tights proxy geometry is deleted.

    MPFB hides helper geometry behind a MASK modifier rather than removing it.
    Left in place it survives glTF export and roughly triples the file size.
    """
    bpy.context.view_layer.objects.active = body
    for modifier in list(body.modifiers):
        if modifier.type != "MASK":
            continue
        # Applying a modifier that is not first warns and can bake the armature
        # deformation in with it; hoist the mask to the top of the stack first.
        bpy.ops.object.modifier_move_to_index(modifier=modifier.name, index=0)
        bpy.ops.object.modifier_apply(modifier=modifier.name)


def build_garments(bpy, body, armature, long_sleeve: bool) -> list:
    shirt_bones = SHIRT_BONES_LONG_SLEEVE if long_sleeve else SHIRT_BONES_SHORT_SLEEVE
    hips_z = armature.pose.bones["mixamorig:Hips"].head.z
    garments = []
    specs = [
        # Shirt takes a lower share because it must claim waist vertices that
        # `Hips` otherwise dominates, then gets trimmed to hem length below.
        ("Shirt", shirt_bones, GARMENT_OFFSET_SLEEVE_M, 0.30, hips_z - SHIRT_DROP_M, 2),
        ("Pants", TROUSER_BONES, TROUSER_OFFSET_M, GARMENT_SHARE, None, 4),
        ("Shoes", SHOE_BONES, SHOE_OFFSET_M, GARMENT_SHARE, None, 1),
    ]
    for name, bones, offset, share, z_floor, smooth in specs:
        shell = extract_shell(
            bpy, body, bones, name, offset, share, z_floor, smooth, name == "Shoes"
        )
        if shell is None:
            print(f"   ! {name}: no vertices matched, skipped")
            continue
        bind_to_armature(bpy, shell, armature)
        if name == "Shirt":
            cylindrical_unwrap(bpy, shell)
        garments.append(shell)

    # Hair is a scalp cap rather than a bone-weighted region, so it comes from
    # the base mesh's own `scalp` vertex group. A bald head reads badly at the
    # close broadcast cameras and MakeHuman's community hair assets carry their
    # own licences, so we derive one instead.
    hair = extract_group_shell(bpy, body, "scalp", "Hair", HAIR_OFFSET_M)
    if hair is not None:
        bind_to_armature(bpy, hair, armature)
        garments.append(hair)
    else:
        print("   ! Hair: scalp group not found, skipped")
    return garments


def extract_group_shell(bpy, body, group: str, name: str, offset: float):
    """Shell built from one explicit vertex group, for non-bone regions."""
    import bmesh  # noqa: PLC0415

    if group not in body.vertex_groups:
        return None
    index = body.vertex_groups[group].index

    mesh = body.data.copy()
    shell = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(shell)

    bm = bmesh.new()
    bm.from_mesh(mesh)
    bm.verts.ensure_lookup_table()
    deform = bm.verts.layers.deform.verify()
    doomed = [v for v in bm.verts if v[deform].get(index, 0.0) < 0.5]
    bmesh.ops.delete(bm, geom=doomed, context="VERTS")
    for vert in bm.verts:
        vert.co += vert.normal * offset
    bm.to_mesh(mesh)
    bm.free()

    if not mesh.polygons:
        bpy.data.objects.remove(shell, do_unlink=True)
        return None
    return shell


def extract_shell(
    bpy,
    body,
    bone_groups: list[str],
    name: str,
    offset: float,
    share: float = GARMENT_SHARE,
    z_floor: float | None = None,
    smooth: int = 0,
    flatten_sole: bool = False,
):
    """Duplicate the body vertices weighted to `bone_groups` into a garment shell.

    Because the shell is copied from the already-skinned body it inherits the
    body's vertex groups verbatim, so it deforms identically with no weight
    transfer or proxy fitting step.
    """
    import bmesh  # noqa: PLC0415

    indices = {
        body.vertex_groups[g].index for g in bone_groups if g in body.vertex_groups
    }
    # Denominator must count bone weights only. The MakeHuman base mesh also
    # carries ~130 non-deform groups (`body`, `joint-*`, `helper-*`, `Mid`...)
    # at weight 1.0, which would swamp any share computed over every group.
    bone_indices = {
        g.index for g in body.vertex_groups if g.name.startswith("mixamorig:")
    }
    if not indices:
        return None

    mesh = body.data.copy()
    shell = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(shell)

    bm = bmesh.new()
    bm.from_mesh(mesh)
    bm.verts.ensure_lookup_table()
    deform = bm.verts.layers.deform.verify()
    # Keep a vertex when the garment's bones own the *majority share* of its
    # weight, rather than testing an absolute weight. MPFB's mixamo weights are
    # not normalised to 1.0 per vertex — foot weights peak around 0.40 — so an
    # absolute threshold silently produces empty shoes.
    doomed = []
    for vert in bm.verts:
        weights = vert[deform]
        total = sum(w for i, w in weights.items() if i in bone_indices)
        mine = sum(weights.get(i, 0.0) for i in indices)
        below_hem = z_floor is not None and vert.co.z < z_floor
        if total <= 0.0 or mine / total < share or below_hem:
            doomed.append(vert)
    bmesh.ops.delete(bm, geom=doomed, context="VERTS")
    # Push the shell off the skin along the vertex normals so it reads as cloth
    # laid over the body rather than a decal on it.
    for vert in bm.verts:
        vert.co += vert.normal * offset
    # Relaxing the shell washes out the anatomy underneath — knees, crotch and
    # calf definition — so trousers read as cloth draped over a leg rather than
    # as a second skin.
    for _ in range(smooth):
        bmesh.ops.smooth_vert(
            bm, verts=bm.verts, factor=0.5, use_axis_x=True, use_axis_y=True, use_axis_z=True
        )
    if flatten_sole:
        # Offsetting along the normals pushes the sole through the pitch. Clamp
        # it flat at ground level so every mesh in the asset sits at z >= 0 and
        # the runtime needs no per-archetype ground offset.
        for vert in bm.verts:
            vert.co.z = max(vert.co.z, 0.0)
    bm.to_mesh(mesh)
    bm.free()

    if not mesh.polygons:
        bpy.data.objects.remove(shell, do_unlink=True)
        return None
    solidify = shell.modifiers.new("thickness", "SOLIDIFY")
    solidify.thickness = 0.004
    solidify.offset = 1.0
    return shell


def bind_to_armature(bpy, obj, armature) -> None:
    obj.parent = armature
    if not any(m.type == "ARMATURE" for m in obj.modifiers):
        modifier = obj.modifiers.new("armature", "ARMATURE")
        modifier.object = armature


def cylindrical_unwrap(bpy, obj) -> None:
    """Unwrap the shirt cylindrically about the torso's own vertical axis.

    Blender is Z-up and the character faces -Y, so the angle is measured from
    the front. `SHIRT_SEAM_TURNS` rotates the seam under the left arm, leaving
    the back and the front each contiguous for the number/name and crest
    artwork composited in `src/render/kit.rs`.
    """
    import math  # noqa: PLC0415

    mesh = obj.data
    uv_layer = mesh.uv_layers.active or mesh.uv_layers.new(name="UVMap")
    coords = [v.co for v in mesh.vertices]
    # Project about the shell's own centre, not the armature origin — the torso
    # sits forward of x=y=0, which otherwise skews front/back off their marks.
    centre_x = (min(c.x for c in coords) + max(c.x for c in coords)) * 0.5
    centre_y = (min(c.y for c in coords) + max(c.y for c in coords)) * 0.5
    z_min = min(c.z for c in coords)
    z_span = max(max(c.z for c in coords) - z_min, 1e-6)

    for loop in mesh.loops:
        co = mesh.vertices[loop.vertex_index].co
        angle = math.atan2(co.x - centre_x, -(co.y - centre_y)) / (2.0 * math.pi)
        uv_layer.data[loop.index].uv = (
            (angle - SHIRT_SEAM_TURNS) % 1.0,
            (co.z - z_min) / z_span,
        )

    # Faces straddling the seam would otherwise stretch the whole texture across
    # a thin strip (u jumping 0.99 -> 0.01). Lift the low side past 1.0 so the
    # face samples continuously; the texture wraps, so u > 1 is well defined.
    for poly in mesh.polygons:
        us = [uv_layer.data[i].uv[0] for i in poly.loop_indices]
        if max(us) - min(us) <= 0.5:
            continue
        for loop_index in poly.loop_indices:
            uv = uv_layer.data[loop_index].uv
            if uv[0] < 0.5:
                uv_layer.data[loop_index].uv = (uv[0] + 1.0, uv[1])


def assign_materials(bpy, body, garments) -> None:
    """Name one material per slot so the runtime can recolour by slot name.

    The colours here are neutral placeholders: `src/render/kit.rs` swaps in
    shared palette handles (skin tone) and a composited shirt texture (team
    colours, number, name, crest) at spawn.
    """
    slots = [(body, "Skin", (0.80, 0.62, 0.50, 1.0))]
    defaults = {
        "Shirt": (0.92, 0.92, 0.90, 1.0),
        "Pants": (0.92, 0.92, 0.90, 1.0),
        "Shoes": (0.14, 0.14, 0.16, 1.0),
        "Hair": (0.12, 0.09, 0.07, 1.0),
    }
    for garment in garments:
        slots.append((garment, garment.name, defaults[garment.name]))

    for obj, name, colour in slots:
        material = bpy.data.materials.new(name=name)
        if not material.use_nodes:
            material.use_nodes = True
        bsdf = material.node_tree.nodes.get("Principled BSDF")
        if bsdf is not None:
            bsdf.inputs["Base Color"].default_value = colour
            bsdf.inputs["Roughness"].default_value = 0.85
            bsdf.inputs["Metallic"].default_value = 0.0
        obj.data.name = "Body" if name == "Skin" else name
        obj.data.materials.clear()
        obj.data.materials.append(material)


def add_bind_action(bpy, armature) -> None:
    """Bake a rest-pose action so Bevy wires up the animation components.

    Bevy's glTF loader only inserts `AnimationPlayer` and `AnimationTargetId`
    for nodes that are animation roots *in the file being loaded*
    (`animation_roots` in `bevy_gltf`). An archetype with no animations of its
    own therefore gets neither, and the shared Xbot idle/run clips have nothing
    to bind to — every figure freezes in bind pose with its arms out.

    A two-frame rest-pose action on every bone is enough to make the loader
    emit those components. Because the bone-name paths match Xbot's exactly,
    the Xbot clips then retarget onto them and this action is never played.
    """
    bpy.context.view_layer.objects.active = armature
    armature.animation_data_create()
    armature.animation_data.action = bpy.data.actions.new("RestPose")
    bpy.ops.object.mode_set(mode="POSE")
    for pose_bone in armature.pose.bones:
        pose_bone.rotation_mode = "QUATERNION"
        for frame in (1, 2):
            pose_bone.keyframe_insert(data_path="rotation_quaternion", frame=frame)
            pose_bone.keyframe_insert(data_path="location", frame=frame)
    bpy.ops.object.mode_set(mode="OBJECT")


def strip_stray_objects(bpy, keep) -> None:
    """Drop everything MPFB leaves in the scene that is not ours.

    Rig creation parks helper primitives (an `Icosphere` bone shape among them)
    in the scene; unnamed and unmaterialled, they survive glTF export and show
    up floating in the match.
    """
    kept = {obj.name for obj in keep}
    for obj in list(bpy.data.objects):
        if obj.type == "MESH" and obj.name not in kept:
            print(f"   - dropped stray object: {obj.name}")
            bpy.data.objects.remove(obj, do_unlink=True)


def export_glb(bpy, path: Path) -> None:
    bpy.ops.export_scene.gltf(
        filepath=str(path),
        export_format="GLB",
        export_apply=True,
        export_skins=True,
        # Macro targets are already baked into the mesh; exporting them as glTF
        # morph targets would multiply the file size for no runtime benefit.
        export_morph=False,
        export_animations=True,
        export_yup=True,
    )


# ---------------------------------------------------------------------------
# Verification
# ---------------------------------------------------------------------------


def read_gltf_json(path: Path) -> dict:
    blob = path.read_bytes()
    offset = 12
    while offset < len(blob):
        length, chunk_type = struct.unpack_from("<II", blob, offset)
        if chunk_type == 0x4E4F534A:
            return json.loads(blob[offset + 8 : offset + 8 + length])
        offset += 8 + length
    raise ValueError(f"no JSON chunk in {path}")


def measure(path: Path, name: str) -> dict:
    """Report the bind-pose numbers the Rust side needs, straight from the GLB."""
    gltf = read_gltf_json(path)
    tris = sum(
        gltf["accessors"][prim["indices"]]["count"] // 3
        for mesh in gltf.get("meshes", [])
        for prim in mesh["primitives"]
        if "indices" in prim
    )
    world = bind_pose_world_translations(gltf)
    foot_y = min(
        world[b][1] for b in ("mixamorig:LeftFoot", "mixamorig:RightFoot") if b in world
    )
    hips_y = world.get("mixamorig:Hips", (0.0, 0.0, 0.0))[1]
    mesh_min_y = min(
        gltf["accessors"][prim["attributes"]["POSITION"]]["min"][1]
        for mesh in gltf.get("meshes", [])
        for prim in mesh["primitives"]
    )
    head_y = world.get("mixamorig:Head", (0.0, 0.0, 0.0))[1]
    return {
        "name": name,
        "mb": path.stat().st_size / 1e6,
        "tris": tris,
        "foot_y": foot_y,
        "hips_y": hips_y,
        "head_y": head_y,
        "mesh_min_y": mesh_min_y,
        "materials": [m.get("name") for m in gltf.get("materials", [])],
    }


def bind_pose_world_translations(gltf: dict) -> dict[str, tuple[float, float, float]]:
    def quat_mul(a, b):
        ax, ay, az, aw = a
        bx, by, bz, bw = b
        return (
            aw * bx + ax * bw + ay * bz - az * by,
            aw * by - ax * bz + ay * bw + az * bx,
            aw * bz + ax * by - ay * bx + az * bw,
            aw * bw - ax * bx - ay * by - az * bz,
        )

    def quat_rot(q, v):
        x, y, z, w = q
        tx, ty, tz = 2 * (y * v[2] - z * v[1]), 2 * (z * v[0] - x * v[2]), 2 * (
            x * v[1] - y * v[0]
        )
        return (
            v[0] + w * tx + y * tz - z * ty,
            v[1] + w * ty + z * tx - x * tz,
            v[2] + w * tz + x * ty - y * tx,
        )

    out: dict[str, tuple[float, float, float]] = {}

    def walk(index: int, parent_q, parent_t, parent_s):
        node = gltf["nodes"][index]
        t = node.get("translation", [0.0, 0.0, 0.0])
        r = tuple(node.get("rotation", [0.0, 0.0, 0.0, 1.0]))
        s = node.get("scale", [1.0, 1.0, 1.0])
        scaled = [t[i] * parent_s[i] for i in range(3)]
        rotated = quat_rot(parent_q, scaled)
        world_t = tuple(parent_t[i] + rotated[i] for i in range(3))
        world_q = quat_mul(parent_q, r)
        world_s = [parent_s[i] * s[i] for i in range(3)]
        if node.get("name"):
            out[node["name"]] = world_t
        for child in node.get("children", []):
            walk(child, world_q, world_t, world_s)

    for root in gltf["scenes"][0]["nodes"]:
        walk(root, (0.0, 0.0, 0.0, 1.0), (0.0, 0.0, 0.0), [1.0, 1.0, 1.0])
    return out


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------


def main() -> None:
    if "--blender-child" in sys.argv:
        payload = sys.argv[sys.argv.index("--blender-child") + 1]
        blender_main(json.loads(payload))
        return

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--install", action="store_true", help="download and install MPFB, then exit"
    )
    parser.add_argument(
        "--list", action="store_true", help="list the archetype matrix and exit"
    )
    parser.add_argument(
        "--only", action="append", default=[], help="build only the named archetype(s)"
    )
    parser.add_argument(
        "--long-sleeve",
        action="store_true",
        help="build long-sleeved shirts (sweaters) instead of short",
    )
    args = parser.parse_args()

    if args.install:
        install_mpfb()
        return

    matrix = default_matrix()
    if args.long_sleeve:
        matrix = [
            Archetype(a.height, a.build, a.ancestry, long_sleeve=True) for a in matrix
        ]
    if args.only:
        wanted = set(args.only)
        matrix = [a for a in matrix if a.name in wanted]
        if not matrix:
            parser.error(f"no archetype matched {sorted(wanted)}")

    if args.list:
        for archetype in matrix:
            print(archetype.name)
        print(f"\n{len(matrix)} archetypes")
        return

    if not BLENDER_RESOURCES.exists():
        parser.error("MPFB is not installed — run with --install first")
    run_in_blender(matrix)


if __name__ == "__main__":
    main()

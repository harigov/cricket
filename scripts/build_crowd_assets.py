#!/usr/bin/env python3
"""Build Quaternius posed-human crowd GLBs for the cricket stadium."""

from __future__ import annotations

import json
import math
import os
import shutil
import struct
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path

sys.dont_write_bytecode = True

REPO_ROOT = Path(__file__).resolve().parents[1]
CACHE_DIR = REPO_ROOT / "target" / "crowd-src"
OUT_DIR = REPO_ROOT / "assets" / "crowd" / "posed"
WORK_DIR = REPO_ROOT / "target" / "crowd-work"

DRIVE_IDS: dict[str, str] = {
    "Male_Standing.obj": "1sYxmvLuMH9CkwwLGyTAhZsN_-tFIhbHu",
    "Male_Standing.mtl": "1smW3UJYnJCVavWgy2Py4SLCH_fzYveqK",
    "Male_Sitting.obj": "183uU-nqxzQJP1N1ykKOXVbjdHwdEaBs_",
    "Male_Sitting.mtl": "1L2EWcd2WqpqaIvEpGSlimfA8QDivF3eX",
    "Male_Sitting_Cheering.obj": "1D_lhGqSOj9nxLJ4T3ksDvx20uBzXpoSl",
    "Male_Sitting_Cheering.mtl": "1JAxo_WSoC_8nukY0RzojYhRQw3LhJoEs",
    "Male_Standing_Waving.obj": "1hUF5EuBSqa5iVigsfhK3nkalybBuZsF7",
    "Male_Standing_Waving.mtl": "1pww0eBJDHguEBCPOYIpSfAY9SzGRw64V",
    "Female_Standing.obj": "13oLeT8urbcq_6W2C1cQq49tqTLKuG69b",
    "Female_Standing.mtl": "1NFMtKoemVwLZuq4BpdMIWXef8FTUC4Rq",
    "Female_Sitting.obj": "1tsH40mlqGXeI7igey4i_gMDKx2wvTGRn",
    "Female_Sitting.mtl": "1DQSrWJW_jz3YcHuT50RZbZwUtP8WUiPj",
    "Female_Sitting_Cheering.obj": "1QV2hODdvt1Ta0kCwfd0Vw7Z6vt6Myhuk",
    "Female_Sitting_Cheering.mtl": "1X8mNUyynTJL1aJoATBJVss0hiYxdV0k8",
    "Woman_Standing_Waving.obj": "1q3UmkWXYQTI33SybQvMfnNAyrp9rPRE3",
    "Woman_Standing_Waving.mtl": "1z9EwRszV-Q5QRHbwzzkMdJtawkivuQ_s",
    "Male_Hairstyle_1.obj": "1ITsZj3OhCHyIx9va025Drd6USpYn7ZUp",
    "Male_Hairstyle_3.obj": "1-kf_HYqtQooOZ3p7Sj1p9TNPOqbI3tXH",
    "Female_Hairstyle_1.obj": "1lLF_OUcoF3aBsEIXo9UYK8NMFANB5JXh",
    "Female_Hairstyle_2.obj": "1DTsBUtMrOccNjRwvKrRduip3RlQgGPyC",
    "License.txt": "1K6M5kb7ugqrPROrSx6O6dUBJA366xq-j",
}

GENDERS = {
    "male": {
        "rest_obj": "Male_Standing.obj",
        "rest_mtl": "Male_Standing.mtl",
        "target_height": 1.78,
        "poses": {
            "sit": ("Male_Sitting.obj", "Male_Sitting.mtl"),
            "cheer": ("Male_Sitting_Cheering.obj", "Male_Sitting_Cheering.mtl"),
            "wave": ("Male_Standing_Waving.obj", "Male_Standing_Waving.mtl"),
        },
        "hairs": {
            "hair1": "Male_Hairstyle_1.obj",
            "hair3": "Male_Hairstyle_3.obj",
            "bald": None,
        },
    },
    "female": {
        "rest_obj": "Female_Standing.obj",
        "rest_mtl": "Female_Standing.mtl",
        "target_height": 1.66,
        "poses": {
            "sit": ("Female_Sitting.obj", "Female_Sitting.mtl"),
            "cheer": ("Female_Sitting_Cheering.obj", "Female_Sitting_Cheering.mtl"),
            "wave": ("Woman_Standing_Waving.obj", "Woman_Standing_Waving.mtl"),
        },
        "hairs": {
            "hair1": "Female_Hairstyle_1.obj",
            "hair2": "Female_Hairstyle_2.obj",
        },
    },
}

Vec3 = tuple[float, float, float]


@dataclass
class ObjMesh:
    vertices: list[Vec3] = field(default_factory=list)
    texcoords: list[tuple[float, float]] = field(default_factory=list)
    normals: list[Vec3] = field(default_factory=list)
    faces: list[tuple[str, list[tuple[int, int | None, int | None]]]] = field(
        default_factory=list
    )


def download_sources() -> None:
    CACHE_DIR.mkdir(parents=True, exist_ok=True)
    for name, file_id in DRIVE_IDS.items():
        dest = CACHE_DIR / name
        if dest.exists() and dest.stat().st_size > 0:
            continue
        url = f"https://drive.google.com/uc?export=download&id={file_id}"
        print(f"Downloading {name}...")
        subprocess.run(["curl", "-sL", url, "-o", str(dest)], check=True)
        if dest.stat().st_size == 0:
            raise RuntimeError(f"Download failed for {name}")


def parse_obj(path: Path) -> ObjMesh:
    mesh = ObjMesh()
    current_material = "default"
    with path.open(encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            parts = line.split()
            tag = parts[0]
            if tag == "v":
                mesh.vertices.append(
                    (float(parts[1]), float(parts[2]), float(parts[3]))
                )
            elif tag == "vt":
                mesh.texcoords.append((float(parts[1]), float(parts[2])))
            elif tag == "vn":
                mesh.normals.append(
                    (float(parts[1]), float(parts[2]), float(parts[3]))
                )
            elif tag == "usemtl":
                current_material = parts[1]
            elif tag == "f":
                verts: list[tuple[int, int | None, int | None]] = []
                for corner in parts[1:]:
                    chunks = corner.split("/")
                    vi = int(chunks[0])
                    ti = int(chunks[1]) if len(chunks) > 1 and chunks[1] else None
                    ni = int(chunks[2]) if len(chunks) > 2 and chunks[2] else None
                    verts.append((vi, ti, ni))
                mesh.faces.append((current_material, verts))
    return mesh


def vec_add(a: Vec3, b: Vec3) -> Vec3:
    return (a[0] + b[0], a[1] + b[1], a[2] + b[2])


def vec_sub(a: Vec3, b: Vec3) -> Vec3:
    return (a[0] - b[0], a[1] - b[1], a[2] - b[2])


def vec_scale(a: Vec3, s: float) -> Vec3:
    return (a[0] * s, a[1] * s, a[2] * s)


def mat_vec(m: list[list[float]], v: Vec3) -> Vec3:
    return (
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    )


def mat_mul(a: list[list[float]], b: list[list[float]]) -> list[list[float]]:
    out = [[0.0] * 3 for _ in range(3)]
    for i in range(3):
        for j in range(3):
            out[i][j] = sum(a[i][k] * b[k][j] for k in range(3))
    return out


def mat_transpose(m: list[list[float]]) -> list[list[float]]:
    return [[m[j][i] for j in range(3)] for i in range(3)]


def mat_det(m: list[list[float]]) -> float:
    return (
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    )


def identity3() -> list[list[float]]:
    return [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]


def centroid(points: list[Vec3]) -> Vec3:
    n = len(points)
    sx = sum(p[0] for p in points)
    sy = sum(p[1] for p in points)
    sz = sum(p[2] for p in points)
    return (sx / n, sy / n, sz / n)


def bbox_center(points: list[Vec3]) -> Vec3:
    xs = [p[0] for p in points]
    ys = [p[1] for p in points]
    zs = [p[2] for p in points]
    return (
        (min(xs) + max(xs)) / 2.0,
        (min(ys) + max(ys)) / 2.0,
        (min(zs) + max(zs)) / 2.0,
    )


def bbox_extents(points: list[Vec3]) -> tuple[Vec3, Vec3]:
    xs = [p[0] for p in points]
    ys = [p[1] for p in points]
    zs = [p[2] for p in points]
    return (
        (min(xs), min(ys), min(zs)),
        (max(xs), max(ys), max(zs)),
    )


def rms_error(a: list[Vec3], b: list[Vec3]) -> float:
    n = len(a)
    acc = 0.0
    for pa, pb in zip(a, b):
        dx = pa[0] - pb[0]
        dy = pa[1] - pb[1]
        dz = pa[2] - pb[2]
        acc += dx * dx + dy * dy + dz * dz
    return math.sqrt(acc / n)


def head_indices(rest_verts: list[Vec3], min_count: int = 30) -> list[int]:
    max_y = max(v[1] for v in rest_verts)
    band = 0.45
    while band <= 2.0:
        threshold = max_y - band
        indices = [i for i, v in enumerate(rest_verts) if v[1] > threshold]
        if len(indices) >= min_count:
            return indices
        band += 0.05
    return indices


def topology_matches(a: ObjMesh, b: ObjMesh) -> bool:
    if len(a.vertices) != len(b.vertices):
        return False
    if len(a.faces) != len(b.faces):
        return False
    for (_, fa), (_, fb) in zip(a.faces, b.faces):
        if len(fa) != len(fb):
            return False
        for ca, cb in zip(fa, fb):
            if ca[0] != cb[0]:
                return False
    return True


def _jacobi_eigen_symmetric(a: list[list[float]]) -> tuple[list[float], list[list[float]]]:
    """Jacobi eigen-decomposition for 3x3 symmetric matrix."""
    m = [row[:] for row in a]
    v = identity3()
    for _ in range(50):
        p, q = 0, 1
        max_val = abs(m[p][q])
        for i in range(3):
            for j in range(i + 1, 3):
                if abs(m[i][j]) > max_val:
                    max_val = abs(m[i][j])
                    p, q = i, j
        if max_val < 1e-12:
            break
        phi = 0.5 * math.atan2(2.0 * m[p][q], m[q][q] - m[p][p])
        c, s = math.cos(phi), math.sin(phi)
        for i in range(3):
            if i not in (p, q):
                mii, miq = m[i][p], m[i][q]
                m[i][p] = c * mii - s * miq
                m[p][i] = m[i][p]
                m[i][q] = s * mii + c * miq
                m[q][i] = m[i][q]
        app = c * c * m[p][p] - 2.0 * s * c * m[p][q] + s * s * m[q][q]
        aqq = s * s * m[p][p] + 2.0 * s * c * m[p][q] + c * c * m[q][q]
        m[p][p], m[q][q] = app, aqq
        m[p][q] = m[q][p] = 0.0
        for i in range(3):
            vip, viq = v[i][p], v[i][q]
            v[i][p] = c * vip - s * viq
            v[i][q] = s * vip + c * viq
    eigvals = [m[i][i] for i in range(3)]
    return eigvals, v


def kabsch_rotation(rest_pts: list[Vec3], pose_pts: list[Vec3]) -> list[list[float]]:
    c_rest = centroid(rest_pts)
    c_pose = centroid(pose_pts)
    rest_c = [vec_sub(p, c_rest) for p in rest_pts]
    pose_c = [vec_sub(p, c_pose) for p in pose_pts]

    try:
        import numpy as np

        p = np.array(rest_c, dtype=float)
        q = np.array(pose_c, dtype=float)
        c = p.T @ q
        u, _, vt = np.linalg.svd(c)
        r = vt.T @ u.T
        if np.linalg.det(r) < 0:
            vt[-1, :] *= -1
            r = vt.T @ u.T
        return r.tolist()
    except ImportError:
        pass

    c_cov = [[0.0] * 3 for _ in range(3)]
    for pr, pp in zip(rest_c, pose_c):
        for i in range(3):
            for j in range(3):
                c_cov[i][j] += pr[i] * pp[j]

    ct_c = mat_mul(mat_transpose(c_cov), c_cov)
    eigvals, v = _jacobi_eigen_symmetric(ct_c)
    order = sorted(range(3), key=lambda i: eigvals[i], reverse=True)
    v_cols = [[v[row][col] for row in range(3)] for col in order]
    u_mat = mat_mul(c_cov, v_cols)
    for col in range(3):
        norm = math.sqrt(sum(u_mat[row][col] ** 2 for row in range(3)))
        if norm < 1e-12:
            norm = 1.0
        for row in range(3):
            u_mat[row][col] /= norm
    vt = mat_transpose(v_cols)
    r = mat_mul(vt, mat_transpose(u_mat))
    if mat_det(r) < 0:
        for row in range(3):
            u_mat[row][2] *= -1.0
        r = mat_mul(vt, mat_transpose(u_mat))
    return r


def _vec_dist(a: Vec3, b: Vec3) -> float:
    dx = a[0] - b[0]
    dy = a[1] - b[1]
    dz = a[2] - b[2]
    return math.sqrt(dx * dx + dy * dy + dz * dz)


def icp_head_transform(
    rest_head: list[Vec3],
    pose_verts: list[Vec3],
    *,
    pose_label: str,
) -> tuple[list[list[float]], Vec3, Vec3]:
    c_rest = centroid(rest_head)
    lo, hi = bbox_extents(rest_head)
    expand = 0.35
    expanded_lo = (lo[0] - expand, lo[1] - expand, lo[2] - expand)
    expanded_hi = (hi[0] + expand, hi[1] + expand, hi[2] + expand)
    candidate_pts = [
        v
        for v in pose_verts
        if expanded_lo[0] <= v[0] <= expanded_hi[0]
        and expanded_lo[1] <= v[1] <= expanded_hi[1]
        and expanded_lo[2] <= v[2] <= expanded_hi[2]
    ]
    if len(candidate_pts) < 30:
        raise RuntimeError(
            f"{pose_label}: too few ICP candidate head vertices ({len(candidate_pts)})"
        )

    c_pose = centroid(candidate_pts)
    r = identity3()
    pair_threshold = 0.25

    for iteration in range(10):
        transformed = [
            vec_add(mat_vec(r, vec_sub(p, c_rest)), c_pose) for p in rest_head
        ]
        matched_rest: list[Vec3] = []
        matched_pose: list[Vec3] = []
        for tp, rp in zip(transformed, rest_head):
            best_dist = float("inf")
            best_pt: Vec3 | None = None
            for cp in candidate_pts:
                dist = _vec_dist(tp, cp)
                if dist < best_dist:
                    best_dist = dist
                    best_pt = cp
            if best_pt is not None and best_dist <= pair_threshold:
                matched_rest.append(rp)
                matched_pose.append(best_pt)
        if len(matched_rest) < 10:
            raise RuntimeError(
                f"{pose_label}: ICP iteration {iteration}: only "
                f"{len(matched_rest)} pairs within {pair_threshold}"
            )
        r = kabsch_rotation(matched_rest, matched_pose)
        c_pose = centroid(matched_pose)

    transformed = [
        vec_add(mat_vec(r, vec_sub(p, c_rest)), c_pose) for p in rest_head
    ]
    final_matches: list[Vec3] = []
    for tp in transformed:
        best_pt = min(candidate_pts, key=lambda cp: _vec_dist(tp, cp))
        final_matches.append(best_pt)
    err = rms_error(transformed, final_matches)
    if err >= 0.08:
        raise RuntimeError(f"{pose_label}: ICP RMS {err:.4f} >= 0.08 source units")
    return r, c_rest, c_pose


def compute_head_transform(
    rest_verts: list[Vec3],
    pose_verts: list[Vec3],
    *,
    pose_label: str,
) -> tuple[list[list[float]], Vec3, Vec3, list[Vec3]]:
    """Return (R, c_rest, c_pose, rest_head)."""
    head_idx = head_indices(rest_verts)
    rest_head = [rest_verts[i] for i in head_idx]
    c_rest = centroid(rest_head)

    if len(rest_verts) != len(pose_verts):
        print(
            f"WARNING: {pose_label}: pose topology differs from rest — "
            "using ICP head alignment"
        )
        return (*icp_head_transform(rest_head, pose_verts, pose_label=pose_label), rest_head)

    pose_head = [pose_verts[i] for i in head_idx]
    c_pose = centroid(pose_head)
    r = kabsch_rotation(rest_head, pose_head)
    transformed = [
        vec_add(mat_vec(r, vec_sub(rest_verts[i], c_rest)), c_pose) for i in head_idx
    ]
    err = rms_error(transformed, pose_head)
    if err >= 0.05:
        print(
            f"WARNING: {pose_label}: head Kabsch RMS error {err:.4f} >= 0.05"
        )
    return r, c_rest, c_pose, rest_head


def transform_hair(
    hair: ObjMesh,
    r: list[list[float]],
    c_rest: Vec3,
    c_pose: Vec3,
    rest_head_pts: list[Vec3],
) -> ObjMesh:
    a_rest = bbox_center(rest_head_pts)
    d = vec_sub(a_rest, c_rest)
    new_verts = [vec_add(mat_vec(r, vec_add(v, d)), c_pose) for v in hair.vertices]
    new_normals = [mat_vec(r, n) for n in hair.normals] if hair.normals else []

    out = ObjMesh(vertices=new_verts, normals=new_normals)
    if hair.faces:
        out.faces = [("Hair", face[1]) for face in hair.faces]
    else:
        tris: list[tuple[int, int | None, int | None]] = []
        for i in range(0, len(hair.vertices), 3):
            for j in range(3):
                idx = i + j + 1
                tris.append((idx, None, idx if hair.normals else None))
        if tris:
            out.faces = [("Hair", tris)]
    return out


def copy_mtl_with_hair(src_mtl: Path, dst_mtl: Path, include_hair: bool) -> None:
    lines = src_mtl.read_text(encoding="utf-8").splitlines()
    out_lines = []
    for line in lines:
        out_lines.append(line)
    if include_hair and not any(l.startswith("newmtl Hair") for l in out_lines):
        out_lines.extend(
            [
                "",
                "newmtl Hair",
                "Ka 0.000 0.000 0.000",
                "Kd 0.18 0.12 0.08",
                "Ks 0.000 0.000 0.000",
                "d 1.0",
                "illum 2",
            ]
        )
    dst_mtl.write_text("\n".join(out_lines) + "\n", encoding="utf-8")


def write_merged_obj(
    path: Path,
    body: ObjMesh,
    hair: ObjMesh | None,
    mtl_name: str,
) -> None:
    lines: list[str] = [f"mtllib {mtl_name}", "o body"]
    for v in body.vertices:
        lines.append(f"v {v[0]:.8f} {v[1]:.8f} {v[2]:.8f}")
    for vt in body.texcoords:
        lines.append(f"vt {vt[0]:.8f} {vt[1]:.8f}")
    for vn in body.normals:
        lines.append(f"vn {vn[0]:.8f} {vn[1]:.8f} {vn[2]:.8f}")

    v_offset = len(body.vertices)
    vt_offset = len(body.texcoords)
    vn_offset = len(body.normals)

    if hair is not None:
        lines.append("o hair")
        for v in hair.vertices:
            lines.append(f"v {v[0]:.8f} {v[1]:.8f} {v[2]:.8f}")
        for vn in hair.normals:
            lines.append(f"vn {vn[0]:.8f} {vn[1]:.8f} {vn[2]:.8f}")

    current_mtl: str | None = None
    for material, corners in body.faces:
        if material != current_mtl:
            lines.append(f"usemtl {material}")
            current_mtl = material
        parts = []
        for vi, ti, ni in corners:
            v_idx = vi
            if ti is not None:
                t_idx = ti
                if ni is not None:
                    parts.append(f"{v_idx}/{t_idx}/{ni}")
                else:
                    parts.append(f"{v_idx}/{t_idx}")
            elif ni is not None:
                parts.append(f"{v_idx}//{ni}")
            else:
                parts.append(str(v_idx))
        lines.append("f " + " ".join(parts))

    if hair is not None and hair.faces:
        lines.append("usemtl Hair")
        for _, corners in hair.faces:
            parts = []
            for vi, _ti, ni in corners:
                v_idx = vi + v_offset
                if ni is not None:
                    n_idx = ni + vn_offset
                    parts.append(f"{v_idx}//{n_idx}")
                else:
                    parts.append(str(v_idx))
            lines.append("f " + " ".join(parts))

    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def apply_normalization(
    mesh: ObjMesh, offset: Vec3, scale: float
) -> ObjMesh:
    def xform(v: Vec3) -> Vec3:
        return vec_scale(vec_sub(v, offset), scale)

    return ObjMesh(
        vertices=[xform(v) for v in mesh.vertices],
        texcoords=mesh.texcoords[:],
        normals=mesh.normals[:],
        faces=mesh.faces[:],
    )


def compute_normalization(rest_verts: list[Vec3], target_height: float) -> tuple[Vec3, float]:
    lo, hi = bbox_extents(rest_verts)
    height = hi[1] - lo[1]
    scale = target_height / height
    offset = ((lo[0] + hi[0]) / 2.0, lo[1], (lo[2] + hi[2]) / 2.0)
    return offset, scale


def resolve_npx() -> str:
    path = os.environ.get("PATH", "")
    nvm_bin = Path.home() / ".nvm/versions/node"
    extra: list[str] = []
    if nvm_bin.is_dir():
        for node_dir in sorted(nvm_bin.glob("v*/bin"), reverse=True):
            extra.append(str(node_dir))
    search_path = os.pathsep.join(extra + path.split(os.pathsep))
    found = shutil.which("npx", path=search_path)
    if not found:
        raise RuntimeError("npx not found; install Node.js to run obj2gltf")
    return found


def obj_to_glb(obj_path: Path, glb_path: Path) -> None:
    glb_path.parent.mkdir(parents=True, exist_ok=True)
    npx = resolve_npx()
    subprocess.run(
        [npx, "-y", "obj2gltf@3", "-i", str(obj_path), "-o", str(glb_path), "-b"],
        check=True,
        cwd=REPO_ROOT,
    )


def mesh_bbox(mesh: ObjMesh) -> tuple[Vec3, Vec3]:
    return bbox_extents(mesh.vertices)


def build_variant(
    gender: str,
    pose_key: str,
    hair_key: str,
    rest_mesh: ObjMesh,
    offset: Vec3,
    scale: float,
) -> Path:
    cfg = GENDERS[gender]
    pose_obj_name, pose_mtl_name = cfg["poses"][pose_key]
    hair_obj_name = cfg["hairs"][hair_key]
    include_hair = hair_obj_name is not None

    pose_mesh = parse_obj(CACHE_DIR / pose_obj_name)
    rest_mesh_local = parse_obj(CACHE_DIR / cfg["rest_obj"])

    pose_label = f"{gender}_{pose_key}"
    r, c_rest, c_pose, rest_head = compute_head_transform(
        rest_mesh_local.vertices,
        pose_mesh.vertices,
        pose_label=pose_label,
    )

    out_name = f"{gender}_{pose_key}_{hair_key}.glb"

    hair_mesh: ObjMesh | None = None
    if include_hair:
        hair_src = parse_obj(CACHE_DIR / hair_obj_name)
        hair_mesh = transform_hair(hair_src, r, c_rest, c_pose, rest_head)

    work_sub = WORK_DIR / f"{gender}_{pose_key}_{hair_key}"
    if work_sub.exists():
        shutil.rmtree(work_sub)
    work_sub.mkdir(parents=True)

    merged_mtl = f"{gender}_{pose_key}_{hair_key}.mtl"
    copy_mtl_with_hair(
        CACHE_DIR / pose_mtl_name, work_sub / merged_mtl, include_hair
    )

    norm_body = apply_normalization(pose_mesh, offset, scale)
    norm_hair = (
        apply_normalization(hair_mesh, offset, scale) if hair_mesh is not None else None
    )

    merged_obj = work_sub / f"{gender}_{pose_key}_{hair_key}.obj"
    write_merged_obj(merged_obj, norm_body, norm_hair, merged_mtl)

    glb_path = OUT_DIR / out_name
    obj_to_glb(merged_obj, glb_path)

    all_verts = norm_body.vertices[:]
    if norm_hair is not None:
        all_verts.extend(norm_hair.vertices)
    lo, hi = bbox_extents(all_verts)
    print(
        f"{out_name}: bbox min=({lo[0]:.4f}, {lo[1]:.4f}, {lo[2]:.4f}) "
        f"max=({hi[0]:.4f}, {hi[1]:.4f}, {hi[2]:.4f})"
    )
    if pose_key == "wave" and hair_key == "hair1":
        print(
            f"  standing height check ({gender}): body max y={hi[1]:.4f} m "
            f"(target {cfg['target_height']:.2f} m)"
        )

    validate_glb(glb_path)
    return glb_path


def load_glb_primitives(path: Path) -> dict[str, list[Vec3]]:
    data = path.read_bytes()
    if data[:4] != b"glTF":
        raise ValueError(f"{path.name}: invalid GLB magic")
    chunk_len = struct.unpack_from("<I", data, 12)[0]
    gltf = json.loads(data[20 : 20 + chunk_len].decode("utf-8"))
    bin_off = 20 + chunk_len + 8

    def accessor_vecs(accessor_index: int) -> list[Vec3]:
        accessor = gltf["accessors"][accessor_index]
        buffer_view = gltf["bufferViews"][accessor["bufferView"]]
        offset = (
            bin_off
            + buffer_view.get("byteOffset", 0)
            + accessor.get("byteOffset", 0)
        )
        return [
            struct.unpack_from("<3f", data, offset + 12 * k)
            for k in range(accessor["count"])
        ]

    prims: dict[str, list[Vec3]] = {}
    for mesh in gltf.get("meshes", []):
        for primitive in mesh["primitives"]:
            material_name = gltf["materials"][primitive["material"]]["name"]
            prims[material_name] = accessor_vecs(primitive["attributes"]["POSITION"])
    return prims


def hair_nn_stats(hair: list[Vec3], body: list[Vec3]) -> tuple[float, float, float]:
    distances: list[float] = []
    for h in hair:
        distances.append(min(_vec_dist(h, b) for b in body))
    distances.sort()
    median = distances[len(distances) // 2]
    mean = sum(distances) / len(distances)
    return median, mean, distances[-1]


def validate_glb(path: Path) -> None:
    bald = path.name.endswith("_bald.glb")
    prims = load_glb_primitives(path)
    materials = set(prims.keys())
    expected = {"Skin", "Shirt", "Pants", "Shoes"} | (set() if bald else {"Hair"})
    if materials != expected:
        raise RuntimeError(
            f"{path.name}: materials {sorted(materials)} != expected {sorted(expected)}"
        )

    body_verts = [v for name, verts in prims.items() if name != "Hair" for v in verts]
    body_lo, body_hi = bbox_extents(body_verts)
    if body_lo[1] < -0.02 or body_lo[1] > 0.02:
        raise RuntimeError(
            f"{path.name}: body min y={body_lo[1]:.4f} not within ±0.02 m of ground"
        )
    if body_hi[1] >= 2.2:
        raise RuntimeError(f"{path.name}: body max y={body_hi[1]:.4f} >= 2.2 m")

    if bald:
        print(
            f"PASS {path.name}: bald body_min_y={body_lo[1]:.3f} "
            f"body_max_y={body_hi[1]:.3f}"
        )
        return

    hair = prims["Hair"]
    median, mean, _mx = hair_nn_stats(hair, body_verts)
    hair_cy = bbox_center(hair)[1]
    if median >= 0.06 or mean >= 0.09:
        raise RuntimeError(
            f"{path.name}: hair nn median={median:.4f} mean={mean:.4f} "
            f"(limits median<0.06 mean<0.09)"
        )
    if hair_cy <= body_hi[1] - 0.50 or hair_cy <= 0.55 * body_hi[1]:
        raise RuntimeError(
            f"{path.name}: hair centre y={hair_cy:.4f} too low vs body max y={body_hi[1]:.4f}"
        )
    print(
        f"PASS {path.name}: hair_median={median:.3f} hair_mean={mean:.3f} "
        f"hair_cy={hair_cy:.3f} body_min_y={body_lo[1]:.3f} body_max_y={body_hi[1]:.3f}"
    )


def verify_glb(path: Path) -> list[str]:
    data = path.read_bytes()
    if data[:4] != b"glTF":
        raise ValueError(f"{path.name}: invalid GLB magic")
    version, length = struct.unpack_from("<II", data, 4)
    if version != 2:
        raise ValueError(f"{path.name}: unsupported glTF version {version}")
    if length != len(data):
        raise ValueError(f"{path.name}: length mismatch")
    chunk_len, chunk_type = struct.unpack_from("<II", data, 12)
    if chunk_type != 0x4E4F534A:
        raise ValueError(f"{path.name}: first chunk is not JSON")
    json_bytes = data[20 : 20 + chunk_len]
    gltf = json.loads(json_bytes.decode("utf-8"))
    return sorted(m.get("name", "") for m in gltf.get("materials", []))


def write_attribution(glb_files: list[str]) -> None:
    content = """# Crowd Asset Attribution

## Quaternius — Background Posed Humans

- **Pack:** Background Posed Humans
- **Author:** [Quaternius](https://quaternius.com/)
- **License:** [CC0 1.0 Universal](https://creativecommons.org/publicdomain/zero/1.0/)
- **Source:** https://quaternius.com/packs/backgroundposedhumans.html
- **Credit line:** Background characters by Quaternius

These posed-human GLBs replace the earlier Kenney blocky crowd models for stadium
spectators. The pack is intended for **distant spectators**, not close-up broadcast shots.

### Generated files (`assets/crowd/posed/`)

"""
    for name in sorted(glb_files):
        content += f"- `{name}`\n"
    content += """
### Regeneration

```bash
python3 scripts/build_crowd_assets.py
```

Downloads source OBJ/MTL from the Quaternius pack (cached under `target/crowd-src/`),
fits hairstyles via rigid head alignment, normalises standing height (male 1.78 m,
female 1.66 m), and converts merged meshes to GLB with `npx -y obj2gltf@3`.
"""
    (REPO_ROOT / "assets" / "crowd" / "ATTRIBUTION.md").write_text(
        content, encoding="utf-8"
    )


def main() -> int:
    download_sources()
    OUT_DIR.mkdir(parents=True, exist_ok=True)

    generated: list[Path] = []
    for gender, cfg in GENDERS.items():
        rest_mesh = parse_obj(CACHE_DIR / cfg["rest_obj"])
        offset, scale = compute_normalization(
            rest_mesh.vertices, cfg["target_height"]
        )
        for pose_key in cfg["poses"]:
            for hair_key in cfg["hairs"]:
                generated.append(
                    build_variant(
                        gender, pose_key, hair_key, rest_mesh, offset, scale
                    )
                )

    glb_names = sorted(p.name for p in OUT_DIR.glob("*.glb"))
    write_attribution(glb_names)

    print(f"\nDone: {len(glb_names)} GLBs in {OUT_DIR}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

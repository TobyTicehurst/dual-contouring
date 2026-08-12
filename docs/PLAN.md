# Implementation Plan

Status: **milestones 1–12 implemented.** Milestone 13 (scene labels) is
planned but not started — see below. Each milestone was a working,
compileable state before moving to the next. See `DESIGN.md` for the data
model and meshing rationale behind these steps.

## Actual project layout

Single Cargo binary crate (not a workspace split), with a hard module
boundary enforcing the Bevy/non-Bevy separation:

```
voxels2/
  Cargo.toml
  src/
    main.rs                  # Bevy App setup, plugin registration, demo scene assembly
    core/                     # NO bevy imports anywhere in here
      mod.rs
      voxel.rs                # Voxel, MaterialId, MaterialProperties, MaterialTable
      grid.rs                 # VoxelGrid storage (dense, no chunking/LOD)
      mesh_data.rs             # MeshData (positions/normals/indices)
      demo_scenes.rs            # hardcoded test/demo VoxelGrid generators
      meshing/
        mod.rs
        common.rs              # shared DC cell-scanning / edge-crossing / quad logic
        surface_nets.rs         # mesher 1: averaged vertex position
        axis_aligned_dc.rs       # mesher 2: axis-aligned per-axis mean
        hardness_blend_dc.rs      # mesher 3: lerp(mesher1, mesher2, hardness)
        reference_surface_nets.rs # adapter over the third-party `fast_surface_nets`
                                   # crate, for visually cross-checking mesher 1
                                   # (not part of the "real" mesher lineup)
    render/                    # bevy-specific code lives here
      mod.rs
      fly_camera.rs             # free-fly camera controller (hand-rolled)
      mesh_convert.rs            # core::MeshData -> bevy Mesh
      voxel_data.rs               # ECS wrappers: VoxelGridComponent, MaterialTableResource
      debug_spheres.rs           # sphere-per-voxel debug renderer (solid mesh entities)
      debug_cubes.rs             # cube-per-voxel debug renderer (solid/transparent/wireframe)
      voxel_mesh_view.rs          # MesherChoice + spawns entities from a mesher's output
    config.rs                  # debug/tuning flags (consts + DebugConfig resource)
  docs/
    DESIGN.md
    PLAN.md
```

`core/` is where "utils" from the prompt ended up — renamed since it's not
generic utilities, it's the voxel/meshing domain logic. It compiles with
zero Bevy imports; `glam` is a direct dependency of the whole crate (pinned
to the exact version Bevy itself uses) precisely so `core` can use `Vec3`
etc. without depending on Bevy.

## Milestones

### 1. Blank Rust project — done
`cargo init`, `bevy = "0.19"` added, `cargo run` opens a window.

### 2. Fly-around scene — done
Hand-rolled `FlyCamera` (WASD + mouse look + Space/Ctrl + Shift-sprint,
cursor auto-grabbed, Escape quits the app). Bevy 0.19 renamed several APIs
from what older docs/tutorials assume (`EventReader`→`MessageReader`,
`Window.cursor_options` is its own `CursorOptions` component,
`DirectionalLight.shadows_enabled`→`shadow_maps_enabled`) — resolved by
reading the installed crate source directly rather than guessing.

### 3. `core` module skeleton — done
`Voxel`, `MaterialId` (with `AIR`/`DIRT`/`BRICK`/`WOOD` constants naming
`MaterialTable::standard()`'s fixed layout), `MaterialProperties` (`name`,
`opaque`, `hardness`, `color`), `MaterialTable`, `MeshData`. `Voxel::new`
enforces the `(0, 1]` weight invariant via `debug_assert!`.

### 4. Voxel storage — done
`VoxelGrid`: dense `Vec<Voxel>`, local `0..size` coordinate space,
`get(IVec3)` returns `Voxel::AIR` for any out-of-bounds coordinate
(including negative), `set` panics out of bounds. `demo_scenes.rs` holds the
hardcoded test/demo fixtures (see below).

### 5. Debug sphere renderer — done
Originally `Gizmos`-based; rebuilt in the hardness-blend milestone pass to
spawn real solid mesh entities instead (gizmos are line-only, so "solid"
wasn't achievable with them) — one unit sphere mesh reused via `Transform`
scale per voxel, radius = `weight * debug_sphere_radius_scale`, coloured as
the *inverse* of the voxel's material colour so it doesn't blend in. Off by
default (`SHOW_DEBUG_SPHERES = false`) now that the meshed surface exists.

### 6. Debug cube renderer — done
Solid/Transparent/Wireframe modes. Wireframe uses Bevy's `Wireframe`
component with **no** `MeshMaterial3d` attached at all — confirmed from
source that the wireframe render phase only requires `Mesh3d`, so this
gives edges-only with no solid fill underneath rather than an overlay on
top of a solid cube. One entity per voxel, deliberately unoptimized. Off by
default; `DEBUG_CUBE_MODE` picks the mode when enabled.

### 7. Mesher 1 — Dual Contouring with averaged vertices — done
`common.rs` walks every sign-changing grid edge (including the implicit
`Air` ring outside the grid) and builds a quad from the 4 surrounding DC
cells, memoizing each cell's vertex. `surface_nets.rs` places each cell's
vertex at the mean of its edge crossings.

**Known limitation, found and characterized (not fixed) during this pass:**
at a "diagonal saddle" configuration — two opaque voxels touching only
diagonally, the other two corners of that face air — the mesh gets a small
crack (verified with an automated watertightness checker in
`surface_nets.rs`'s tests). This is the classic "ambiguous face" problem
from isosurface extraction, and it's inherent to *naive* Surface Nets, not
a bug in this implementation: the winding-selection rule used here is
mathematically identical to the well-established `fast_surface_nets` crate's
`maybe_make_quad`, confirmed both by reading its source and by rendering
the same shape through it side by side in the demo scene (`reference_surface_nets.rs`) — the crack shows up there too. Properly
resolving it needs real Hermite/gradient data (a QEF solve, the thing this
mesher deliberately doesn't use) or a Marching-Cubes-style asymptotic
decider; out of scope for now, tracked as a documented test
(`diagonal_saddle_is_a_known_naive_limitation`) rather than left as a
silent surprise.

### 8. Mesher 2 — Dual Contouring with axis-aligned normals — done
`axis_aligned_dc.rs`: each axis's coordinate is the mean of that axis's own
edge crossings; an axis with none falls back to the cell's centre. A lone
voxel test confirms every output vertex lands exactly on a true cube corner
(unlike mesher 1's rounded pull-in).

### 9. Mesher 3 — Hardness blend — done
`hardness_blend_dc.rs`: `averaged.lerp(axis_aligned, cell.hardness)`, where
`Cell::hardness` (computed in `common.rs`, since that's where the corner
data already is) is the weight-averaged hardness of the cell's opaque
corners. Verified to reduce to *bit-exact* matches with mesher 1/2 at
hardness 0.0/1.0 respectively, and that the blend at an intermediate
hardness is exactly the lerp of the other two meshers' own placement for
that cell.

### 10. Mesh conversion + real rendering — done
`mesh_convert.rs` converts `MeshData` to a real `bevy::Mesh`.
`voxel_mesh_view.rs` reacts to newly-spawned `VoxelGridComponent` entities,
runs whichever mesher their `MesherChoice` names, and spawns a child mesh
entity. Colour is sampled from the grid's first non-air voxel — correct for
every demo scene except the mixed-material ones, where it's a known,
accepted simplification (real per-vertex colour was never load-bearing for
what these demos are checking: mesh *shape*, not texture).

### 11. Demo scene — done
All three meshers, plus the `fast_surface_nets` reference and the
hardness-blend mesher, are represented across nine side-by-side scenes in
`main.rs`:
- rolling hill (mesher 1) and the same hill through the reference crate,
- a brick wall through mesher 2, and the same wall through mesher 1 for
  comparison,
- a wood branch (mesher 1),
- a block half dirt/half brick through mesher 3 (the blend *across* a
  material boundary),
- the exact same voxel sphere three times — dirt/wood/brick, i.e. hardness
  0.0/0.5/1.0 — through mesher 3, isolating what each hardness value alone
  looks like from the cross-boundary blend.

Debug sphere/cube renderers remain available via `config.rs` flags, layered
over any of the above.

### 12. Tuning flags — done
`config.rs` holds `SHOW_DEBUG_SPHERES`, `DEBUG_SPHERE_RADIUS_SCALE`,
`SHOW_DEBUG_CUBES`, `DEBUG_CUBE_MODE`, `DEBUG_CUBE_TRANSPARENT_ALPHA`,
`SHOW_MESHED_SURFACE`, loaded into a `DebugConfig` resource at startup.
Recompiling to change values remains fine; the resource wrapper leaves room
for a runtime debug UI later without another refactor.

### 13. Scene labels — planned, not started
Requested alongside the milestone-11 polish: a text label per demo scene
(e.g. "mesher 1", "hardness 0.5") so the comparisons are legible without
cross-referencing `main.rs`. Deferred rather than bolted on quickly because
Bevy 0.19 has no built-in world-space/billboarded text — `bevy_text`'s
`Text2d` is for `Camera2d` scenes, not label-in-3D-space. The real options:
1. UI `Node` + `Text` (screen-space), with a system that recomputes each
   label's screen position every frame from `Camera::world_to_viewport`
   applied to a stored 3D anchor position — needs a marker component
   linking a UI node to its anchor, and must run after camera movement.
2. A mesh-based "3D text" approach (e.g. via a text-to-mesh crate), so
   labels are real world-space objects — heavier, but no per-frame
   projection bookkeeping.
Leaning toward (1) since it's pure Bevy with no new dependency, but hasn't
been scoped in detail yet.

## Deliberately out of scope for this pass

- Chunking, LOD, streaming/infinite worlds.
- Sparse voxel storage.
- Wood voxel direction/orientation data (see `DESIGN.md` open question) —
  plain diagonal DC connectivity turned out fine on the wood branch demo;
  never needed.
- Translucent materials (glass, water) and the refractive-index-style
  question of two translucent materials meeting — `Air` is the only
  non-opaque material for now (see `DESIGN.md`).
- Runtime terrain editing (construction/deconstruction *interactions* — the
  data model supports it, but no player-facing dig/place tooling yet).
- Live/incremental remeshing: if a grid changes, regenerating its mesh from
  scratch is fine. Fast incremental updates are planned to come *after*
  chunking exists, not before.
- Any texturing beyond flat debug colours; real per-vertex colour for
  mixed-material meshes (see milestone 10).
- Greedy meshing / face culling / entity-batching optimizations for the
  debug renderers — one entity per voxel is fine until it's actually a
  problem.
- Properly resolving the diagonal-saddle crack in mesher 1 (see milestone
  7) — documented, not fixed.

## Resolved decisions

- Fly camera: hand-rolled (no `bevy_flycam` dependency).
- Bevy version: `0.19.0`.
- Wireframe rendering: Bevy's built-in `WireframePlugin`/`Wireframe`
  component, working as expected (mesh-with-no-material approach for
  edges-only rendering).

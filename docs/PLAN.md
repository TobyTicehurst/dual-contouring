# Implementation Plan

Status: **milestones 1–12 implemented.** Milestone 13 (scene labels) is
planned but not started — see below. Each milestone was a working,
compileable state before moving to the next. See `DESIGN.md` for the data
model and meshing rationale behind these steps. See `## Roadmap` below for
what comes after the milestones (chunking, LOD, performance, seamless
multi-material meshing), and `## Future` for what comes after that
(remeshing, terrain generation, tree generation).

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
      debug_active_cells.rs       # wireframe cube per active Dual-Contouring cell (the dual grid)
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

Added alongside (same category, added later): `debug_active_cells.rs` — a
wireframe cube per active Dual-Contouring *cell* rather than per voxel,
using `common::active_cells` (a cell is active iff its 8 corners don't all
share the same opaqueness — independent of mesher choice or
`diagonal_bridging`, since bridging only changes how an active cell's
crossings get grouped, never whether it's active). Offset by half a unit
from the voxel debug cubes, since a cell's own corners are voxel positions.
Off by default (`SHOW_DEBUG_ACTIVE_CELLS`); coloured differently
(`DEBUG_ACTIVE_CELL_COLOR`, via `WireframeColor`) so it's distinguishable
from voxel wireframes when both are on.

### 7. Mesher 1 — Dual Contouring with averaged vertices — done
`common.rs` walks every sign-changing grid edge (including the implicit
`Air` ring outside the grid) and builds a quad from the 4 surrounding DC
cells, memoizing each cell's vertex. `surface_nets.rs` places each cell's
vertex at the mean of its edge crossings.

**Known limitation, found and characterized during this pass, later fixed:**
at a "diagonal saddle" configuration — two opaque voxels touching only
diagonally, the other two corners of that face air — the mesh got a small
crack (verified with an automated watertightness checker in
`surface_nets.rs`'s tests). This is the classic "ambiguous face" problem
from isosurface extraction, and was inherent to *naive* Surface Nets, not a
bug specific to this implementation: the winding-selection rule used here
was mathematically identical to the well-established `fast_surface_nets`
crate's `maybe_make_quad`, confirmed both by reading its source and by
rendering the same shape through it side by side in the demo scene
(`reference_surface_nets.rs`) — the crack showed up there too.

**Fixed** by `common::opaque_components`/`build_cell_group`: the root cause
was that a single "ambiguous" cell's 6 edge crossings (3 belonging to each
diagonal voxel) were all averaged into *one* shared dual vertex, forcing two
genuinely disjoint surface patches through a single point. The fix groups a
cell's opaque corners into connected components (via cube-edge adjacency —
two opaque corners are in the same component only if some path of cube
edges between them stays on opaque corners the whole way), and gives each
component its own vertex instead of merging them. This is the same
density-driven idea behind Marching Cubes' asymptotic decider, adapted to
this cell-based dual-vertex scheme rather than MC's per-cell triangulation
tables. Verified fixed (0 non-manifold edges) for both the tested diagonal
case and its mirror image (a solid block missing the same diagonal corner
pair, which turns out to have never actually been ambiguous — see
`mirrored_diagonal_air_case_was_never_actually_ambiguous`, since the
*opaque* side there is already a single connected blob). Visible directly
in the demo scene's "diagonal saddle" grid.

Splitting correctly broke the wood branch demo, though: wood's whole
purpose (see milestone 4/`wood_branch`) is voxels that only touch
diagonally reading as one connected branch, which is exactly the case
`opaque_components` now (correctly, in general) treats as disconnected.
Fixed with a new `MaterialProperties::diagonal_bridging` flag (`true` only
for `wood`): `opaque_components` still requires real grid-edge adjacency to
connect two corners by default, but also connects them across a face *or
space* diagonal when *both* corners' materials have `diagonal_bridging`
set — letting wood opt back into the old always-merged behaviour (including
its own version of the same crack/pinch-point this milestone otherwise
fixes) while every other material keeps the corrected one. Space-diagonal
bridging (the cube's two opposite corners, touching at only a single point)
was added after the face-diagonal-only version turned out to still leave a
gap in the wood branch demo — a long enough branch steps through a
corner-only touch too (see `differing_axis_count`), not just face-diagonal
ones.

(A fake-volume mesher was prototyped to also give diagonal joins real width
at the pinch point, instead of just merging the two voxels' crossings into a
single shared vertex — reverted in favour of just using this bridging-only
behaviour for branches.)

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
All three meshers, plus the `fast_surface_nets` reference, are represented
across eleven side-by-side scenes in `main.rs`:
- rolling hill (mesher 1) and the same hill through the reference crate,
- a brick wall through mesher 2, and the same wall through mesher 1 for
  comparison,
- a wood branch (mesher 1),
- the diagonal-saddle pair (two voxels touching only diagonally, on a small
  dirt floor) through mesher 1 — watertight since the milestone 7 fix — and
  the same grid through the `fast_surface_nets` reference crate, which still
  cracks there (it has no equivalent fix),
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

## Roadmap

What comes after the milestones above, in dependency order — each phase
builds on the one before it. Not started.

### A. Chunking — storage/coordinates/boundary sampling done, LOD and lifecycle still open

Split the single `VoxelGrid` into fixed-size sub-grids, each with its own
entity, mesh, and (eventually) re-mesh lifecycle. Foundational: LOD,
meaningful performance testing, and later remeshing/streaming all need it.
Matches the standing decision (from the original project brief) to
implement chunking before fast/incremental remeshing.

Chunk size: **32×32×32** (32,768 voxels/chunk) by default
(`chunk::DEFAULT_CHUNK_SIZE`) — a reasonable middle ground against typical
sizes used elsewhere (e.g. Minecraft's 16³ sections): large enough to keep
entity/draw-call counts sane at world scale, small enough to bound a single
chunk's mesh-generation cost and keep future remeshing latency low. Kept
tunable, not fixed: `VoxelWorld::chunk_size` is a per-world field, not a
global constant, so this can still change once real performance data
exists.

The three open design questions this phase started with are now resolved:
- **Storage:** `core::chunk::VoxelWorld` wraps a plain
  `HashMap<ChunkCoord, VoxelGrid>` — confirmed as the actual right choice,
  not just a stopgap, since chunks are sparse and the world is conceptually
  unbounded (this also gives "sparse voxel storage" for free: `set` lazily
  creates a chunk on first write, so all-air regions never allocate one).
- **Coordinate system:** world voxel coordinate → chunk coordinate via
  `div_euclid`, → local (`0..chunk_size`) coordinate via `rem_euclid`
  (`VoxelWorld::chunk_coord_for`/`local_coord_for`), correct for negative
  world coordinates too. `ChunkCoord(IVec3)` is the `HashMap` key;
  `VoxelWorld::chunk_origin` maps a `ChunkCoord` back to its world-space
  voxel origin (`coord * chunk_size`).
- **Boundary sampling:** a new `voxel_source::VoxelSource` trait (`get`,
  `size`) abstracts "anything meshing can read voxels from"; `meshing::common`
  (and each real mesher's `pub fn mesh`) now takes `&dyn VoxelSource`
  instead of `&VoxelGrid` concretely, with `VoxelGrid` itself implementing
  it unchanged (out-of-bounds still reads `Air`). `VoxelWorld::view` hands
  out a lazy `ChunkView` — the second implementation — that reads a chunk's
  own local range normally but, for the single ring of positions
  `mesh_with` ever queries just past a grid's own `size()`, looks up the
  real neighbouring chunk (including diagonals, via `VoxelWorld::get`)
  instead of assuming `Air`. No copying, and (this took a false start to
  learn) it has to work this way rather than building a padded *copy* of
  each chunk to hand to the existing `&VoxelGrid`-based meshers: `mesh_with`
  derives its own scan bounds from `grid.size()`, so a bigger padded grid
  made it mesh the halo region too, producing duplicate/overlapping
  geometry between adjacent chunks instead of a clean shared seam.
  Decoupling "how far a mesher can look" (`ChunkView`, effectively
  unbounded) from "how far it scans" (`ChunkView::size()`, still just
  `chunk_size`) is what actually fixes it. Verified watertight across a
  chunk boundary both by a unit test
  (`chunk::tests::two_adjacent_chunks_mesh_watertight_across_their_shared_boundary`,
  reusing `meshing::common::count_non_manifold_edges` — this is what caught
  the padded-copy bug before it shipped) and visually, via the new "chunked
  rolling hill (multi-chunk)" demo scene in `main.rs` (an 80×80 hill
  spanning 3×3 chunks, built by splitting the existing `rolling_hill_slab`
  generator with `VoxelWorld::from_dense`). The `fast_surface_nets`
  reference mesher (see milestone 7) builds its own SDF directly against a
  concrete `VoxelGrid` rather than through `VoxelSource`, so it doesn't
  participate in chunking — it's a single-grid comparison tool only,
  `render::chunk_mesh_view` just skips it.

Still open, deliberately deferred to their own later roadmap phases:
- **Lifecycle:** generalise the existing `Added<VoxelWorldComponent>`
  reactive spawn pattern (`render::chunk_mesh_view`, mirroring
  `voxel_mesh_view`'s `Added<VoxelGridComponent>`) to also react to
  *changes*, not just additions — needed once remeshing (see `## Future`)
  lands, not before.
- **LOD:** every chunk meshes at full resolution today; per-chunk/
  octree-adaptive resolution is `## Roadmap` phase C.

### B. Seamless multi-material meshing — done

`voxel_mesh_view.rs` used to sample one material's colour for an entire mesh
entity (the grid's first non-air voxel) — a known, accepted simplification
noted since milestone 10. Two different opaque materials touching never
generated a seam/crack in the *geometry* (per `DESIGN.md`'s sign rule —
only opaque/non-opaque differences generate a surface at all), but there
was no way to show *which* material was where on a single continuous mesh.

Implemented via real per-vertex colour, following the "likely approach"
this phase started with almost exactly:
- `MeshData` gained a `colors: Vec<Color>` buffer alongside
  positions/normals (`core::voxel::Color`, kept Bevy-free like the rest of
  `core`); `push_triangle` now takes a `[Color; 3]` per triangle.
- `common::Cell` gained a `color` field, computed in `build_cell`/
  `build_cell_group` the same way `hardness` already was — the weight-
  averaged `color` of a cell/component's own opaque corners (same loop,
  same weighting, just blending colour instead of hardness). Applies to
  every mesher for free, since none of them needed to change: `place_vertex`
  was never responsible for colour, only position.
- `mesh_convert.rs` inserts `Mesh::ATTRIBUTE_COLOR`, converting each sRGB
  `core::voxel::Color` to linear (`Color::srgb(..).to_linear()`) — feeding
  sRGB values in directly would wash the blend out. Bevy's PBR pipeline
  multiplies a mesh's vertex colour into its material automatically once
  the attribute is present, so `voxel_mesh_view`/`chunk_mesh_view` now just
  spawn every mesh with `base_color: Color::WHITE` and let the mesh's own
  colour show through — this *removed* the old first-non-air-voxel
  `base_color_of` sampling entirely rather than adding a parallel path.
- `reference_surface_nets.rs` (the `fast_surface_nets` comparison adapter)
  doesn't go through `Cell` at all, so it kept the old flat-colour-per-mesh
  behaviour, just formalised as a uniform per-vertex colour instead of a
  material-level one — fine, since every demo scene using it is single-
  material.
- Verified on `demo_scenes::mixed_hardness_block` (dirt/brick): a unit test
  (`common::tests::cell_color_is_the_weight_averaged_color_of_its_opaque_corners`)
  checks the blend formula directly, and
  `hardness_blend_dc::tests::mixed_hardness_block_mesh_has_more_than_one_vertex_colour`
  checks the resulting mesh actually carries more than one distinct vertex
  colour (not just formula-correct in isolation). Confirmed visually too —
  the block's mesh shows a real gradient from dirt-brown to brick-red
  across the seam, not a hard per-triangle swap.
- Chunk boundaries: unaffected by construction — `ChunkView` (Roadmap phase
  A) already gives every mesher real neighbouring-voxel data at a chunk's
  edge, colour included, since colour is computed from the same corner data
  position is.

### C. LOD — chunk-level resolution selection done, seam stitching and octree DC still open

Explore two complementary techniques:
1. **Transvoxel algorithm** — the standard technique for stitching
   seamless, crack-free transitions between chunks meshed at *different*
   whole-chunk LOD/resolution levels, using special "transition cells" at
   mismatched-resolution chunk boundaries. Simpler mental model: one
   resolution per chunk, picked by distance.
2. **Octree-based adaptive Dual Contouring** — resolution varies smoothly
   *within* a mesh (or within/across chunks) via an octree, instead of
   swapping whole discrete chunk LODs. Gives sub-chunk resolution control —
   e.g. making tree/wood voxels finer-grained than surrounding terrain
   voxels without forcing the whole chunk to a higher resolution.

Not mutually exclusive — a plausible end state is octree-adaptive
resolution *within* each chunk, with Transvoxel-style stitching wherever
two different resolutions meet (chunk-to-chunk or octree-node-to-node).
Prototype both independently first to see which fits before committing.

**Increment 1, done:** the resolution-selection half of technique 1 —
picking one mesh resolution per chunk based on distance from the camera,
*without* stitching the resulting seams yet (deliberately scoped down, see
below).

- `core::voxel_source::DownsampledView` wraps any `&dyn VoxelSource`
  (concretely, a chunk's `ChunkView`) and samples every `stride`-th voxel:
  `get(pos) = source.get(pos * stride)`, `size() = source.size() / stride`.
  This is the same decoupling trick `ChunkView` used for chunk boundaries —
  "how the source is sampled" is separated from "how far `mesh_with`
  scans" — so `common.rs` and every mesher needed zero changes; they already
  only consume `&dyn VoxelSource`. Because it wraps a `ChunkView` rather
  than a `VoxelGrid` directly, cross-chunk boundary lookups keep working
  unchanged at any stride: the ring `mesh_with` queries just past a
  downsampled chunk's (smaller) `size()` still resolves to a real
  neighbouring-chunk voxel, just sampled at `stride` spacing like
  everything else.
- `render::chunk_mesh_view::lod_stride_for_distance` maps distance-from-
  camera to a stride via a few hardcoded bands (`<100`→1, `<200`→2,
  `<400`→4, else→8) — starting values, not yet backed by real performance
  data, tunable later just like `chunk::DEFAULT_CHUNK_SIZE`. Only
  power-of-two divisors of the chunk size are valid strides
  (`DownsampledView::new` asserts this in debug builds).
- Selection happens once, at spawn time, in `spawn_chunked_meshed_views`
  (which already reacts to `Added<VoxelWorldComponent>`): it reads the
  camera's spawn-time `Transform` and the chunked world entity's own
  `Transform` (chunks are meshed in the parent entity's local space, so
  distance has to account for its world offset too), computes each chunk's
  distance to the camera, and meshes a `DownsampledView` of that chunk
  instead of the bare `ChunkView`. A downsampled mesh comes out in
  downsampled-local space, corrected by giving the spawned chunk entity a
  uniform `Transform` scale of `stride` alongside its existing translation
  — uniform scale doesn't distort the mesh's already-correct normals, so
  nothing else needed adjusting. Not dynamic: re-selecting as the camera
  moves needs the same reactive lifecycle work already deferred for
  remeshing (phase A's "react to changes, not just additions" note) — out
  of scope here too.
- Verified by unit tests (`voxel_source::tests` — stride 1 is a no-op,
  stride 2 samples the expected voxel, and a downsampled mesh has
  measurably fewer triangles than full resolution for the same shape;
  `chunk_mesh_view::tests` — the distance bands pick the expected stride at
  each boundary) and visually via a new demo scene, "chunked rolling hill
  (LOD demo, varying resolution with distance)" — a 128×128 hill (4×4
  chunks) placed so its distance to the camera spans multiple bands; a
  logged per-world stride histogram (`chunk_mesh_view`'s `info!`) confirmed
  chunks at strides 1, 2, and 4 all appear in that one scene. The existing
  near chunked-hill demo (`chunked rolling hill (multi-chunk)`, at
  `(60,0,0)`) also now shows a mix of stride 1/2 chunks rather than being
  purely full-resolution — an accurate side effect of applying the same
  distance-based selection everywhere, not a special case for the new demo.
- **Still open, deliberately deferred:**
  - **Seam stitching:** chunks meshed at different strides don't line up at
    their shared boundary, so visible cracks/steps are expected at LOD
    transitions in the new demo — see "Increment 2" below.
  - **Octree-adaptive DC:** entirely unexplored; technique 2 above.
  - **Dynamic re-selection:** LOD is chosen once at spawn, not
    recomputed as the camera moves — needs the same reactive lifecycle
    generalization phase A already flagged for remeshing.

**Increment 2, attempted, not working yet:** seam stitching. Three design
passes, each corrected by the automated watertightness test catching a real
flaw in the previous one:

1. *Fan-bridging* (rejected): mesh both chunks fully independently, bridge
   the seam with new triangles after the fact. Wrong shape of fix — both
   sides already produce complete, self-consistent (just differently-
   detailed) surfaces, so bridging either duplicates geometry or leaves
   edges with no matching partner.
2. *Merged single-vertex cell, first cut* (rejected before being coded
   further): catching two real gaps before writing more of it — cells can
   legitimately need more than one vertex (the same saddle-point splitting
   `common::opaque_components` already handles), and a naive 13-point
   Transvoxel-style layout only covers a transitioning face's own regular
   interior, not its perimeter.
3. **Current attempt**: `core::meshing::transition` computes one shared
   vertex per coarse-resolution footprint (`transition_vertex`, a 13-point
   sample layout — 9 fine points on the boundary plane, 4 coarse points one
   `coarse_stride` back — reusing `build_cell`'s own crossing-interpolation
   formula), and `common::mesh_with_overrides` (a new parameter on
   `mesh_with_boundary`) lets both chunks' otherwise-untouched scans
   reference that vertex instead of computing their own independent one,
   via a pre-seeded `get_or_build_vertex` cache entry — no suppression, no
   new triangles, just redirecting which vertex existing quads use.
   `chunk::balance_lod_levels` was also strengthened (still a single,
   provably-terminating relaxation — see its own doc comment for why the
   naive "raise the over-constrained chunk" version could oscillate
   forever) to guarantee a chunk has at most one transitioning face, so
   only the 2:1, single-face case needs handling at all.

   `TangentialSpan` (`Regular`/`RingLow`/`RingHigh`) generalizes the
   footprint to also cover a transitioning face's *perimeter* — wherever
   another chunk boundary (even an ordinary same-resolution one, like the
   edge of solid ground) crosses it, that boundary's own "one ring beyond"
   cells sit right on the transitioning layer and need the same shared-
   vertex treatment, not just the face's regular interior. This was found,
   and fixed, via the same test-driven process.

   **Where it stands**: `transition_vertex`, `fine_override_cells`/
   `coarse_override_cell`, and `mesh_with_overrides` are each independently
   correct per their own unit tests, and direct inspection confirmed fine
   and coarse sides do compute bit-identical shared vertices. But the full
   cross-chunk watertightness test
   (`transition::tests::fine_and_coarse_chunks_mesh_watertight_across_a_2_to_1_boundary`,
   currently `#[ignore]`-d with the specifics of the failure on the test
   itself) still fails: a connectivity gap *within one side's own mesh*,
   where geometry adjacent to two different override footprints doesn't
   connect the way manual analysis of `scan_edge`/`get_or_build_vertex`
   predicts it should. Debugging (opacity checks, direct triangle
   inspection) confirmed the specific edge in question *should* be produced
   by an active, correctly-scanned quad, but isn't — root cause not yet
   found. **Not wired into `render::chunk_mesh_view`** — LOD boundaries
   still show the increment-1 seams, unchanged. Next attempt should start
   from the `#[ignore]`-d test rather than from scratch.

### D. Performance testing and optimization

Only meaningful once chunking (and ideally LOD) exist — a single small demo
grid isn't representative of real-world load.
- Baseline: timing instrumentation around mesh generation (per-chunk and
  total), frame time, chunk load/unload cost.
- Realistic test scenes: many chunks, mixed materials, varying LOD
  distances.
- Optimization order, highest payoff first: multithreaded per-chunk meshing
  (chunks are already independent once chunking exists, so this is close to
  free — e.g. via `rayon`) → re-profile → only then consider SIMD (e.g.
  migrating hot position math from glam's `Vec3` to the SIMD-aligned
  `Vec3A`) or GPU compute, and only if profiling actually shows they're
  warranted rather than assumed.

Lightweight ad-hoc profiling can and should happen earlier too (e.g. right
after chunking) — this phase is dedicated, systematic measurement and
tuning once the architecture is stable, not the only time performance gets
looked at.

## Future (further out, not yet scoped in detail)

- **Remeshing (construction and deconstruction):** player-facing dig/place
  tooling, plus incremental re-mesh of just the modified chunk(s) instead
  of a full rebuild — needs dirty-chunk tracking and, eventually, fast
  incremental meshing algorithms rather than just re-running the current
  from-scratch mesher on a smaller grid.
- **Terrain generation:** procedural generation of voxel grid content
  (noise-based heightmaps, caves, biomes) — everything today is
  hand-authored in `demo_scenes.rs`.
- **Tree generation:** procedural generation of wood-branch-style
  structures, generalizing the current hardcoded `wood_branch` demo into
  something randomized/parametric.
- **Debug renderer optimization:** greedy meshing / face culling / entity-
  batching for the sphere/cube/active-cell debug renderers (`debug_spheres.rs`,
  `debug_cubes.rs`, `debug_active_cells.rs`) — currently one entity per
  voxel, deliberately unoptimized (see milestones 5/6). Fine at today's demo
  scales; worth revisiting once these are used on larger or chunked scenes
  where per-voxel entity count becomes the actual bottleneck.
- **Debug view bug on the rolling hill demo:** the debug renderers reportedly
  render incorrectly specifically for the rolling hill demo (user-reported;
  root cause not yet diagnosed). A reasonable first suspect is
  `rolling_hill_slab`'s fractional-weight top layer, which most other demo
  scenes don't exercise — needs investigation before a fix.
- **Per-demo scene selection:** `main.rs` currently always spawns every demo
  scene side by side in one combined scene. Add a way to load just one demo
  in isolation (e.g. picked via a CLI arg or env var, mirroring the
  `VOXELS2_CAMERA_POS`/`VOXELS2_CAMERA_LOOK_AT` pattern added for automated
  screenshots) while keeping the existing "all demos at once" scene as the
  default/an option — useful for focused debugging and for cleanly framing a
  single demo in a screenshot.
- **Automated in-app screenshot capture is unreliable:** the
  `bevy_ci_testing`-driven `Screenshot`/`ScreenshotAndExit` mechanism (see
  `VOXELS2_CAMERA_POS`/`VOXELS2_CAMERA_LOOK_AT` above) reliably captured real
  geometry the first time it was tried, then returned a blank (flat
  background colour, no geometry) capture on every subsequent attempt in the
  same session, including replaying camera coordinates that had just worked
  — with no error/warning logged and a clean exit either way. Not yet
  diagnosed: candidates include GPU/driver state across repeated rapid
  process launches, and (untested) an interaction with the window-visibility
  fix (window starts `visible: false` and is shown a few frames in — see
  `make_window_visible` in `main.rs`) versus the screenshot's own
  swap-chain-texture readback timing. Until this is root-caused, prefer an
  external screenshot (e.g. capturing the actual OS window from outside the
  process) over the in-app mechanism, or just eyeball it by running the app
  directly.

## Deliberately out of scope for this pass

- Translucent materials (glass, water) and the refractive-index-style
  question of two translucent materials meeting — `Air` is the only
  non-opaque material for now (see `DESIGN.md`).
- Any texturing beyond flat debug/per-material colours (real UV-mapped
  textures) — separate from, and further out than, the seamless
  multi-material colour work in `## Roadmap` above.

Chunking, LOD, runtime terrain editing, live remeshing, and debug renderer
optimization are no longer "out of scope" in the open-ended sense — see
`## Roadmap` and `## Future` above for the planned order.

## Resolved decisions

- Fly camera: hand-rolled (no `bevy_flycam` dependency).
- Bevy version: `0.19.0`.
- Wireframe rendering: Bevy's built-in `WireframePlugin`/`Wireframe`
  component, working as expected (mesh-with-no-material approach for
  edges-only rendering).

# Voxel Rendering Exploration — Design Doc

## Goal

Explore voxel rendering techniques for a future game engine. The core tension
to resolve: a single voxel representation and meshing pipeline must be able to
produce **smooth, organic surfaces** (rolling hills, water, clouds) **and**
**crisp right angles** (walls, cliffs, bricks) **and** **thin/detailed
features** (branches, twigs), all from the same underlying data, and the
terrain must remain fully constructible/deconstructible at runtime (no
precomputed/baked geometry assumptions).

## Voxel Data Model

Each voxel stores:

| Field      | Type              | Meaning                                                                 |
|------------|-------------------|--------------------------------------------------------------------------|
| `weight`   | `f32` in `(0, 1]` | "Fullness" of the voxel; governs how far the surface sits from the voxel centre (see the crossing-point formula below). `0` is not a valid value — a voxel that would be "0% full" is simply `Air` instead. The mesher trusts this invariant and does no validation of its own; it's enforced wherever voxels are authored/edited. |
| `material` | enum / index       | What the voxel is made of and how it behaves/renders. |

A unit voxel is a cube from `(-0.5,-0.5,-0.5)` to `(0.5,0.5,0.5)` relative to
its centre, and voxel centres sit on integer coordinates.

**Whether a surface exists on an edge (sign):** purely a function of
material, not weight. Every material is either `opaque` or not (`Air` is the
only non-opaque material to start with). An edge between two voxel centres
generates a surface iff the two materials differ **and** at least one side is
non-opaque. Two different opaque materials touching (e.g. dirt against
brick) never generate a surface between them — they just meet with a hard
material change and no visible seam, per the original spec. Weight plays no
role in *whether* a surface exists, only *where* it sits.

**Where the surface sits (crossing point):** on a surface-generating edge
between an opaque voxel `O` (weight `wO`) and its non-opaque neighbour, the
crossing point sits at distance `0.5 * wO` from `O`'s centre, along the edge
toward the neighbour. `weight = 1` puts the surface exactly on the shared
face (a "full" cube reaching all the way to its neighbour); `weight → 0`
pulls it back toward `O`'s own centre. This is a linear-extent
interpolation, not a volume-fraction one.

This covers every case that exists today (opaque vs. `Air`). It doesn't yet
cover translucent materials (glass, water) — those are planned to behave
like `Air` for the *opaque-neighbour-generates-a-surface* rule above, but
additionally generate their own surface against `Air` (e.g. a water voxel
against air renders a water surface), and translucent-against-translucent
would need some added concept of transparency/refractive index. None of that
is needed yet since `Air` is the only non-opaque material, but the `opaque:
bool` field below is named to leave room for a future third state rather
than being a strict `transparent`/`opaque` binary.

### Material Properties

Materials are looked up by index into a material table (not hardcoded match
statements), so adding a new material is just a new table entry. Initial
properties:

| Property    | Type    | Meaning |
|-------------|---------|---------|
| `name`      | string  | Debug/UI label |
| `opaque`    | bool    | Whether this material counts as "solid" for sign/cell-activity purposes (see interpolation rule above). `false` only for `Air` today; named to leave room for a future translucent category rather than being read as a strict opposite of "transparent". |
| `hardness`  | `f32` in `[0, 1]` | 0 = fully smooth (rounded, like dirt), 1 = fully rectilinear (right angles, like brick). Drives the blend in the third mesher (see below). |
| `color`     | RGBA    | Debug/base render colour until real texturing exists |

Initial materials: `Air (opaque = false, hardness N/A)`, `Dirt (hardness 0.0)`,
`Brick (hardness 1.0)`, `Wood (hardness ~0.5, see below)`.

## Meshing Strategy

All meshers consume the same voxel grid and produce the same intermediate
mesh representation (positions/normals/indices), so they're interchangeable
and comparable side-by-side in the demo scene.

We build **one shared Dual Contouring skeleton**: for every 2×2×2 cell of
voxel corners that has a sign change (some corners "inside", some "outside"
relative to the air/solid threshold), compute edge-crossing points via
`weight` interpolation, group the cell's opaque corners into connected
components (two corners connect if they share a real cube edge; a diagonal
touch — no shared edge — only connects them if *both* voxels' materials have
`diagonal_bridging` set, see below), then place one vertex **per component**
and quad it between adjacent cells' vertices (standard DC connectivity). In
the overwhelmingly common case a cell's opaque corners are already one
connected blob, so this reduces to "one vertex per active cell"; it only
produces more than one when a cell's solid region is genuinely disconnected
(e.g. two voxels touching only diagonally). What differs between the three
meshers is **how each component's vertex position is computed** — this
replaces the Hermite normal data classic DC relies on (which we don't have,
since our voxels have no direction/gradient data by design):

1. **Averaging (Surface-Nets-style):** vertex = average of all edge-crossing
   points for that cell. No normal information used. This is smooth and
   rounded — good for dirt/hills/water.

2. **Axis-aligned normals:** each edge crossing is assigned the cardinal
   normal of the edge it lies on (an X-edge crossing gets normal `±X`, etc. —
   this is the one direction we *do* know for free from the grid). Solving
   the (now diagonal, so trivially decoupled per-axis) QEF against these
   axis-aligned constraints pulls the vertex onto axis-aligned planes,
   producing right angles — good for brick/walls/cliffs. Each axis is solved
   independently as the mean of that axis's edge-crossing coordinates; if a
   cell has *no* crossings on a given axis (common — e.g. the interior of a
   flat floor has no X/Y crossings, only Z), that axis is underdetermined and
   falls back to the cell's geometric centre for that component. This
   fallback is correct by construction, not just a stopgap: it's exactly
   what produces a clean flat grid-aligned plane in that case.

3. **Hardness blend:** vertex position = `lerp(averaged_pos, axis_aligned_pos, hardness)`
   where `hardness` is derived from the materials contributing to that cell
   (e.g. weight-averaged hardness of the non-air corners). `hardness = 0`
   reduces exactly to mesher 1, `hardness = 1` reduces exactly to mesher 2.
   This is the mesher actually used for mixed-material terrain.

**Known limitation, deferred: coincident (not intersecting) vertices from
different components.** A cell whose opaque corners split into multiple
connected components (see `PLAN.md` milestone 7 — `opaque_components`, added
after this doc was written, to fix a crack at exactly this kind of
diagonal-touching cell) gives each component its own vertex — but meshers 2
and 3 can place two *different* components' vertices at the *exact same
point*, not just close together.
`axis_aligned_dc` computes each axis independently from that axis's own
crossings alone, with no cell-wide averaging pulling it back toward its own
corner; at `weight = 1.0`, an isolated corner's vertex lands exactly on the
cell's geometric centre. By the cube's symmetry, *every* corner in a
maximally-split cell (the 4-component "checkerboard" case, where the opaque
corners are a full parity-class of the cube) lands at that same centre point
when every contributing voxel is full-weight — confirmed algebraically, not
just observed. The same coincidence happens for the ordinary 2-component
diagonal-saddle case too. This doesn't produce crossing/self-intersecting
triangles (each component's own quads stay locally well-formed) — just
multiple, physically unrelated components' geometry meeting at one shared
point instead of staying visibly separate. `surface_nets` (mesher 1) doesn't
have this issue, since its whole-crossing average stays anchored near its
own corner regardless of weight. Not addressed yet; revisit if it turns out
to matter visually (most likely to show up with brick, or hardness-blend at
high hardness, on diagonal-touching geometry).

**Vertex normals for shading** are a separate concern from the vertex
*position* math above and don't need to reuse the axis-aligned QEF normals.
For this pass, just compute simple flat per-face normals (cross product of
each triangle's edges) — good enough to see the meshers' shapes clearly.
Proper shading (in particular, per-voxel-material flat shading where every
vertex on a face shares one normal, which needs either vertex duplication or
a shader-side trick) is a deliberate future exploration, not part of this
pass.

**Thin/detailed features (branches):** resolved via
`MaterialProperties::diagonal_bridging` (`true` only for `wood`) — two
voxels of a bridging material that touch only diagonally (no shared face)
are grouped into one connected component instead of splitting (which is
what non-bridging materials correctly do at the same configuration — the
diagonal-saddle fix, see `PLAN.md` milestone 7), so they share one vertex
and the branch reads as continuous rather than showing a gap. The tradeoff:
that shared vertex still narrows the join to nearly a single point/line,
since there's no actual voxel data *between* the two diagonal voxels to
give it real width. A mesher that faked extra volume at these joins (a
small connecting cylinder, cloned from mesher 3) was prototyped and worked,
but was reverted in favour of this simpler bridging-only behaviour for
now — no per-voxel direction/orientation data was needed after all.

## Debug Tooling

Three debug-only visualizations, all reading directly from the voxel grid
(bypassing the mesh pipeline) so they're useful for verifying data
independent of meshing bugs:

- **Sphere-per-voxel:** a sphere at the centre of every non-air voxel, radius
  scaled by `weight`. Good for eyeballing raw grid state independent of any
  mesher.
- **Cube-per-voxel:** a unit cube at each voxel, solid/transparent/wireframe,
  toggle-able. Not optimized (no greedy meshing/culling) — it's explicitly a
  debug tool, not a renderer.
- **Cube-per-active-cell:** a wireframe cube at every Dual-Contouring cell
  that will place at least one vertex — the *dual* grid, offset by half a
  unit from the voxel cubes above (a cell's own corners are voxel
  positions). Useful for seeing exactly which cell an ambiguous/split
  configuration lands in.

All three are driven by a small set of flags (a Bevy `Resource`, defaulted
from consts) so they can be tuned without hunting through code, at the cost
of a recompile for now.

## Code Organization

Hard separation between voxel/meshing *logic* and *Bevy-specific* glue, so
the algorithms are independently testable and reusable if the rendering
backend ever changes:

- `src/core/` — no Bevy dependency. Voxel/material types, dense grid
  storage, `MeshData`, demo/test grid generators, and all mesher
  implementations (`meshing/`). Produces plain position/normal/index
  buffers, never a `bevy::render::mesh::Mesh`.
- `src/render/` — Bevy-specific glue: fly camera, converts `core::MeshData`
  output into real `bevy::render::mesh::Mesh` + `Handle<StandardMaterial>`
  entities, the three debug renderers above, `VoxelGridComponent`/
  `MesherChoice` ECS wiring.
- `src/main.rs` / `src/config.rs` — app setup, demo scene assembly,
  debug/tuning flags.

Single Cargo binary crate, not a workspace split — see `PLAN.md` for the
exact file tree.

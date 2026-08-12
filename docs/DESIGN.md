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

### Open question: per-voxel direction (wood)

Wood voxels connecting diagonally (to allow thin branches) may eventually
need a direction/orientation so branches can pack closer together and read
correctly as thin cylinders rather than blobs. This is **not implemented in
this first pass** — it's called out because it affects the data model. To
avoid a big refactor later, the voxel storage should leave room for optional
per-material auxiliary data (e.g. a side-table keyed by voxel position, or an
`Option<Direction>` field) without forcing every material to carry a
direction it doesn't need. Other materials (dirt, brick) explicitly do *not*
get directions — orientation-free building is a deliberate usability
constraint for the player.

## Meshing Strategy

All meshers consume the same voxel grid and produce the same intermediate
mesh representation (positions/normals/indices), so they're interchangeable
and comparable side-by-side in the demo scene.

We build **one shared Dual Contouring skeleton**: for every 2×2×2 cell of
voxel corners that has a sign change (some corners "inside", some "outside"
relative to the air/solid threshold), compute edge-crossing points via
`weight` interpolation, then place one vertex per active cell and quad it
between adjacent active cells (standard DC connectivity). What differs
between the three meshers is **how the cell vertex position is computed** —
this replaces the Hermite normal data classic DC relies on (which we don't
have, since our voxels have no direction/gradient data by design):

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

**Vertex normals for shading** are a separate concern from the vertex
*position* math above and don't need to reuse the axis-aligned QEF normals.
For this pass, just compute simple flat per-face normals (cross product of
each triangle's edges) — good enough to see the meshers' shapes clearly.
Proper shading (in particular, per-voxel-material flat shading where every
vertex on a face shares one normal, which needs either vertex duplication or
a shader-side trick) is a deliberate future exploration, not part of this
pass.

Thin/detailed features (branches): a standard DC cell already spans a 2×2×2
block of voxel centres, so two voxels touching only diagonally (sharing a
cell without sharing a face) already land in a common cell and get bridged
by the normal cell-vertex/quad logic — no special-cased neighbour sampling
should be required just to get a mesh to form across a diagonal join. The
open question is purely about *shape*: whether diagonal branch joints look
acceptably thin/branch-like out of the box, or whether they need the
per-voxel wood direction mentioned below to avoid reading as blobby. Treat
that as something to evaluate empirically after mesher 1 is working on a
hand-authored branch test shape, not something to pre-build.

## Debug Tooling

Two debug-only visualizations, both reading directly from the voxel grid
(bypassing the mesh pipeline) so they're useful for verifying data
independent of meshing bugs:

- **Sphere-per-voxel:** a sphere at the centre of every non-air voxel, radius
  scaled by `weight` from the start (cheap, and it's the main thing this view
  exists to show). Good for eyeballing raw grid state independent of any
  mesher.
- **Cube-per-voxel:** a unit cube at each voxel, solid/transparent/wireframe,
  toggle-able. Not optimized (no greedy meshing/culling) — it's explicitly a
  debug tool, not a renderer.

Both are driven by a small set of flags (a Bevy `Resource`, defaulted from
consts) so they can be tuned without hunting through code, at the cost of a
recompile for now.

## Code Organization

Hard separation between voxel/meshing *logic* and *Bevy-specific* glue, so
the algorithms are independently testable and reusable if the rendering
backend ever changes:

- `voxel_core` — no Bevy dependency. Voxel/material types, the dense grid
  storage, and all mesher implementations. Produces plain
  position/normal/index buffers ("Triangle"/"MeshData" structs of our own),
  never a `bevy::render::mesh::Mesh`.
- Bevy application crate/module — camera/flycam, converts `voxel_core`
  mesh output into real `bevy::render::mesh::Mesh` + `Handle<StandardMaterial>`
  entities, debug gizmo/cube rendering, demo scene assembly, input-driven
  debug flags.

See `PLAN.md` for how this is split into files and build order.

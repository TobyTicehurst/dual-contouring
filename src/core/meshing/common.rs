//! Shared Dual-Contouring cell-scanning and quad-generation logic, used by
//! every mesher in `core::meshing`. What differs between meshers is only
//! *where* each active cell's vertex is placed — see `docs/DESIGN.md`.
//!
//! There's no Hermite (gradient/normal) data to drive a classic DC vertex
//! solve, so instead of scanning every cell up front, this walks every grid
//! edge that crosses the opaque/non-opaque boundary (including one ring just
//! outside the grid's own `size()`, so meshes cap off cleanly at its edge)
//! and, for each one, builds a quad from the four Dual-Contouring cells
//! around it — memoizing each cell's vertex so it's computed once no matter
//! how many edges reference it.
//!
//! Voxel lookups go through `voxel_source::VoxelSource` rather than
//! `VoxelGrid` directly, so this same scanning logic works unchanged for a
//! single dense grid (where that outside ring reads as `Air`) and for one
//! chunk of a `chunk::VoxelWorld` (where a `ChunkView` answers that same
//! ring from the real neighbouring chunk instead).

use std::collections::HashMap;

use glam::{IVec3, Vec3};

use crate::core::grid::VoxelGrid;
use crate::core::mesh_data::MeshData;
use crate::core::voxel::{Color, MaterialTable};
use crate::core::voxel_source::VoxelSource;

/// Where the surface crosses one edge of a cell — one endpoint is opaque,
/// the other isn't. Position is in the same local grid space as voxel
/// centres (see the crossing-point formula in `docs/DESIGN.md`). `axis` is
/// the one direction we know "for free" from the grid — the edge's own
/// orientation — used by the axis-aligned mesher.
pub struct EdgeCrossing {
    pub position: Vec3,
    pub axis: Axis,
}

/// The edge crossings of one Dual-Contouring cell: the cube spanning grid
/// corners `cell_coord` to `cell_coord + (1,1,1)`. A cell only exists (is
/// "active") if it has at least one crossing, i.e. its 8 corners don't all
/// agree on opaqueness.
pub struct Cell {
    pub cell_coord: IVec3,
    pub crossings: Vec<EdgeCrossing>,
    /// This cell's effective hardness: the weight-averaged `hardness` of its
    /// opaque corners' materials, in `[0, 1]`. Only the hardness-blend
    /// mesher reads this; it's computed here (rather than in that mesher)
    /// because it needs the corner data `build_cell` already has in hand.
    pub hardness: f32,
    /// This cell's effective colour: the weight-averaged `color` of its
    /// opaque corners' materials — the same computation as `hardness`, just
    /// blending colour instead. Lets a mesh spanning a material boundary
    /// show the transition per-vertex instead of one flat colour for the
    /// whole mesh (see `docs/PLAN.md` Roadmap phase B).
    pub color: Color,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub(crate) fn unit(self) -> IVec3 {
        match self {
            Axis::X => IVec3::X,
            Axis::Y => IVec3::Y,
            Axis::Z => IVec3::Z,
        }
    }

    /// The next two axes in cyclic order (X→Y→Z→X). Used to build the loop
    /// of 4 cells around an edge along this axis; this specific ordering is
    /// what makes the resulting quad winding come out with the correct
    /// outward-facing normal (verified by the tests in this module).
    pub(crate) fn others(self) -> (Axis, Axis) {
        match self {
            Axis::X => (Axis::Y, Axis::Z),
            Axis::Y => (Axis::Z, Axis::X),
            Axis::Z => (Axis::X, Axis::Y),
        }
    }
}

/// The 8 corners of a cube, as offsets from `cell_coord`.
const CUBE_CORNERS: [IVec3; 8] = [
    IVec3::new(0, 0, 0),
    IVec3::new(1, 0, 0),
    IVec3::new(0, 1, 0),
    IVec3::new(1, 1, 0),
    IVec3::new(0, 0, 1),
    IVec3::new(1, 0, 1),
    IVec3::new(0, 1, 1),
    IVec3::new(1, 1, 1),
];

/// The 12 edges of a cube, as pairs of corner offsets differing in one axis.
const CUBE_EDGES: [(IVec3, IVec3); 12] = [
    (IVec3::new(0, 0, 0), IVec3::new(1, 0, 0)),
    (IVec3::new(0, 1, 0), IVec3::new(1, 1, 0)),
    (IVec3::new(0, 0, 1), IVec3::new(1, 0, 1)),
    (IVec3::new(0, 1, 1), IVec3::new(1, 1, 1)),
    (IVec3::new(0, 0, 0), IVec3::new(0, 1, 0)),
    (IVec3::new(1, 0, 0), IVec3::new(1, 1, 0)),
    (IVec3::new(0, 0, 1), IVec3::new(0, 1, 1)),
    (IVec3::new(1, 0, 1), IVec3::new(1, 1, 1)),
    (IVec3::new(0, 0, 0), IVec3::new(0, 0, 1)),
    (IVec3::new(1, 0, 0), IVec3::new(1, 0, 1)),
    (IVec3::new(0, 1, 0), IVec3::new(0, 1, 1)),
    (IVec3::new(1, 1, 0), IVec3::new(1, 1, 1)),
];

fn is_opaque(grid: &dyn VoxelSource, materials: &MaterialTable, pos: IVec3) -> bool {
    materials.get(grid.get(pos).material).opaque
}

/// Whether any of a cell's 12 edges cross the opaque/non-opaque boundary —
/// equivalently, whether its 8 corners don't all share the same opaqueness
/// (the two are logically the same thing on a connected graph like a cube's
/// edges, so checking corners directly is cheaper than building crossings).
/// Independent of any mesher or of `diagonal_bridging`: bridging only
/// changes how many vertices an active cell's crossings get grouped into,
/// never whether the cell is active at all.
fn is_cell_active(grid: &dyn VoxelSource, materials: &MaterialTable, cell_coord: IVec3) -> bool {
    let first = is_opaque(grid, materials, cell_coord + CUBE_CORNERS[0]);
    CUBE_CORNERS[1..]
        .iter()
        .any(|&offset| is_opaque(grid, materials, cell_coord + offset) != first)
}

/// Every cell coordinate with at least one vertex in the final mesh —
/// the same padded range `mesh_with` scans edges over, but checked directly
/// per cell rather than discovered incidentally while walking edges. Meant
/// for debug visualisation (see `render::debug_active_cells`), not the real
/// meshing path.
pub fn active_cells(grid: &VoxelGrid, materials: &MaterialTable) -> Vec<IVec3> {
    let size = grid.size();
    let (sx, sy, sz) = (size.x as i32, size.y as i32, size.z as i32);
    let mut cells = Vec::new();
    for x in -1..sx {
        for y in -1..sy {
            for z in -1..sz {
                let cell_coord = IVec3::new(x, y, z);
                if is_cell_active(grid, materials, cell_coord) {
                    cells.push(cell_coord);
                }
            }
        }
    }
    cells
}

/// `CUBE_CORNERS`/`CUBE_EDGES` offsets are always in `{0, 1}` per axis, which
/// is exactly a 3-bit index into `CUBE_CORNERS`' own fixed ordering — avoids
/// a linear search through the array every time a corner needs an index.
fn corner_index(offset: IVec3) -> usize {
    (offset.x + 2 * offset.y + 4 * offset.z) as usize
}

/// How many of the 3 axes two cube corners' offsets differ in: 1 for a
/// `CUBE_EDGES` pair (real grid adjacency), 2 for a face diagonal (the
/// classic ambiguous-face relationship), 3 for a space diagonal — the two
/// corners at opposite ends of the cube, touching at only a single point.
fn differing_axis_count(a: IVec3, b: IVec3) -> u8 {
    let diff = b - a;
    (diff.x != 0) as u8 + (diff.y != 0) as u8 + (diff.z != 0) as u8
}

/// Groups a cell's opaque corners into connected components: two opaque
/// corners are in the same component iff some path between them, stepping
/// only to other opaque corners, stays connected the whole way. A step to a
/// `CUBE_EDGES` neighbour (real grid adjacency) always counts; a step across
/// a face or space diagonal only counts when `bridges[i] && bridges[j]` —
/// i.e. both corners' voxels opted into `MaterialProperties::diagonal_bridging`
/// (see `wood`, whose thin branch-like shapes are meant to read as connected
/// even where they only touch diagonally, including the corner-only space-
/// diagonal case — a longer branch can step through one of those too).
///
/// This is the density-driven decision an asymptotic decider makes for a
/// classic Marching-Cubes ambiguous face, adapted to this cell's dual-vertex
/// scheme: a "diagonal saddle" cell (two opaque corners touching only across
/// an ambiguous face's diagonal, the other two corners of that face left
/// non-opaque) has no edge-path between its two opaque corners that avoids
/// the non-opaque ones, so — for non-bridging materials — it comes back as 2
/// separate components instead of 1. See `build_cell_group`, which uses that
/// to give the cell 2 separate dual vertices instead of forcing both surface
/// patches through one shared point (the actual mechanism behind the crack
/// this fixes for everything except diagonal-bridging materials, which
/// deliberately keep the old single-vertex behaviour).
fn opaque_components(opaque: &[bool; 8], bridges: &[bool; 8]) -> [Option<usize>; 8] {
    let mut component: [Option<usize>; 8] = [None; 8];
    let mut next_id = 0usize;

    for start in 0..8 {
        if !opaque[start] || component[start].is_some() {
            continue;
        }
        let id = next_id;
        next_id += 1;
        let mut stack = vec![start];
        component[start] = Some(id);
        while let Some(corner) = stack.pop() {
            for (n, &n_offset) in CUBE_CORNERS.iter().enumerate() {
                if !opaque[n] || component[n].is_some() {
                    continue;
                }
                let axis_count = differing_axis_count(CUBE_CORNERS[corner], n_offset);
                let is_edge = axis_count == 1;
                let is_bridged_diagonal = (axis_count == 2 || axis_count == 3) && bridges[corner] && bridges[n];
                if is_edge || is_bridged_diagonal {
                    component[n] = Some(id);
                    stack.push(n);
                }
            }
        }
    }

    component
}

/// Every `CUBE_EDGES` pair differs in exactly one axis by construction, so
/// the edge's axis is just whichever component of `b - a` is non-zero.
fn axis_of(diff: IVec3) -> Axis {
    if diff.x != 0 {
        Axis::X
    } else if diff.y != 0 {
        Axis::Y
    } else {
        Axis::Z
    }
}

/// Builds the cell at `cell_coord` as a single merged whole (all crossings
/// in one `Cell`, no component splitting), or `None` if none of its 12 edges
/// cross the opaque/non-opaque boundary. Real meshing goes through
/// `build_cell_group` instead (see below); this whole-cell version is kept
/// `pub(crate)` purely for the hardness-blend mesher's tests, which build a
/// single cell directly to check its blend formula in isolation from the
/// full scan/quad pipeline.
#[cfg(test)]
pub(crate) fn build_cell(grid: &VoxelGrid, materials: &MaterialTable, cell_coord: IVec3) -> Option<Cell> {
    let mut crossings = Vec::new();

    for &(a, b) in &CUBE_EDGES {
        let pos_a = cell_coord + a;
        let pos_b = cell_coord + b;
        let opaque_a = is_opaque(grid, materials, pos_a);
        let opaque_b = is_opaque(grid, materials, pos_b);
        if opaque_a == opaque_b {
            continue;
        }

        let (opaque_pos, other_pos) = if opaque_a { (pos_a, pos_b) } else { (pos_b, pos_a) };
        let weight = grid.get(opaque_pos).weight;
        let position = opaque_pos.as_vec3() + (other_pos - opaque_pos).as_vec3() * (0.5 * weight);
        let axis = axis_of(b - a);
        crossings.push(EdgeCrossing { position, axis });
    }

    if crossings.is_empty() {
        return None;
    }

    let mut weighted_hardness_sum = 0.0;
    let mut weighted_color_sum = (0.0, 0.0, 0.0);
    let mut weight_sum = 0.0;
    for &offset in &CUBE_CORNERS {
        let voxel = grid.get(cell_coord + offset);
        let properties = materials.get(voxel.material);
        if properties.opaque {
            weighted_hardness_sum += voxel.weight * properties.hardness;
            weighted_color_sum.0 += voxel.weight * properties.color.r;
            weighted_color_sum.1 += voxel.weight * properties.color.g;
            weighted_color_sum.2 += voxel.weight * properties.color.b;
            weight_sum += voxel.weight;
        }
    }
    // weight_sum > 0 is guaranteed: `crossings` is non-empty, so at least
    // one corner is opaque.
    let hardness = weighted_hardness_sum / weight_sum;
    let color = Color {
        r: weighted_color_sum.0 / weight_sum,
        g: weighted_color_sum.1 / weight_sum,
        b: weighted_color_sum.2 / weight_sum,
        a: 1.0,
    };

    Some(Cell { cell_coord, crossings, hardness, color })
}

/// The actual generator behind `mesh_with`: like `build_cell`, but splits an
/// ambiguous cell (see `opaque_components`) into one `Cell` per opaque
/// component instead of merging all its crossings into one. Each returned
/// `Cell` is paired with the `CUBE_EDGES`-style corner-offset pairs of the
/// edges it owns, so `get_or_build_vertex` can look up which of a cell's
/// (possibly several) vertices a specific grid edge's crossing belongs to.
pub(crate) fn build_cell_group(grid: &dyn VoxelSource, materials: &MaterialTable, cell_coord: IVec3) -> Vec<(Cell, Vec<(IVec3, IVec3)>)> {
    let mut opaque = [false; 8];
    let mut bridges = [false; 8];
    for &offset in &CUBE_CORNERS {
        let properties = materials.get(grid.get(cell_coord + offset).material);
        opaque[corner_index(offset)] = properties.opaque;
        bridges[corner_index(offset)] = properties.diagonal_bridging;
    }
    let component = opaque_components(&opaque, &bridges);

    struct Group {
        crossings: Vec<EdgeCrossing>,
        local_edges: Vec<(IVec3, IVec3)>,
        weighted_hardness_sum: f32,
        weighted_color_sum: (f32, f32, f32),
        weight_sum: f32,
    }
    let mut groups: Vec<Group> = Vec::new();

    for &(a, b) in &CUBE_EDGES {
        let pos_a = cell_coord + a;
        let pos_b = cell_coord + b;
        let opaque_a = opaque[corner_index(a)];
        let opaque_b = opaque[corner_index(b)];
        if opaque_a == opaque_b {
            continue;
        }

        let (opaque_pos, other_pos, opaque_corner) = if opaque_a { (pos_a, pos_b, a) } else { (pos_b, pos_a, b) };
        let weight = grid.get(opaque_pos).weight;
        let position = opaque_pos.as_vec3() + (other_pos - opaque_pos).as_vec3() * (0.5 * weight);
        let axis = axis_of(b - a);

        let group_id = component[corner_index(opaque_corner)]
            .expect("the opaque endpoint of a sign-changing edge is always itself opaque");
        while groups.len() <= group_id {
            groups.push(Group {
                crossings: Vec::new(),
                local_edges: Vec::new(),
                weighted_hardness_sum: 0.0,
                weighted_color_sum: (0.0, 0.0, 0.0),
                weight_sum: 0.0,
            });
        }
        groups[group_id].crossings.push(EdgeCrossing { position, axis });
        groups[group_id].local_edges.push((a, b));
    }

    // An inactive cell (all corners the same opaqueness — fully solid or
    // fully air) has no sign-changing `CUBE_EDGES` at all, so `groups` stays
    // empty here even though `component` may still mark corners as
    // (single-component) opaque. Every other call site only ever reaches
    // this function via a known sign-changing edge, so this case was
    // previously unreachable; `transition::stitch_face` calls it directly
    // on arbitrary boundary cells, some of which are genuinely inactive.
    if groups.is_empty() {
        return Vec::new();
    }

    // Hardness/colour per component: the weight-averaged hardness/colour of
    // just that component's own opaque corners (mirrors `build_cell`'s
    // whole-cell version, but scoped so a split cell's two vertices don't
    // blend in each other's material — they're meant to represent
    // physically separate surface patches).
    for &offset in &CUBE_CORNERS {
        if let Some(group_id) = component[corner_index(offset)] {
            let voxel = grid.get(cell_coord + offset);
            let properties = materials.get(voxel.material);
            groups[group_id].weighted_hardness_sum += voxel.weight * properties.hardness;
            groups[group_id].weighted_color_sum.0 += voxel.weight * properties.color.r;
            groups[group_id].weighted_color_sum.1 += voxel.weight * properties.color.g;
            groups[group_id].weighted_color_sum.2 += voxel.weight * properties.color.b;
            groups[group_id].weight_sum += voxel.weight;
        }
    }

    groups
        .into_iter()
        .map(|group| {
            let hardness = group.weighted_hardness_sum / group.weight_sum;
            let color = Color {
                r: group.weighted_color_sum.0 / group.weight_sum,
                g: group.weighted_color_sum.1 / group.weight_sum,
                b: group.weighted_color_sum.2 / group.weight_sum,
                a: 1.0,
            };
            (Cell { cell_coord, crossings: group.crossings, hardness, color }, group.local_edges)
        })
        .collect()
}

fn get_or_build_vertex(
    grid: &dyn VoxelSource,
    materials: &MaterialTable,
    cache: &mut HashMap<IVec3, Vec<(Vec<(IVec3, IVec3)>, Vec3, Color)>>,
    place_vertex: &mut impl FnMut(&Cell) -> Vec3,
    cell_coord: IVec3,
    local_edge: (IVec3, IVec3),
) -> (Vec3, Color) {
    let components = cache.entry(cell_coord).or_insert_with(|| {
        build_cell_group(grid, materials, cell_coord)
            .into_iter()
            .map(|(cell, local_edges)| (local_edges, place_vertex(&cell), cell.color))
            .collect()
    });

    components
        .iter()
        .find_map(|(local_edges, position, color)| local_edges.contains(&local_edge).then_some((*position, *color)))
        .expect("a cell adjacent to a sign-changing edge must contain a matching crossing")
}

/// Splits the quad into 2 triangles, skipping either one that's degenerate
/// (two of its three corners at the same position). This can't happen via
/// the normal scan — `build_cell_group` always gives genuinely different
/// cells genuinely different positions — but `mesh_with_overrides`
/// (`core::meshing::transition`) can deliberately collapse several distinct
/// cells onto one shared vertex, and a quad straddling the override
/// boundary can end up with two of its four corners equal as a result. A
/// zero-area triangle there wouldn't be visibly wrong, but it would count
/// as a phantom edge for anything checking manifoldness (e.g.
/// `count_non_manifold_edges`), so it's dropped instead of emitted.
fn push_quad(mesh: &mut MeshData, v0: (Vec3, Color), v1: (Vec3, Color), v2: (Vec3, Color), v3: (Vec3, Color)) {
    if v0.0 != v1.0 && v1.0 != v2.0 && v2.0 != v0.0 {
        let n0 = (v1.0 - v0.0).cross(v2.0 - v0.0).normalize_or_zero();
        mesh.push_triangle([v0.0, v1.0, v2.0], n0, [v0.1, v1.1, v2.1]);
    }
    if v0.0 != v2.0 && v2.0 != v3.0 && v3.0 != v0.0 {
        let n1 = (v2.0 - v0.0).cross(v3.0 - v0.0).normalize_or_zero();
        mesh.push_triangle([v0.0, v2.0, v3.0], n1, [v0.1, v2.1, v3.1]);
    }
}

/// One grid edge, from `lo` to `lo + axis`, that crosses the opaque/
/// non-opaque boundary: builds the quad connecting the 4 Dual-Contouring
/// cells around it, winding it so the normal faces from the opaque side
/// toward the non-opaque side.
fn scan_edge(
    grid: &dyn VoxelSource,
    materials: &MaterialTable,
    cache: &mut HashMap<IVec3, Vec<(Vec<(IVec3, IVec3)>, Vec3, Color)>>,
    place_vertex: &mut impl FnMut(&Cell) -> Vec3,
    mesh: &mut MeshData,
    lo: IVec3,
    axis: Axis,
) {
    let hi = lo + axis.unit();
    let opaque_lo = is_opaque(grid, materials, lo);
    let opaque_hi = is_opaque(grid, materials, hi);
    if opaque_lo == opaque_hi {
        return;
    }

    let (axis1, axis2) = axis.others();
    let p0 = lo - axis1.unit() - axis2.unit();
    let p1 = lo - axis2.unit();
    let p2 = lo;
    let p3 = lo - axis1.unit();

    // `lo`/`hi` are the same absolute grid edge no matter which of the 4
    // surrounding cells is asking — just expressed in that cell's own local
    // corner-offset space (`CUBE_EDGES`' convention: 0 along the edge's own
    // axis, 1 at the far end), which is how `get_or_build_vertex` picks the
    // right one of a split cell's several vertices.
    let local_edge = |p: IVec3| -> (IVec3, IVec3) {
        let a = lo - p;
        (a, a + axis.unit())
    };

    let v0 = get_or_build_vertex(grid, materials, cache, place_vertex, p0, local_edge(p0));
    let v1 = get_or_build_vertex(grid, materials, cache, place_vertex, p1, local_edge(p1));
    let v2 = get_or_build_vertex(grid, materials, cache, place_vertex, p2, local_edge(p2));
    let v3 = get_or_build_vertex(grid, materials, cache, place_vertex, p3, local_edge(p3));

    if opaque_lo {
        // Solid -> non-opaque in the +axis direction: outward normal +axis.
        push_quad(mesh, v0, v1, v2, v3);
    } else {
        // Non-opaque -> solid in the +axis direction: outward normal -axis.
        push_quad(mesh, v3, v2, v1, v0);
    }
}

/// Which of a `mesh_with_boundary` grid's 6 faces should have their
/// outermost cell layer left out of the scan entirely — used by chunk-level
/// LOD boundary stitching (`core::meshing::transition`, `docs/PLAN.md`
/// Roadmap phase C) so a finer chunk's own closing geometry at a face can be
/// replaced by transition geometry instead of overlapping it. Every field
/// `false` (the `Default`) reproduces `mesh_with`'s behaviour exactly.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct BoundarySuppression {
    pub suppress_neg_x: bool,
    pub suppress_pos_x: bool,
    pub suppress_neg_y: bool,
    pub suppress_pos_y: bool,
    pub suppress_neg_z: bool,
    pub suppress_pos_z: bool,
}

/// Scans `grid` for every sign-changing edge and emits a quad around each
/// one. `place_vertex` decides where a given cell's vertex goes — that's
/// the only thing that differs between meshers built on top of this.
pub fn mesh_with(grid: &dyn VoxelSource, materials: &MaterialTable, place_vertex: impl FnMut(&Cell) -> Vec3) -> MeshData {
    mesh_with_boundary(grid, materials, place_vertex, BoundarySuppression::default())
}

/// Like `mesh_with`, but able to leave a face's outermost cell layer
/// unscanned (see `BoundarySuppression`). A cell's own coordinate along an
/// axis is reachable two ways: that axis's own `-1..size` scan (which
/// closes off *its* boundary), and the tangential `0..size` range the other
/// two axes' scans use (since `0..size` already includes `size-1`) — so
/// suppressing a `+` face has to shrink both, or the untouched tangential
/// scans would still rebuild the "suppressed" layer's cells regardless.
/// Reusing the same upper bound (`ehx`/`ehy`/`ehz`) for a face's own scan
/// *and* as the tangential limit in the other two scans is what makes that
/// hold; a `-` face only ever needs its own axis's lower bound moved, since
/// the tangential ranges never reached `-1` to begin with.
pub fn mesh_with_boundary(
    grid: &dyn VoxelSource,
    materials: &MaterialTable,
    place_vertex: impl FnMut(&Cell) -> Vec3,
    suppression: BoundarySuppression,
) -> MeshData {
    mesh_with_overrides(grid, materials, place_vertex, suppression, &[])
}

/// Like `mesh_with_boundary`, but able to force specific cells to resolve
/// to an already-known vertex instead of computing their own — used by
/// chunk-level LOD boundary stitching (`core::meshing::transition`,
/// `docs/PLAN.md` Roadmap phase C) so a chunk's ordinary scan can be told
/// "when you reach this cell, use this vertex" without touching
/// `scan_edge`/`build_cell_group` at all. Each `overrides` entry is
/// pre-seeded into `get_or_build_vertex`'s cache as a single component
/// matching *every* possible local edge (`CUBE_EDGES`), so its
/// `or_insert_with` closure — which would otherwise call
/// `build_cell_group` — never runs for that cell; every local edge that
/// would have reached it resolves straight to the given position/colour
/// instead. Two independent callers (e.g. two neighbouring chunks) that
/// compute the same override position from the same real voxel data will
/// end up sharing an actual vertex, which is what makes the boundary
/// watertight without any new bridging geometry.
pub fn mesh_with_overrides(
    grid: &dyn VoxelSource,
    materials: &MaterialTable,
    mut place_vertex: impl FnMut(&Cell) -> Vec3,
    suppression: BoundarySuppression,
    overrides: &[(IVec3, Vec3, Color)],
) -> MeshData {
    let mut mesh = MeshData::new();
    let mut cache: HashMap<IVec3, Vec<(Vec<(IVec3, IVec3)>, Vec3, Color)>> = HashMap::new();
    for &(cell_coord, position, color) in overrides {
        cache.insert(cell_coord, vec![(CUBE_EDGES.to_vec(), position, color)]);
    }
    let size = grid.size();
    let (sx, sy, sz) = (size.x as i32, size.y as i32, size.z as i32);

    let elx = if suppression.suppress_neg_x { 0 } else { -1 };
    let ehx = if suppression.suppress_pos_x { sx - 1 } else { sx };
    let ely = if suppression.suppress_neg_y { 0 } else { -1 };
    let ehy = if suppression.suppress_pos_y { sy - 1 } else { sy };
    let elz = if suppression.suppress_neg_z { 0 } else { -1 };
    let ehz = if suppression.suppress_pos_z { sz - 1 } else { sz };

    for x in elx..ehx {
        for y in 0..ehy {
            for z in 0..ehz {
                scan_edge(grid, materials, &mut cache, &mut place_vertex, &mut mesh, IVec3::new(x, y, z), Axis::X);
            }
        }
    }
    for y in ely..ehy {
        for x in 0..ehx {
            for z in 0..ehz {
                scan_edge(grid, materials, &mut cache, &mut place_vertex, &mut mesh, IVec3::new(x, y, z), Axis::Y);
            }
        }
    }
    for z in elz..ehz {
        for x in 0..ehx {
            for y in 0..ehy {
                scan_edge(grid, materials, &mut cache, &mut place_vertex, &mut mesh, IVec3::new(x, y, z), Axis::Z);
            }
        }
    }

    mesh
}

/// Test-only watertightness check, shared by every mesher built on
/// `mesh_with` (they all inherit the same scanning/winding logic, so this
/// lives here rather than being duplicated per mesher). Counts mesh edges
/// that aren't properly shared between exactly two triangles with opposite
/// winding: same-direction duplicates (broken winding) and edges with no
/// reverse partner at all (a hole/crack). A fully closed, consistently-wound
/// mesh should score 0 — see the saddle-point discussion in
/// `surface_nets.rs` for a case where it deliberately doesn't.
#[cfg(test)]
pub(crate) fn count_non_manifold_edges(mesh_data: &MeshData) -> usize {
    use std::collections::HashMap;

    let key = |v: Vec3| (v.x.to_bits(), v.y.to_bits(), v.z.to_bits());
    let mut directed_edge_counts: HashMap<((u32, u32, u32), (u32, u32, u32)), u32> = HashMap::new();

    for tri in mesh_data.indices.chunks_exact(3) {
        let p: Vec<Vec3> = tri.iter().map(|&i| mesh_data.positions[i as usize]).collect();
        for (a, b) in [(p[0], p[1]), (p[1], p[2]), (p[2], p[0])] {
            *directed_edge_counts.entry((key(a), key(b))).or_insert(0) += 1;
        }
    }

    directed_edge_counts
        .iter()
        .filter(|&(&(a, b), &count)| {
            count != 1 || directed_edge_counts.get(&(b, a)).copied().unwrap_or(0) != 1
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::voxel::{MaterialId, Voxel};
    use glam::UVec3;

    /// `BoundarySuppression::default()` (every field false) must reproduce
    /// `mesh_with`'s exact output — the regression guard for every existing
    /// call site, none of which pass a suppression at all.
    #[test]
    fn no_suppression_matches_mesh_with_exactly() {
        let mut grid = VoxelGrid::new(UVec3::new(4, 4, 4));
        grid.set(IVec3::new(1, 1, 1), Voxel::new(1.0, MaterialId::DIRT));
        let materials = MaterialTable::standard();
        let place_vertex = |cell: &Cell| -> Vec3 {
            let sum: Vec3 = cell.crossings.iter().map(|c| c.position).sum();
            sum / cell.crossings.len() as f32
        };

        let via_mesh_with = mesh_with(&grid, &materials, place_vertex);
        let via_boundary = mesh_with_boundary(&grid, &materials, place_vertex, BoundarySuppression::default());

        assert_eq!(via_mesh_with.positions.len(), via_boundary.positions.len());
        assert_eq!(via_mesh_with.indices, via_boundary.indices);
    }

    /// Suppressing `+Y` on a flat slab that's solid all the way to the top
    /// of the grid (`y in 0..3` of a 3-tall grid, so the true top surface's
    /// crossing sits exactly at `cell_coord.y = size.y - 1`, the layer
    /// `suppress_pos_y` excludes) should remove that top cap: fewer
    /// triangles than unsuppressed, and no remaining vertex should reach
    /// anywhere near `y=3` — the highest cell left is `cell_coord.y=1`,
    /// which only reaches up to `y=2`.
    #[test]
    fn suppressing_a_face_removes_geometry_reaching_that_boundary() {
        let mut grid = VoxelGrid::new(UVec3::new(6, 3, 6));
        for x in 0..6 {
            for z in 0..6 {
                for y in 0..3 {
                    grid.set(IVec3::new(x, y, z), Voxel::new(1.0, MaterialId::DIRT));
                }
            }
        }
        let materials = MaterialTable::standard();
        let place_vertex = |cell: &Cell| -> Vec3 {
            let sum: Vec3 = cell.crossings.iter().map(|c| c.position).sum();
            sum / cell.crossings.len() as f32
        };

        let unsuppressed = mesh_with(&grid, &materials, place_vertex);
        assert!(unsuppressed.positions.iter().any(|p| p.y > 2.4), "sanity check: the unsuppressed slab has a top cap near y=3");

        let suppression = BoundarySuppression { suppress_pos_y: true, ..Default::default() };
        let suppressed = mesh_with_boundary(&grid, &materials, place_vertex, suppression);

        assert!(suppressed.indices.len() < unsuppressed.indices.len(), "suppression should remove some geometry");
        for &pos in &suppressed.positions {
            assert!(pos.y <= 2.0 + 1e-4, "no vertex should reach the suppressed +Y boundary, got {pos:?}");
        }
    }

    /// An override on one of a lone voxel's 8 surrounding cells should make
    /// every triangle touching that cell use the forced position/colour
    /// instead of its normally-computed one, while every other cell's
    /// geometry stays exactly as it was — the mechanism
    /// `core::meshing::transition` relies on to make two independently
    /// meshed chunks share a real vertex without any new bridging geometry.
    #[test]
    fn an_override_replaces_only_that_cells_geometry() {
        let mut grid = VoxelGrid::new(UVec3::new(3, 3, 3));
        grid.set(IVec3::new(1, 1, 1), Voxel::new(1.0, MaterialId::DIRT));
        let materials = MaterialTable::standard();
        let place_vertex = |cell: &Cell| -> Vec3 {
            let sum: Vec3 = cell.crossings.iter().map(|c| c.position).sum();
            sum / cell.crossings.len() as f32
        };

        let baseline = mesh_with(&grid, &materials, place_vertex);

        let overridden_cell = IVec3::new(1, 1, 1);
        let forced_position = Vec3::new(99.0, 99.0, 99.0);
        let forced_color = Color::rgb(1.0, 0.0, 1.0);
        let overrides = [(overridden_cell, forced_position, forced_color)];
        let overridden = mesh_with_overrides(&grid, &materials, place_vertex, BoundarySuppression::default(), &overrides);

        assert_eq!(overridden.positions.len(), baseline.positions.len(), "overriding a cell shouldn't change triangle count");
        assert!(overridden.positions.contains(&forced_position), "some vertex should now be the forced override position");

        // Every vertex NOT equal to the forced position should exactly
        // match some vertex in the baseline mesh — i.e. every cell other
        // than the overridden one is completely unaffected.
        for &pos in &overridden.positions {
            if pos != forced_position {
                assert!(baseline.positions.contains(&pos), "un-overridden vertex {pos:?} should match the baseline mesh exactly");
            }
        }
    }

    /// A single opaque voxel touches 8 cells (every cell that has it as one
    /// of its 8 corners: `cell_coord` ranges over `{0,1}` in each axis
    /// relative to the voxel), and all 8 are active — each sees exactly 1
    /// opaque corner among 7 air ones, which is already a sign disagreement.
    #[test]
    fn lone_voxel_has_eight_active_cells() {
        let mut grid = VoxelGrid::new(UVec3::new(3, 3, 3));
        grid.set(IVec3::new(1, 1, 1), Voxel::new(1.0, MaterialId::DIRT));
        let materials = MaterialTable::standard();

        let cells = active_cells(&grid, &materials);

        assert_eq!(cells.len(), 8);
        for x in 0..2 {
            for y in 0..2 {
                for z in 0..2 {
                    let cell_coord = IVec3::new(x, y, z);
                    assert!(cells.contains(&cell_coord), "expected {cell_coord:?} to be active");
                }
            }
        }
    }

    /// A single opaque voxel surrounded by air, averaged-vertex-style: every
    /// active cell's vertex should land at the mean of that voxel's 3
    /// visible edge crossings, which for a lone voxel is pulled well inside
    /// the unit cube (this is the "roundedness" the averaging mesher is
    /// meant to produce — see `surface_nets.rs`, which is what actually
    /// exercises `place_vertex`; this test only exercises the shared
    /// scanning/winding logic via the simplest possible placement rule).
    #[test]
    fn lone_voxel_has_six_quads_with_outward_normals() {
        let mut grid = VoxelGrid::new(UVec3::new(3, 3, 3));
        grid.set(IVec3::new(1, 1, 1), Voxel::new(1.0, MaterialId::DIRT));
        let materials = MaterialTable::standard();

        let mesh = mesh_with(&grid, &materials, |cell| {
            let sum: Vec3 = cell.crossings.iter().map(|c| c.position).sum();
            sum / cell.crossings.len() as f32
        });

        // 6 sign-changing edges (one per face of the lone voxel) => 6 quads
        // => 12 triangles => 36 (duplicated, unshared) vertex entries.
        assert_eq!(mesh.indices.len(), 36);
        assert_eq!(mesh.positions.len(), 36);
        assert_eq!(mesh.normals.len(), 36);

        // Every normal should point away from the voxel centre (1,1,1) —
        // i.e. have a positive dot product with (vertex - centre).
        let centre = Vec3::new(1.0, 1.0, 1.0);
        for (pos, normal) in mesh.positions.iter().zip(&mesh.normals) {
            assert!(
                (*pos - centre).dot(*normal) > 0.0,
                "normal {normal:?} at {pos:?} should point away from the solid voxel"
            );
        }
    }

    /// The diagonal-saddle configuration (two opaque voxels touching only
    /// diagonally): for a non-bridging material (dirt), `build_cell_group`
    /// must split the ambiguous cell into 2 separate vertices — that's the
    /// fix this module exists for. For a diagonal-bridging material (wood),
    /// it must come back as a single merged vertex instead, matching the old
    /// pre-fix behaviour that wood's thin branch shapes depend on.
    #[test]
    fn diagonal_bridging_keeps_the_ambiguous_cell_merged_for_wood_but_not_dirt() {
        let materials = MaterialTable::standard();
        let cell_coord = IVec3::new(1, 1, 1);

        let mut dirt_grid = VoxelGrid::new(UVec3::new(4, 3, 4));
        dirt_grid.set(IVec3::new(1, 1, 1), Voxel::new(1.0, MaterialId::DIRT));
        dirt_grid.set(IVec3::new(2, 1, 2), Voxel::new(1.0, MaterialId::DIRT));
        let dirt_groups = build_cell_group(&dirt_grid, &materials, cell_coord);
        assert_eq!(dirt_groups.len(), 2, "dirt's diagonal pair should split into 2 vertices");

        let mut wood_grid = VoxelGrid::new(UVec3::new(4, 3, 4));
        wood_grid.set(IVec3::new(1, 1, 1), Voxel::new(1.0, MaterialId::WOOD));
        wood_grid.set(IVec3::new(2, 1, 2), Voxel::new(1.0, MaterialId::WOOD));
        let wood_groups = build_cell_group(&wood_grid, &materials, cell_coord);
        assert_eq!(wood_groups.len(), 1, "wood's diagonal pair should stay merged into 1 vertex");
        assert_eq!(wood_groups[0].0.crossings.len(), 6, "the merged vertex should see all 6 crossings, like the old whole-cell behaviour");
    }

    /// Two different opaque materials sharing a cell (a real grid edge apart,
    /// so they merge into one component — see `opaque_components`) should
    /// blend into one weight-averaged colour for that cell's single vertex,
    /// the same way `hardness` already blends (see
    /// `hardness_blend_dc::tests::blend_matches_lerp_of_the_two_base_meshers_for_a_single_cell`)
    /// — the mechanism `docs/PLAN.md` Roadmap phase B relies on for a mesh
    /// spanning a material boundary to show the transition per-vertex.
    #[test]
    fn cell_color_is_the_weight_averaged_color_of_its_opaque_corners() {
        let mut grid = VoxelGrid::new(UVec3::new(3, 3, 3));
        grid.set(IVec3::new(1, 1, 1), Voxel::new(1.0, MaterialId::DIRT));
        grid.set(IVec3::new(2, 1, 1), Voxel::new(1.0, MaterialId::BRICK));
        let materials = MaterialTable::standard();

        let cell = build_cell(&grid, &materials, IVec3::new(1, 1, 1)).expect("cell should be active");

        let dirt = materials.get(MaterialId::DIRT).color;
        let brick = materials.get(MaterialId::BRICK).color;
        // Both corners have equal weight (1.0), so this reduces to a plain
        // elementwise average.
        assert!((cell.color.r - (dirt.r + brick.r) / 2.0).abs() < 1e-6);
        assert!((cell.color.g - (dirt.g + brick.g) / 2.0).abs() < 1e-6);
        assert!((cell.color.b - (dirt.b + brick.b) / 2.0).abs() < 1e-6);
    }

    /// The same bridging behaviour, but for a space-diagonal pair (the two
    /// opposite corners of the cell's own cube, touching at only a single
    /// point) rather than a face diagonal — the case a longer wood branch
    /// can also step through.
    #[test]
    fn diagonal_bridging_also_covers_the_space_diagonal_for_wood_but_not_dirt() {
        let materials = MaterialTable::standard();
        let cell_coord = IVec3::new(1, 1, 1);

        let mut dirt_grid = VoxelGrid::new(UVec3::new(3, 3, 3));
        dirt_grid.set(IVec3::new(1, 1, 1), Voxel::new(1.0, MaterialId::DIRT));
        dirt_grid.set(IVec3::new(2, 2, 2), Voxel::new(1.0, MaterialId::DIRT));
        let dirt_groups = build_cell_group(&dirt_grid, &materials, cell_coord);
        assert_eq!(dirt_groups.len(), 2, "dirt's space-diagonal pair should split into 2 vertices");

        let mut wood_grid = VoxelGrid::new(UVec3::new(3, 3, 3));
        wood_grid.set(IVec3::new(1, 1, 1), Voxel::new(1.0, MaterialId::WOOD));
        wood_grid.set(IVec3::new(2, 2, 2), Voxel::new(1.0, MaterialId::WOOD));
        let wood_groups = build_cell_group(&wood_grid, &materials, cell_coord);
        assert_eq!(wood_groups.len(), 1, "wood's space-diagonal pair should stay merged into 1 vertex");
        assert_eq!(wood_groups[0].0.crossings.len(), 6, "the merged vertex should see all 6 crossings, like the old whole-cell behaviour");
    }
}

//! Shared Dual-Contouring cell-scanning and quad-generation logic, used by
//! every mesher in `core::meshing`. What differs between meshers is only
//! *where* each active cell's vertex is placed — see `docs/DESIGN.md`.
//!
//! There's no Hermite (gradient/normal) data to drive a classic DC vertex
//! solve, so instead of scanning every cell up front, this walks every grid
//! edge that crosses the opaque/non-opaque boundary (including one ring of
//! implicit `Air` just outside the grid, so meshes cap off cleanly at its
//! edge) and, for each one, builds a quad from the four Dual-Contouring
//! cells around it — memoizing each cell's vertex so it's computed once no
//! matter how many edges reference it.

use std::collections::HashMap;

use glam::{IVec3, Vec3};

use crate::core::grid::VoxelGrid;
use crate::core::mesh_data::MeshData;
use crate::core::voxel::MaterialTable;

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
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    fn unit(self) -> IVec3 {
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
    fn others(self) -> (Axis, Axis) {
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

fn is_opaque(grid: &VoxelGrid, materials: &MaterialTable, pos: IVec3) -> bool {
    materials.get(grid.get(pos).material).opaque
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

/// Builds the cell at `cell_coord`, or `None` if none of its 12 edges cross
/// the opaque/non-opaque boundary. `pub(crate)` (rather than test-only)
/// since the hardness-blend mesher's tests build individual cells directly
/// to check its blend formula in isolation from the full scan/quad pipeline.
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
    let mut weight_sum = 0.0;
    for &offset in &CUBE_CORNERS {
        let voxel = grid.get(cell_coord + offset);
        let properties = materials.get(voxel.material);
        if properties.opaque {
            weighted_hardness_sum += voxel.weight * properties.hardness;
            weight_sum += voxel.weight;
        }
    }
    // weight_sum > 0 is guaranteed: `crossings` is non-empty, so at least
    // one corner is opaque.
    let hardness = weighted_hardness_sum / weight_sum;

    Some(Cell { cell_coord, crossings, hardness })
}

fn get_or_build_vertex(
    grid: &VoxelGrid,
    materials: &MaterialTable,
    cache: &mut HashMap<IVec3, Vec3>,
    place_vertex: &mut impl FnMut(&Cell) -> Vec3,
    cell_coord: IVec3,
) -> Vec3 {
    if let Some(&position) = cache.get(&cell_coord) {
        return position;
    }
    let cell = build_cell(grid, materials, cell_coord)
        .expect("a cell adjacent to a sign-changing edge must itself be active");
    let position = place_vertex(&cell);
    cache.insert(cell_coord, position);
    position
}

fn push_quad(mesh: &mut MeshData, v0: Vec3, v1: Vec3, v2: Vec3, v3: Vec3) {
    let n0 = (v1 - v0).cross(v2 - v0).normalize_or_zero();
    mesh.push_triangle([v0, v1, v2], n0);
    let n1 = (v2 - v0).cross(v3 - v0).normalize_or_zero();
    mesh.push_triangle([v0, v2, v3], n1);
}

/// One grid edge, from `lo` to `lo + axis`, that crosses the opaque/
/// non-opaque boundary: builds the quad connecting the 4 Dual-Contouring
/// cells around it, winding it so the normal faces from the opaque side
/// toward the non-opaque side.
fn scan_edge(
    grid: &VoxelGrid,
    materials: &MaterialTable,
    cache: &mut HashMap<IVec3, Vec3>,
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

    let v0 = get_or_build_vertex(grid, materials, cache, place_vertex, p0);
    let v1 = get_or_build_vertex(grid, materials, cache, place_vertex, p1);
    let v2 = get_or_build_vertex(grid, materials, cache, place_vertex, p2);
    let v3 = get_or_build_vertex(grid, materials, cache, place_vertex, p3);

    if opaque_lo {
        // Solid -> non-opaque in the +axis direction: outward normal +axis.
        push_quad(mesh, v0, v1, v2, v3);
    } else {
        // Non-opaque -> solid in the +axis direction: outward normal -axis.
        push_quad(mesh, v3, v2, v1, v0);
    }
}

/// Scans `grid` for every sign-changing edge and emits a quad around each
/// one. `place_vertex` decides where a given cell's vertex goes — that's
/// the only thing that differs between meshers built on top of this.
pub fn mesh_with(
    grid: &VoxelGrid,
    materials: &MaterialTable,
    mut place_vertex: impl FnMut(&Cell) -> Vec3,
) -> MeshData {
    let mut mesh = MeshData::new();
    let mut cache: HashMap<IVec3, Vec3> = HashMap::new();
    let size = grid.size();
    let (sx, sy, sz) = (size.x as i32, size.y as i32, size.z as i32);

    for x in -1..sx {
        for y in 0..sy {
            for z in 0..sz {
                scan_edge(grid, materials, &mut cache, &mut place_vertex, &mut mesh, IVec3::new(x, y, z), Axis::X);
            }
        }
    }
    for y in -1..sy {
        for x in 0..sx {
            for z in 0..sz {
                scan_edge(grid, materials, &mut cache, &mut place_vertex, &mut mesh, IVec3::new(x, y, z), Axis::Y);
            }
        }
    }
    for z in -1..sz {
        for x in 0..sx {
            for y in 0..sy {
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
}

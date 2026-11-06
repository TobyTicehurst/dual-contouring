//! Mesher 1: Dual Contouring with an averaged vertex per cell in place of
//! Hermite data — equivalent to Surface Nets. Produces smooth, rounded
//! surfaces; see `docs/DESIGN.md`.

use glam::Vec3;

use crate::core::mesh_data::MeshData;
use crate::core::voxel::MaterialTable;
use crate::core::voxel_source::VoxelSource;

use super::common::{mesh_with, Cell};

pub fn mesh(grid: &dyn VoxelSource, materials: &MaterialTable) -> MeshData {
    mesh_with(grid, materials, place_vertex)
}

/// Exposed (not just used internally) so the hardness-blend mesher can lerp
/// between this and `axis_aligned_dc::place_vertex` for the same cell.
pub fn place_vertex(cell: &Cell) -> Vec3 {
    let sum: Vec3 = cell.crossings.iter().map(|c| c.position).sum();
    sum / cell.crossings.len() as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::demo_scenes;
    use crate::core::grid::VoxelGrid;
    use crate::core::voxel::{MaterialId, Voxel};
    use glam::{IVec3, UVec3};

    use super::super::common::count_non_manifold_edges;

    #[test]
    fn lone_voxel_is_pulled_well_inside_its_unit_cube() {
        // A single full-weight voxel surrounded by air: every active cell
        // sees exactly 3 crossings (the 3 edges from the voxel visible in
        // that cell), all at distance 0.5 from the voxel centre, so the
        // averaged vertex should land noticeably closer to the centre than
        // the cube's own corners/faces do — the "roundedness" this mesher
        // is meant to produce.
        let mut grid = VoxelGrid::new(UVec3::new(3, 3, 3));
        grid.set(IVec3::new(1, 1, 1), Voxel::new(1.0, MaterialId::DIRT));
        let materials = MaterialTable::standard();

        let mesh_data = mesh(&grid, &materials);
        assert!(!mesh_data.positions.is_empty());

        let centre = Vec3::new(1.0, 1.0, 1.0);
        for pos in &mesh_data.positions {
            let distance = (*pos - centre).length();
            // A face crossing sits at distance 0.5 from the centre; a true
            // cube corner would sit at distance 0.866 (sqrt(0.75)). Every
            // averaged vertex should sit strictly inside that.
            assert!(
                distance < 0.75,
                "vertex {pos:?} is distance {distance} from centre — expected it pulled in, not corner-sharp"
            );
        }
    }

    #[test]
    fn rolling_hill_meshes_without_panicking() {
        let grid = demo_scenes::rolling_hill_slab(8, 8, 4);
        let materials = MaterialTable::standard();

        let mesh_data = mesh(&grid, &materials);

        assert!(!mesh_data.positions.is_empty());
        assert_eq!(mesh_data.positions.len(), mesh_data.indices.len());
        assert_eq!(mesh_data.positions.len() % 3, 0);
    }

    /// The grid is padded with implicit `Air` on every side, so a
    /// well-behaved shape's meshed surface should be fully closed with no
    /// boundary edges. A perfectly flat slab has no ambiguous "diagonal"
    /// configurations (see `diagonal_saddle_is_a_known_naive_limitation`
    /// below), so it's a meaningful regression check for the common case.
    #[test]
    fn flat_slab_mesh_is_watertight() {
        let mut grid = VoxelGrid::new(UVec3::new(6, 4, 6));
        for x in 0..6 {
            for z in 0..6 {
                for y in 0..3 {
                    grid.set(IVec3::new(x, y, z), Voxel::new(1.0, MaterialId::DIRT));
                }
            }
        }
        let materials = MaterialTable::standard();
        let mesh_data = mesh(&grid, &materials);

        assert_eq!(count_non_manifold_edges(&mesh_data), 0);
    }

    /// Two opaque voxels touching only diagonally (with the other two
    /// corners of that shared face left as air) is the classic "ambiguous
    /// face" / saddle-point configuration from isosurface extraction. This
    /// *used* to crack under naive Surface Nets: the single Dual-Contouring
    /// cell straddling both voxels put all 6 of their edge crossings through
    /// one shared vertex, forcing two genuinely disjoint surface patches
    /// through a single point. It's now fixed by `common::opaque_components`
    /// (grouping a cell's crossings by which connected component of opaque
    /// corners they belong to, and giving each component its own vertex) —
    /// see `common::build_cell_group`. This test now asserts the fix instead
    /// of documenting the crack.
    #[test]
    fn diagonal_saddle_is_resolved_by_component_splitting() {
        let mut grid = VoxelGrid::new(UVec3::new(4, 3, 4));
        grid.set(IVec3::new(1, 1, 1), Voxel::new(1.0, MaterialId::DIRT));
        grid.set(IVec3::new(2, 1, 2), Voxel::new(1.0, MaterialId::DIRT));
        let materials = MaterialTable::standard();

        let mesh_data = mesh(&grid, &materials);

        assert!(!mesh_data.positions.is_empty());
        assert_eq!(count_non_manifold_edges(&mesh_data), 0);
    }

    /// The mirror image of the case above — a solid 2x2x2 block missing the
    /// same diagonal pair of corners instead of consisting of only them —
    /// stays watertight even without any special handling: unlike the
    /// all-opaque-corners-disjoint case, the *opaque* region here is a
    /// single connected blob (each air corner reaches it directly), so the
    /// true surface really is one connected saddle-shaped patch — which a
    /// single shared vertex can represent validly. Component splitting only
    /// needs to trigger when the *opaque* side is what's disconnected.
    #[test]
    fn mirrored_diagonal_air_case_was_never_actually_ambiguous() {
        let mut grid = VoxelGrid::new(UVec3::new(4, 4, 4));
        for &p in &[
            IVec3::new(2, 1, 1),
            IVec3::new(1, 1, 2),
            IVec3::new(1, 2, 1),
            IVec3::new(2, 2, 1),
            IVec3::new(1, 2, 2),
            IVec3::new(2, 2, 2),
        ] {
            grid.set(p, Voxel::new(1.0, MaterialId::DIRT));
        }
        let materials = MaterialTable::standard();

        let mesh_data = mesh(&grid, &materials);

        assert!(!mesh_data.positions.is_empty());
        assert_eq!(count_non_manifold_edges(&mesh_data), 0);
    }
}

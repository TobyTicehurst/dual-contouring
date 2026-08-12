//! Mesher 3: blends mesher 1's averaged placement and mesher 2's
//! axis-aligned placement by each cell's effective hardness (see
//! `Cell::hardness`) — `hardness = 0.0` reduces to exactly mesher 1's
//! output, `hardness = 1.0` to exactly mesher 2's, matching the materials'
//! own `hardness` property (`0.0` = dirt/smooth, `1.0` = brick/sharp). This
//! is the mesher actually meant for mixed-material terrain. See
//! `docs/DESIGN.md`.

use glam::Vec3;

use crate::core::grid::VoxelGrid;
use crate::core::mesh_data::MeshData;
use crate::core::voxel::MaterialTable;

use super::common::{mesh_with, Cell};
use super::{axis_aligned_dc, surface_nets};

pub fn mesh(grid: &VoxelGrid, materials: &MaterialTable) -> MeshData {
    mesh_with(grid, materials, place_vertex)
}

fn place_vertex(cell: &Cell) -> Vec3 {
    let averaged = surface_nets::place_vertex(cell);
    let axis_aligned = axis_aligned_dc::place_vertex(cell);
    averaged.lerp(axis_aligned, cell.hardness)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::demo_scenes;
    use crate::core::meshing::common::build_cell;
    use crate::core::voxel::{MaterialId, Voxel};
    use glam::{IVec3, UVec3};

    #[test]
    fn blend_matches_lerp_of_the_two_base_meshers_for_a_single_cell() {
        // Wood has hardness 0.5. Rather than hand-computing an expected
        // position, this checks the actual relationship the mesher promises:
        // its output must be the midpoint lerp of the other two meshers' own
        // placement for that same cell.
        let mut grid = VoxelGrid::new(UVec3::new(3, 3, 3));
        grid.set(IVec3::new(1, 1, 1), Voxel::new(1.0, MaterialId::WOOD));
        let materials = MaterialTable::standard();

        let cell = build_cell(&grid, &materials, IVec3::new(1, 1, 1)).expect("cell should be active");
        assert!((cell.hardness - 0.5).abs() < 1e-6);

        let averaged = surface_nets::place_vertex(&cell);
        let axis_aligned = axis_aligned_dc::place_vertex(&cell);
        let blended = place_vertex(&cell);

        assert!(
            blended.abs_diff_eq(averaged.lerp(axis_aligned, 0.5), 1e-6),
            "blended {blended:?} should be the midpoint of averaged {averaged:?} and axis-aligned {axis_aligned:?}"
        );
    }

    #[test]
    fn uniform_dirt_matches_surface_nets_exactly() {
        // Dirt has hardness 0.0, so the blend should reduce to exactly
        // mesher 1's output — not just "close", the plan calls for exact.
        let grid = demo_scenes::rolling_hill_slab(8, 8, 4);
        let materials = MaterialTable::standard();

        let blended = mesh(&grid, &materials);
        let averaged = surface_nets::mesh(&grid, &materials);

        assert_eq!(blended.positions, averaged.positions);
    }

    #[test]
    fn uniform_brick_matches_axis_aligned_exactly() {
        // Brick has hardness 1.0, so the blend should reduce to exactly
        // mesher 2's output.
        let grid = demo_scenes::brick_wall_with_window(9, 9, 1);
        let materials = MaterialTable::standard();

        let blended = mesh(&grid, &materials);
        let axis_aligned = axis_aligned_dc::mesh(&grid, &materials);

        assert_eq!(blended.positions, axis_aligned.positions);
    }

    #[test]
    fn mixed_hardness_block_meshes_without_panicking() {
        let grid = demo_scenes::mixed_hardness_block(8, 4, 4);
        let materials = MaterialTable::standard();

        let mesh_data = mesh(&grid, &materials);

        assert!(!mesh_data.positions.is_empty());
        assert_eq!(mesh_data.positions.len(), mesh_data.indices.len());
    }
}

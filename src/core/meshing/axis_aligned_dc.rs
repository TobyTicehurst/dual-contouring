//! Mesher 2: Dual Contouring with axis-aligned normals in place of Hermite
//! data. Each edge crossing is assigned the cardinal normal of the edge
//! it's on (the one direction we know "for free" from the grid), which
//! decouples the QEF solve into an independent mean per axis — no general
//! 3×3 solve needed. Produces flat, right-angled surfaces; good for brick,
//! walls, cliffs. See `docs/DESIGN.md`.

use glam::Vec3;

use crate::core::mesh_data::MeshData;
use crate::core::voxel::MaterialTable;
use crate::core::voxel_source::VoxelSource;

use super::common::{mesh_with, Axis, Cell};

pub fn mesh(grid: &dyn VoxelSource, materials: &MaterialTable) -> MeshData {
    mesh_with(grid, materials, place_vertex)
}

/// Exposed (not just used internally) so the hardness-blend mesher can lerp
/// between this and `surface_nets::place_vertex` for the same cell.
pub fn place_vertex(cell: &Cell) -> Vec3 {
    // Cell-centre fallback for any axis with no crossings on it (common —
    // e.g. the interior of a flat floor has no X/Z crossings, only Y). This
    // is correct by construction, not just a stopgap: it's exactly what
    // produces a clean flat, grid-aligned plane in that case.
    let mut result = cell.cell_coord.as_vec3() + Vec3::splat(0.5);

    for axis in [Axis::X, Axis::Y, Axis::Z] {
        let (sum, count) = cell
            .crossings
            .iter()
            .filter(|c| c.axis == axis)
            .fold((0.0, 0u32), |(sum, count), c| (sum + component(c.position, axis), count + 1));
        if count > 0 {
            set_component(&mut result, axis, sum / count as f32);
        }
    }

    result
}

fn component(v: Vec3, axis: Axis) -> f32 {
    match axis {
        Axis::X => v.x,
        Axis::Y => v.y,
        Axis::Z => v.z,
    }
}

fn set_component(v: &mut Vec3, axis: Axis, value: f32) {
    match axis {
        Axis::X => v.x = value,
        Axis::Y => v.y = value,
        Axis::Z => v.z = value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::demo_scenes;
    use crate::core::grid::VoxelGrid;
    use crate::core::meshing::common::EdgeCrossing;
    use crate::core::meshing::common::count_non_manifold_edges;
    use crate::core::voxel::{MaterialId, Voxel};
    use glam::{IVec3, UVec3};

    #[test]
    fn lone_voxel_produces_a_sharp_unit_cube() {
        // Unlike mesher 1 (which pulls vertices well inside the cube), every
        // axis-aligned vertex around a lone full-weight voxel should land
        // exactly on one of the unit cube's own 8 corners: (0.5 or 1.5) in
        // each of X, Y, Z. Sharp, not rounded.
        let mut grid = VoxelGrid::new(UVec3::new(3, 3, 3));
        grid.set(IVec3::new(1, 1, 1), Voxel::new(1.0, MaterialId::DIRT));
        let materials = MaterialTable::standard();

        let mesh_data = mesh(&grid, &materials);
        assert!(!mesh_data.positions.is_empty());

        for pos in &mesh_data.positions {
            for &c in &[pos.x, pos.y, pos.z] {
                assert!(
                    (c - 0.5).abs() < 1e-5 || (c - 1.5).abs() < 1e-5,
                    "coordinate {c} in {pos:?} is not exactly 0.5 or 1.5 — expected a sharp cube corner"
                );
            }
        }
    }

    #[test]
    fn axis_with_no_crossings_falls_back_to_cell_centre() {
        // Hand-built cell: crossings only on Y (as in the interior of a flat
        // floor), so X and Z must fall back to the cell's own centre.
        let cell = Cell {
            cell_coord: IVec3::new(2, 0, 2),
            crossings: vec![
                EdgeCrossing { position: Vec3::new(2.0, 0.5, 2.0), axis: Axis::Y },
                EdgeCrossing { position: Vec3::new(3.0, 0.5, 2.0), axis: Axis::Y },
                EdgeCrossing { position: Vec3::new(2.0, 0.5, 3.0), axis: Axis::Y },
                EdgeCrossing { position: Vec3::new(3.0, 0.5, 3.0), axis: Axis::Y },
            ],
            hardness: 0.0, // unused by this mesher; arbitrary placeholder
            color: crate::core::voxel::Color::rgb(0.0, 0.0, 0.0), // unused by this mesher; arbitrary placeholder
        };

        let vertex = place_vertex(&cell);
        assert!((vertex.x - 2.5).abs() < 1e-6, "X should fall back to the cell centre: {vertex:?}");
        assert!((vertex.y - 0.5).abs() < 1e-6, "Y should come from the actual crossings: {vertex:?}");
        assert!((vertex.z - 2.5).abs() < 1e-6, "Z should fall back to the cell centre: {vertex:?}");
    }

    #[test]
    fn brick_wall_meshes_without_panicking() {
        let grid = demo_scenes::brick_wall_with_window(9, 9, 1);
        let materials = MaterialTable::standard();

        let mesh_data = mesh(&grid, &materials);

        assert!(!mesh_data.positions.is_empty());
        assert_eq!(mesh_data.positions.len(), mesh_data.indices.len());
    }

    #[test]
    fn brick_wall_mesh_is_watertight() {
        // A rectangular window cut from a rectangle has no diagonal-only
        // (checkerboard) corners, so unlike the rolling hill's saddles, this
        // shape has no ambiguous configurations at all.
        let grid = demo_scenes::brick_wall_with_window(9, 9, 1);
        let materials = MaterialTable::standard();

        let mesh_data = mesh(&grid, &materials);

        assert_eq!(count_non_manifold_edges(&mesh_data), 0);
    }
}

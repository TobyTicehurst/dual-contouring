//! Dense voxel grid storage. No chunking, no LOD, no sparse storage — a
//! deliberate simplification for now, not an oversight (see `docs/PLAN.md`
//! milestone 4).

use glam::{IVec3, UVec3};

use super::voxel::Voxel;

/// A dense, fixed-size 3D grid of voxels, indexed by local grid coordinates
/// `0..size` in each axis. World placement (if any) is the caller's concern
/// — this type only knows about its own local coordinate space.
#[derive(Debug, Clone)]
pub struct VoxelGrid {
    size: UVec3,
    voxels: Vec<Voxel>,
}

impl VoxelGrid {
    /// A new grid of the given dimensions, filled entirely with `Voxel::AIR`.
    pub fn new(size: UVec3) -> Self {
        let count = size.x as usize * size.y as usize * size.z as usize;
        Self {
            size,
            voxels: vec![Voxel::AIR; count],
        }
    }

    pub fn size(&self) -> UVec3 {
        self.size
    }

    /// Reads the voxel at `pos`. Any position outside the grid — including
    /// negative coordinates — reads as `Voxel::AIR`, which is what lets every
    /// mesher probe one ring past the grid's used region without special-
    /// casing edges (see `docs/DESIGN.md`).
    pub fn get(&self, pos: IVec3) -> Voxel {
        self.index_of(pos).map_or(Voxel::AIR, |i| self.voxels[i])
    }

    /// Writes the voxel at `pos`. Unlike `get`, `pos` must be in bounds —
    /// writing out of bounds is an authoring bug, not a routine event, so
    /// this panics rather than silently doing nothing.
    pub fn set(&mut self, pos: IVec3, voxel: Voxel) {
        let index = self
            .index_of(pos)
            .unwrap_or_else(|| panic!("VoxelGrid::set: {pos:?} is out of bounds ({:?})", self.size));
        self.voxels[index] = voxel;
    }

    fn index_of(&self, pos: IVec3) -> Option<usize> {
        if pos.x < 0 || pos.y < 0 || pos.z < 0 {
            return None;
        }
        let (x, y, z) = (pos.x as u32, pos.y as u32, pos.z as u32);
        if x >= self.size.x || y >= self.size.y || z >= self.size.z {
            return None;
        }
        Some(((z * self.size.y + y) * self.size.x + x) as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::voxel::MaterialId;

    #[test]
    fn new_grid_is_all_air() {
        let grid = VoxelGrid::new(UVec3::new(2, 2, 2));
        assert_eq!(grid.get(IVec3::ZERO).material, MaterialId::AIR);
    }

    #[test]
    fn out_of_bounds_reads_are_air() {
        let grid = VoxelGrid::new(UVec3::new(2, 2, 2));
        assert_eq!(grid.get(IVec3::new(-1, 0, 0)).material, MaterialId::AIR);
        assert_eq!(grid.get(IVec3::new(2, 0, 0)).material, MaterialId::AIR);
        assert_eq!(grid.get(IVec3::new(0, 0, 100)).material, MaterialId::AIR);
    }

    #[test]
    fn set_then_get_roundtrips() {
        let mut grid = VoxelGrid::new(UVec3::new(2, 2, 2));
        let voxel = Voxel::new(0.75, MaterialId::DIRT);
        grid.set(IVec3::new(1, 0, 1), voxel);
        assert_eq!(grid.get(IVec3::new(1, 0, 1)), voxel);
        // Neighbouring cells are untouched.
        assert_eq!(grid.get(IVec3::new(0, 0, 1)).material, MaterialId::AIR);
    }

    #[test]
    #[should_panic]
    fn set_out_of_bounds_panics() {
        let mut grid = VoxelGrid::new(UVec3::new(2, 2, 2));
        grid.set(IVec3::new(5, 0, 0), Voxel::AIR);
    }
}

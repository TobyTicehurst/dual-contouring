//! Abstraction over "anything meshing can read voxels from" — implemented
//! by both a plain `VoxelGrid` and `chunk::ChunkView` (a lazy, chunk-plus-
//! neighbours view into a `VoxelWorld`), so `meshing::common`'s shared DC
//! scanning logic works unchanged for both a single dense grid and one
//! chunk of a chunked world. Teaching every mesher itself about chunk
//! neighbours would mean touching each one; instead the *boundary lookup*
//! is abstracted away here, and every mesher stays exactly as ignorant of
//! chunking as it was before (see `docs/PLAN.md` Roadmap phase A).

use glam::{IVec3, UVec3};

use super::voxel::Voxel;

pub trait VoxelSource {
    /// Reads the voxel at `pos`. Implementations decide what happens
    /// outside their own logical bounds — a plain `VoxelGrid` reads as
    /// `Air`; a `ChunkView` reads whichever real neighbouring chunk owns
    /// that position instead (`Air` too, if none exists there).
    fn get(&self, pos: IVec3) -> Voxel;

    /// The logical size meshing should scan — not necessarily the full
    /// extent of what `get` can answer. A `ChunkView` can answer lookups far
    /// outside its own chunk, but `mesh_with` only ever needs to reach the
    /// single ring of positions just past this boundary.
    fn size(&self) -> UVec3;
}

/// A coarser view of another `VoxelSource`, sampling every `stride`-th voxel
/// instead of every one — the mechanism behind chunk-level LOD (see
/// `docs/PLAN.md` Roadmap phase C). Wraps any `&dyn VoxelSource` (in
/// practice, a chunk's `ChunkView`), so meshing a downsampled chunk still
/// gets real neighbouring-chunk data at its boundary for free: the one ring
/// `mesh_with` queries just past this view's own (smaller) `size()` is still
/// answered by the wrapped source, just at `stride` spacing like everything
/// else.
///
/// A mesh built from this view comes out in downsampled-local space — vertex
/// `n` along an axis corresponds to real position `n * stride` in the
/// wrapped source — so the caller must scale the resulting mesh's transform
/// by `stride` to place it correctly (see `render::chunk_mesh_view`).
pub struct DownsampledView<'a> {
    source: &'a dyn VoxelSource,
    stride: u32,
}

impl<'a> DownsampledView<'a> {
    /// `stride` must evenly divide `source.size()` in every axis (checked in
    /// debug builds only) — true for any power-of-two stride against the
    /// default 32³ chunk size, which is the only way this is used today.
    pub fn new(source: &'a dyn VoxelSource, stride: u32) -> Self {
        debug_assert!(stride > 0, "DownsampledView: stride must be positive");
        let size = source.size();
        debug_assert!(
            size.x % stride == 0 && size.y % stride == 0 && size.z % stride == 0,
            "DownsampledView: stride {stride} must evenly divide source size {size:?}"
        );
        Self { source, stride }
    }
}

impl VoxelSource for DownsampledView<'_> {
    fn get(&self, pos: IVec3) -> Voxel {
        self.source.get(pos * self.stride as i32)
    }

    fn size(&self) -> UVec3 {
        self.source.size() / self.stride
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::grid::VoxelGrid;
    use crate::core::meshing::surface_nets;
    use crate::core::voxel::{MaterialId, MaterialTable, Voxel};

    /// Stride 1 must behave exactly like the wrapped source — no change in
    /// behaviour for chunks close enough to pick full resolution.
    #[test]
    fn stride_one_is_a_no_op() {
        let mut grid = VoxelGrid::new(UVec3::new(4, 4, 4));
        grid.set(IVec3::new(2, 1, 3), Voxel::new(1.0, MaterialId::DIRT));
        let view = DownsampledView::new(&grid, 1);

        assert_eq!(view.size(), grid.size());
        for pos in [IVec3::new(0, 0, 0), IVec3::new(2, 1, 3), IVec3::new(-1, 0, 0)] {
            assert_eq!(view.get(pos), grid.get(pos));
        }
    }

    /// At stride 2, downsampled position `n` should read the wrapped
    /// source's real position `2n`, not `n`.
    #[test]
    fn stride_two_samples_every_other_voxel() {
        let mut grid = VoxelGrid::new(UVec3::new(4, 4, 4));
        grid.set(IVec3::new(2, 0, 0), Voxel::new(1.0, MaterialId::DIRT));
        let view = DownsampledView::new(&grid, 2);

        assert_eq!(view.size(), UVec3::new(2, 2, 2));
        // Downsampled (1,0,0) -> real (2,0,0), the voxel that was set.
        assert_eq!(view.get(IVec3::new(1, 0, 0)).material, MaterialId::DIRT);
        // Downsampled (0,0,0) -> real (0,0,0), untouched.
        assert_eq!(view.get(IVec3::new(0, 0, 0)).material, MaterialId::AIR);
    }

    /// The actual proof this coarsens geometry, not just samples differently
    /// in isolation: meshing the same shape through a stride-2 view should
    /// produce a noticeably smaller mesh than meshing it at full resolution.
    #[test]
    fn downsampled_mesh_has_fewer_triangles_than_full_resolution() {
        let mut grid = VoxelGrid::new(UVec3::new(8, 8, 8));
        for x in 0..8 {
            for z in 0..8 {
                let height = 2 + ((x + z) % 3);
                for y in 0..height {
                    grid.set(IVec3::new(x, y as i32, z), Voxel::new(1.0, MaterialId::DIRT));
                }
            }
        }
        let materials = MaterialTable::standard();

        let full_res = surface_nets::mesh(&grid, &materials);
        let downsampled_view = DownsampledView::new(&grid, 2);
        let downsampled = surface_nets::mesh(&downsampled_view, &materials);

        assert!(
            downsampled.indices.len() < full_res.indices.len(),
            "downsampled mesh ({} indices) should have fewer triangles than full resolution ({} indices)",
            downsampled.indices.len(),
            full_res.indices.len()
        );
    }
}

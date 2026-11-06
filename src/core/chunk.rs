//! Chunked voxel storage: splits an otherwise-unbounded voxel world into
//! fixed-size `VoxelGrid`s, keyed by chunk coordinate. See `docs/PLAN.md`
//! Roadmap phase A for the motivation — this is what LOD, meaningful
//! performance testing, and later remeshing/streaming all build on.

use std::collections::HashMap;

use glam::{IVec3, UVec3};

use super::grid::VoxelGrid;
use super::voxel::{MaterialId, Voxel};
use super::voxel_source::VoxelSource;

/// The chunk size used unless a `VoxelWorld` is built with a different one.
/// 32³ = 32,768 voxels/chunk — see `docs/PLAN.md` for the reasoning. Treated
/// as tunable, not fixed: `VoxelWorld::chunk_size` is a field, not this
/// const, so a world is free to pick its own.
pub const DEFAULT_CHUNK_SIZE: u32 = 32;

/// Identifies a chunk within a `VoxelWorld` — the voxel-space origin of
/// chunk `c` is `c.0 * chunk_size`. Used as the `VoxelWorld` storage key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChunkCoord(pub IVec3);

/// A sparse, effectively unbounded voxel world: a `HashMap` from chunk
/// coordinate to that chunk's dense `VoxelGrid`, so only chunks that
/// actually have content need to exist at all. World voxel coordinates map
/// to a chunk coordinate and a local (`0..chunk_size`) coordinate within it
/// — see `chunk_coord_for`/`local_coord_for`.
pub struct VoxelWorld {
    chunk_size: u32,
    chunks: HashMap<ChunkCoord, VoxelGrid>,
}

impl VoxelWorld {
    pub fn new(chunk_size: u32) -> Self {
        Self { chunk_size, chunks: HashMap::new() }
    }

    pub fn chunk_size(&self) -> u32 {
        self.chunk_size
    }

    /// Which chunk a world voxel coordinate falls in. Uses `div_euclid` (not
    /// plain integer division) so negative world coordinates map to the
    /// correct chunk instead of rounding toward zero.
    pub fn chunk_coord_for(&self, world_pos: IVec3) -> ChunkCoord {
        let cs = self.chunk_size as i32;
        ChunkCoord(IVec3::new(world_pos.x.div_euclid(cs), world_pos.y.div_euclid(cs), world_pos.z.div_euclid(cs)))
    }

    /// A world voxel coordinate's position within its own chunk, always in
    /// `0..chunk_size` regardless of sign (see `chunk_coord_for`).
    pub fn local_coord_for(&self, world_pos: IVec3) -> IVec3 {
        let cs = self.chunk_size as i32;
        IVec3::new(world_pos.x.rem_euclid(cs), world_pos.y.rem_euclid(cs), world_pos.z.rem_euclid(cs))
    }

    /// The world-space voxel coordinate of a chunk's own local `(0,0,0)`.
    pub fn chunk_origin(&self, coord: ChunkCoord) -> IVec3 {
        coord.0 * self.chunk_size as i32
    }

    /// Reads the voxel at a world coordinate. A chunk that doesn't exist
    /// (never written to) reads as entirely `Air` — the sparse-storage
    /// equivalent of `VoxelGrid::get`'s out-of-bounds-is-`Air` rule.
    pub fn get(&self, world_pos: IVec3) -> Voxel {
        let coord = self.chunk_coord_for(world_pos);
        match self.chunks.get(&coord) {
            Some(grid) => grid.get(self.local_coord_for(world_pos)),
            None => Voxel::AIR,
        }
    }

    /// Writes the voxel at a world coordinate, lazily creating (as all-`Air`)
    /// whichever chunk it falls in if this is the first write to it.
    pub fn set(&mut self, world_pos: IVec3, voxel: Voxel) {
        let coord = self.chunk_coord_for(world_pos);
        let local = self.local_coord_for(world_pos);
        let chunk_size = self.chunk_size;
        let grid = self.chunks.entry(coord).or_insert_with(|| VoxelGrid::new(UVec3::splat(chunk_size)));
        grid.set(local, voxel);
    }

    pub fn chunk(&self, coord: ChunkCoord) -> Option<&VoxelGrid> {
        self.chunks.get(&coord)
    }

    /// Every chunk that currently exists (has had at least one non-air voxel
    /// written to it, directly or via `from_dense`).
    pub fn chunks(&self) -> impl Iterator<Item = (ChunkCoord, &VoxelGrid)> {
        self.chunks.iter().map(|(&coord, grid)| (coord, grid))
    }

    /// A lazy meshing view of chunk `coord`: reads like the chunk's own
    /// `VoxelGrid` for local positions in `0..chunk_size`, but — unlike a
    /// plain `VoxelGrid` — also answers the single ring of positions just
    /// past that boundary (all `mesh_with` ever needs beyond a grid's own
    /// size) by looking up the real neighbouring chunk in this world,
    /// instead of assuming `Air`. No copying: every mesher already only
    /// consumes a `&dyn VoxelSource`, so this needs no mesher-side changes
    /// (see `voxel_source::VoxelSource`).
    pub fn view(&self, coord: ChunkCoord) -> ChunkView<'_> {
        ChunkView { world: self, origin: self.chunk_origin(coord), chunk_size: self.chunk_size }
    }

    /// Splits an existing dense `VoxelGrid` (e.g. a `demo_scenes` generator's
    /// output) into a `VoxelWorld`, letting single-grid content be reused to
    /// build multi-chunk test scenes. All-air regions don't create a chunk.
    pub fn from_dense(grid: &VoxelGrid, chunk_size: u32) -> Self {
        let mut world = Self::new(chunk_size);
        let size = grid.size();
        for x in 0..size.x {
            for y in 0..size.y {
                for z in 0..size.z {
                    let pos = IVec3::new(x as i32, y as i32, z as i32);
                    let voxel = grid.get(pos);
                    if voxel.material != MaterialId::AIR {
                        world.set(pos, voxel);
                    }
                }
            }
        }
        world
    }
}

/// The 6 face-adjacent neighbour directions of a chunk — used by
/// `balance_lod_levels` (and, for meshing, `render::chunk_mesh_view`'s
/// boundary-stitching lookup).
pub const FACE_NEIGHBOR_OFFSETS: [IVec3; 6] = [
    IVec3::new(1, 0, 0),
    IVec3::new(-1, 0, 0),
    IVec3::new(0, 1, 0),
    IVec3::new(0, -1, 0),
    IVec3::new(0, 0, 1),
    IVec3::new(0, 0, -1),
];

/// "Restricted octree" LOD balancing: adjusts `raw_levels` (a chunk's own
/// distance-based LOD level, 0 = full resolution, higher = coarser — see
/// `render::chunk_mesh_view::lod_stride_for_distance`) so that (a) no two
/// face-adjacent *existing* chunks ever differ by more than one level, and
/// (b) no chunk has more than one face-adjacent neighbour that's exactly
/// one level coarser than it. Together these guarantee every chunk needs
/// at most one transitioning face, each an exact 2:1 gap — the only shape
/// `core::meshing::transition`'s merged transition-cell handles — see
/// `docs/PLAN.md` Roadmap phase C.
///
/// Both rules only ever *lower* some chunk's level (never raise one) so
/// they share a single monotonically-decreasing potential, bounded below
/// by 0 — the same termination argument as a plain single-rule relaxation.
/// This matters: an earlier draft of rule (b) raised the chunk with too
/// many coarser neighbours instead, which could undo rule (a)'s own most
/// recent clamp on that same chunk and oscillate forever (worked example
/// in `docs/PLAN.md`). Lowering the *extra* coarser neighbours down to
/// match, instead of raising the chunk itself, avoids that entirely.
pub fn balance_lod_levels(world: &VoxelWorld, raw_levels: &HashMap<ChunkCoord, u32>) -> HashMap<ChunkCoord, u32> {
    let mut levels = raw_levels.clone();

    let mut changed = true;
    while changed {
        changed = false;
        for (coord, _) in world.chunks() {
            let Some(&level) = levels.get(&coord) else { continue };

            // (a) Don't let this chunk be more than one level coarser than
            // its finest existing neighbour.
            let min_neighbor_level =
                FACE_NEIGHBOR_OFFSETS.iter().filter_map(|&offset| levels.get(&ChunkCoord(coord.0 + offset)).copied()).min();
            if let Some(min_neighbor_level) = min_neighbor_level {
                let max_allowed = min_neighbor_level + 1;
                if level > max_allowed {
                    levels.insert(coord, max_allowed);
                    changed = true;
                    continue;
                }
            }

            // (b) Don't let this chunk have more than one neighbour that's
            // exactly one level coarser than it (i.e. more than one
            // transitioning face) — pull every extra one down to match,
            // keeping only the first (in a fixed, deterministic order).
            let mut kept_one = false;
            for &offset in &FACE_NEIGHBOR_OFFSETS {
                let neighbor = ChunkCoord(coord.0 + offset);
                let Some(&neighbor_level) = levels.get(&neighbor) else { continue };
                if neighbor_level == level + 1 {
                    if !kept_one {
                        kept_one = true;
                    } else {
                        levels.insert(neighbor, level);
                        changed = true;
                    }
                }
            }
        }
    }

    levels
}

/// A `VoxelSource` view of one chunk of a `VoxelWorld` — see
/// `VoxelWorld::view`. Borrows the world rather than copying anything, so
/// this is cheap to build fresh per mesh.
pub struct ChunkView<'a> {
    world: &'a VoxelWorld,
    origin: IVec3,
    chunk_size: u32,
}

impl VoxelSource for ChunkView<'_> {
    fn get(&self, pos: IVec3) -> Voxel {
        self.world.get(self.origin + pos)
    }

    fn size(&self) -> UVec3 {
        UVec3::splat(self.chunk_size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::meshing::common::count_non_manifold_edges;
    use crate::core::meshing::surface_nets;
    use crate::core::voxel::MaterialTable;

    #[test]
    fn chunk_and_local_coords_round_trip_for_positive_and_negative_positions() {
        let world = VoxelWorld::new(4);

        assert_eq!(world.chunk_coord_for(IVec3::new(0, 0, 0)), ChunkCoord(IVec3::new(0, 0, 0)));
        assert_eq!(world.local_coord_for(IVec3::new(0, 0, 0)), IVec3::new(0, 0, 0));

        assert_eq!(world.chunk_coord_for(IVec3::new(5, 3, 7)), ChunkCoord(IVec3::new(1, 0, 1)));
        assert_eq!(world.local_coord_for(IVec3::new(5, 3, 7)), IVec3::new(1, 3, 3));

        // Negative coordinates must floor toward -infinity, not toward zero:
        // -1 is the last voxel of chunk -1, not local coordinate -1 in chunk 0.
        assert_eq!(world.chunk_coord_for(IVec3::new(-1, 0, 0)), ChunkCoord(IVec3::new(-1, 0, 0)));
        assert_eq!(world.local_coord_for(IVec3::new(-1, 0, 0)), IVec3::new(3, 0, 0));
        assert_eq!(world.chunk_coord_for(IVec3::new(-4, 0, 0)), ChunkCoord(IVec3::new(-1, 0, 0)));
        assert_eq!(world.chunk_coord_for(IVec3::new(-5, 0, 0)), ChunkCoord(IVec3::new(-2, 0, 0)));
    }

    #[test]
    fn chunk_origin_is_chunk_coord_times_chunk_size() {
        let world = VoxelWorld::new(4);
        assert_eq!(world.chunk_origin(ChunkCoord(IVec3::new(2, -1, 0))), IVec3::new(8, -4, 0));
    }

    #[test]
    fn get_on_a_never_written_chunk_reads_as_air() {
        let world = VoxelWorld::new(4);
        assert_eq!(world.get(IVec3::new(100, 100, 100)).material, MaterialId::AIR);
    }

    #[test]
    fn set_then_get_roundtrips_across_a_chunk_boundary() {
        let mut world = VoxelWorld::new(4);
        let voxel = Voxel::new(0.6, MaterialId::DIRT);
        // chunk size 4: (4,0,0) is local (0,0,0) of the neighbouring chunk,
        // not local (4,0,0) of chunk (0,0,0).
        world.set(IVec3::new(4, 0, 0), voxel);

        assert_eq!(world.get(IVec3::new(4, 0, 0)), voxel);
        assert_eq!(world.get(IVec3::new(3, 0, 0)).material, MaterialId::AIR);

        let neighbour_chunk = world.chunk(ChunkCoord(IVec3::new(1, 0, 0))).expect("chunk should have been created");
        assert_eq!(neighbour_chunk.get(IVec3::new(0, 0, 0)), voxel);
    }

    #[test]
    fn from_dense_splits_voxels_into_the_right_chunks() {
        let mut grid = VoxelGrid::new(UVec3::new(6, 1, 1));
        grid.set(IVec3::new(1, 0, 0), Voxel::new(1.0, MaterialId::DIRT));
        grid.set(IVec3::new(5, 0, 0), Voxel::new(1.0, MaterialId::BRICK));

        let world = VoxelWorld::from_dense(&grid, 4);

        assert_eq!(world.get(IVec3::new(1, 0, 0)).material, MaterialId::DIRT);
        assert_eq!(world.get(IVec3::new(5, 0, 0)).material, MaterialId::BRICK);
        assert!(world.chunk(ChunkCoord(IVec3::new(0, 0, 0))).is_some());
        assert!(world.chunk(ChunkCoord(IVec3::new(1, 0, 0))).is_some());
        // No content anywhere else, so no other chunks should exist.
        assert_eq!(world.chunks().count(), 2);
    }

    #[test]
    fn chunk_view_reads_a_neighbouring_chunks_voxel_across_the_boundary() {
        let mut world = VoxelWorld::new(4);
        // Just across the +X boundary of chunk (0,0,0) — local (4,1,1) is
        // one step past that chunk's own 0..4 range, exactly the ring
        // `mesh_with` queries just past a grid's own size.
        world.set(IVec3::new(4, 1, 1), Voxel::new(1.0, MaterialId::DIRT));

        let view = world.view(ChunkCoord(IVec3::new(0, 0, 0)));

        assert_eq!(view.get(IVec3::new(4, 1, 1)).material, MaterialId::DIRT);
        // And the view still reads Air at its own untouched local cells.
        assert_eq!(view.get(IVec3::new(1, 1, 1)).material, MaterialId::AIR);
    }

    /// A 3-chunk row where the middle chunk's raw (distance-based) level
    /// wants to be much coarser than its immediate neighbours: balancing
    /// must clamp it to at most one level coarser than its tightest
    /// neighbour constraint (both neighbours are level 0, so the middle
    /// chunk can be at most level 1 — not the raw 3 it started with).
    #[test]
    fn balance_lod_levels_clamps_an_isolated_coarse_chunk_between_fine_neighbours() {
        let mut world = VoxelWorld::new(4);
        for coord in [ChunkCoord(IVec3::new(-1, 0, 0)), ChunkCoord(IVec3::new(0, 0, 0)), ChunkCoord(IVec3::new(1, 0, 0))] {
            world.set(world.chunk_origin(coord), Voxel::new(1.0, MaterialId::DIRT));
        }
        let raw_levels = HashMap::from([
            (ChunkCoord(IVec3::new(-1, 0, 0)), 0),
            (ChunkCoord(IVec3::new(0, 0, 0)), 3),
            (ChunkCoord(IVec3::new(1, 0, 0)), 0),
        ]);

        let balanced = balance_lod_levels(&world, &raw_levels);

        assert_eq!(balanced[&ChunkCoord(IVec3::new(-1, 0, 0))], 0);
        assert_eq!(balanced[&ChunkCoord(IVec3::new(0, 0, 0))], 1);
        assert_eq!(balanced[&ChunkCoord(IVec3::new(1, 0, 0))], 0);
    }

    /// A chunk with no existing neighbours has nothing to balance against,
    /// so its raw level is left alone even if it's very coarse.
    #[test]
    fn balance_lod_levels_leaves_an_isolated_chunk_unchanged() {
        let mut world = VoxelWorld::new(4);
        let coord = ChunkCoord(IVec3::new(0, 0, 0));
        world.set(world.chunk_origin(coord), Voxel::new(1.0, MaterialId::DIRT));
        let raw_levels = HashMap::from([(coord, 5)]);

        let balanced = balance_lod_levels(&world, &raw_levels);

        assert_eq!(balanced[&coord], 5);
    }

    /// Already-balanced input (every pair of neighbours within one level of
    /// each other) should come back unchanged.
    #[test]
    fn balance_lod_levels_is_a_no_op_when_already_balanced() {
        let mut world = VoxelWorld::new(4);
        for coord in [ChunkCoord(IVec3::new(0, 0, 0)), ChunkCoord(IVec3::new(1, 0, 0))] {
            world.set(world.chunk_origin(coord), Voxel::new(1.0, MaterialId::DIRT));
        }
        let raw_levels = HashMap::from([(ChunkCoord(IVec3::new(0, 0, 0)), 0), (ChunkCoord(IVec3::new(1, 0, 0)), 1)]);

        let balanced = balance_lod_levels(&world, &raw_levels);

        assert_eq!(balanced, raw_levels);
    }

    /// A chunk with two coarser neighbours on different faces (e.g. at a
    /// world corner) must end up with only one transitioning face: one
    /// neighbour stays coarser, the other gets pulled down to match. Which
    /// one is kept is deterministic (`FACE_NEIGHBOR_OFFSETS` order), not
    /// meant to be guessed by callers, but re-running the already-balanced
    /// result must be a genuine fixed point either way.
    #[test]
    fn balance_lod_levels_limits_a_chunk_to_one_transitioning_face() {
        let mut world = VoxelWorld::new(4);
        let center = ChunkCoord(IVec3::new(0, 0, 0));
        let plus_x = ChunkCoord(IVec3::new(1, 0, 0));
        let plus_z = ChunkCoord(IVec3::new(0, 0, 1));
        for coord in [center, plus_x, plus_z] {
            world.set(world.chunk_origin(coord), Voxel::new(1.0, MaterialId::DIRT));
        }
        let raw_levels = HashMap::from([(center, 0), (plus_x, 1), (plus_z, 1)]);

        let balanced = balance_lod_levels(&world, &raw_levels);

        assert_eq!(balanced[&center], 0, "the chunk with 2 coarser neighbours shouldn't itself be raised");
        let coarser_neighbor_count = [balanced[&plus_x], balanced[&plus_z]].iter().filter(|&&l| l == 1).count();
        assert_eq!(coarser_neighbor_count, 1, "exactly one neighbour should remain coarser (one transitioning face)");
        // +X comes before +Z in FACE_NEIGHBOR_OFFSETS, so it's the one kept.
        assert_eq!(balanced[&plus_x], 1);
        assert_eq!(balanced[&plus_z], 0);

        assert_eq!(balance_lod_levels(&world, &balanced), balanced, "an already-balanced result must be a fixed point");
    }

    /// The actual proof that chunking doesn't introduce seams: mesh two
    /// adjacent chunks of a boundary-straddling shape independently (each
    /// through its own `ChunkView`, exactly like `render::chunk_mesh_view`
    /// does), translate each into world space the same way the render
    /// path's chunk `Transform` does, merge the two meshes, and check the
    /// combined result is watertight.
    #[test]
    fn two_adjacent_chunks_mesh_watertight_across_their_shared_boundary() {
        let chunk_size = 4;
        let mut world = VoxelWorld::new(chunk_size);
        // A small block straddling the boundary between chunk (0,0,0) and
        // chunk (1,0,0) at chunk_size 4.
        for x in 2..6 {
            for y in 0..2 {
                for z in 0..2 {
                    world.set(IVec3::new(x, y, z), Voxel::new(1.0, MaterialId::DIRT));
                }
            }
        }

        let materials = MaterialTable::standard();
        let mut merged = crate::core::mesh_data::MeshData::new();

        for coord in [ChunkCoord(IVec3::new(0, 0, 0)), ChunkCoord(IVec3::new(1, 0, 0))] {
            let view = world.view(coord);
            let mesh = surface_nets::mesh(&view, &materials);
            let offset = world.chunk_origin(coord).as_vec3();

            let base = merged.positions.len() as u32;
            merged.positions.extend(mesh.positions.iter().map(|&p| p + offset));
            merged.normals.extend(&mesh.normals);
            merged.indices.extend(mesh.indices.iter().map(|&i| i + base));
        }

        assert_eq!(count_non_manifold_edges(&merged), 0, "chunk boundary should mesh watertight, not crack");
    }
}

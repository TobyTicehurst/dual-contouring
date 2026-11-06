//! Thin ECS-visible wrappers around `core` types. `core` itself stays
//! Bevy-free (see `docs/DESIGN.md`), so the `Component`/`Resource` derives
//! live here instead of on the types they wrap.

use bevy::prelude::*;

use crate::core::chunk::{ChunkCoord, VoxelWorld};
use crate::core::grid::VoxelGrid;
use crate::core::voxel::MaterialTable;

/// Attaches a `VoxelGrid` to an entity. Combine with a `Transform` for world
/// placement — the grid itself only knows its own local `0..size` space.
#[derive(Component)]
pub struct VoxelGridComponent(pub VoxelGrid);

/// Attaches a chunked `VoxelWorld` to an entity. Combine with a `Transform`
/// for world placement, same as `VoxelGridComponent` — see
/// `render::chunk_mesh_view`, which spawns one mesh child per chunk instead
/// of the single mesh child `VoxelGridComponent` gets.
#[derive(Component)]
pub struct VoxelWorldComponent(pub VoxelWorld);

/// ECS-visible wrapper around a `ChunkCoord`, attached to each per-chunk
/// mesh entity `render::chunk_mesh_view` spawns. Not read by anything yet —
/// kept cheap and available for chunk-scoped debugging/remeshing later (see
/// `docs/PLAN.md` Roadmap phase A's lifecycle note).
#[derive(Component, Clone, Copy)]
pub struct ChunkCoordComponent(pub ChunkCoord);

/// The active `MaterialTable`, as a resource so any system can look up a
/// voxel's rendering properties (colour, hardness, ...) by `MaterialId`.
#[derive(Resource)]
pub struct MaterialTableResource(pub MaterialTable);

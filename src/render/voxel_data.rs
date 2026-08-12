//! Thin ECS-visible wrappers around `core` types. `core` itself stays
//! Bevy-free (see `docs/DESIGN.md`), so the `Component`/`Resource` derives
//! live here instead of on the types they wrap.

use bevy::prelude::*;

use crate::core::grid::VoxelGrid;
use crate::core::voxel::MaterialTable;

/// Attaches a `VoxelGrid` to an entity. Combine with a `Transform` for world
/// placement — the grid itself only knows its own local `0..size` space.
#[derive(Component)]
pub struct VoxelGridComponent(pub VoxelGrid);

/// The active `MaterialTable`, as a resource so any system can look up a
/// voxel's rendering properties (colour, hardness, ...) by `MaterialId`.
#[derive(Resource)]
pub struct MaterialTableResource(pub MaterialTable);

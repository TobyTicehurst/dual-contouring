//! Debug view: a wireframe cube at every "active" Dual-Contouring cell (see
//! `core::meshing::common::active_cells`) — the lattice cubes between voxel
//! sample points that have at least one sign-changing edge, i.e. the cells a
//! mesher places at least one vertex inside. A cell's own corners *are*
//! voxel positions, so its cube is offset by half a unit in every axis from
//! the voxel debug cubes in `debug_cubes.rs` — this shows the dual grid
//! directly, which is handy for seeing exactly which cell an
//! ambiguous/split configuration lands in.

use bevy::pbr::wireframe::{Wireframe, WireframeColor, WireframePlugin};
use bevy::prelude::*;

use crate::config::DebugConfig;
use crate::core::meshing::common::active_cells;

use super::voxel_data::{MaterialTableResource, VoxelGridComponent};

pub struct DebugActiveCellsPlugin;

impl Plugin for DebugActiveCellsPlugin {
    fn build(&self, app: &mut App) {
        // `debug_cubes.rs`'s wireframe mode also needs this plugin; Bevy
        // panics if the same plugin type is added twice, so only add it here
        // if nothing else already has.
        if !app.is_plugin_added::<WireframePlugin>() {
            app.add_plugins(WireframePlugin::default());
        }
        app.add_systems(Update, spawn_debug_active_cells);
    }
}

/// Runs once per grid entity (`Added<VoxelGridComponent>`), spawning every
/// active cell's wireframe cube as a child so it inherits the grid's
/// `Transform`. Purely geometric — doesn't depend on the grid's chosen
/// `MesherChoice` or on `diagonal_bridging`, since cell activity is the same
/// regardless of how an active cell's crossings end up grouped.
fn spawn_debug_active_cells(
    mut commands: Commands,
    config: Res<DebugConfig>,
    material_table: Res<MaterialTableResource>,
    mut meshes: ResMut<Assets<Mesh>>,
    grids: Query<(Entity, &VoxelGridComponent), Added<VoxelGridComponent>>,
) {
    if !config.show_debug_active_cells {
        return;
    }

    let cube_mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));

    for (grid_entity, VoxelGridComponent(grid)) in &grids {
        let cells = active_cells(grid, &material_table.0);
        commands.entity(grid_entity).with_children(|parent| {
            for cell_coord in cells {
                parent.spawn((
                    Mesh3d(cube_mesh.clone()),
                    // A cell's own corners are voxel sample points, so its
                    // spatial cube spans `cell_coord` to `cell_coord + 1` —
                    // centred half a unit past the cell's own coordinate.
                    Transform::from_translation(cell_coord.as_vec3() + Vec3::splat(0.5)),
                    Wireframe,
                    WireframeColor { color: config.debug_active_cell_color },
                ));
            }
        });
    }
}

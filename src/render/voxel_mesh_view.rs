//! Spawns a real (non-debug) mesh entity for a grid, built by whichever
//! mesher its `MesherChoice` names. This is the first place `core::meshing`
//! output actually gets converted (via `mesh_convert`) and rendered.

use bevy::prelude::*;
use bevy::render::render_resource::Face;
use glam::IVec3;

use crate::config::DebugConfig;
use crate::core::meshing::{axis_aligned_dc, hardness_blend_dc, reference_surface_nets, surface_nets};
use crate::core::voxel::MaterialId;

use super::mesh_convert::to_bevy_mesh;
use super::voxel_data::{MaterialTableResource, VoxelGridComponent};

/// Which mesher a grid entity should be rendered with. Attach alongside
/// `VoxelGridComponent` to opt an entity into the real mesh renderer.
/// `ReferenceSurfaceNets` isn't part of the "real" lineup at all — it's the
/// `fast_surface_nets` crate's independent implementation of the same idea,
/// kept around purely to visually cross-check our own `SurfaceNets` against
/// (see `core::meshing::reference_surface_nets`).
#[derive(Component, Clone, Copy)]
pub enum MesherChoice {
    SurfaceNets,
    AxisAlignedDualContouring,
    HardnessBlend,
    ReferenceSurfaceNets,
}

pub struct VoxelMeshViewPlugin;

impl Plugin for VoxelMeshViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, spawn_meshed_views);
    }
}

fn spawn_meshed_views(
    mut commands: Commands,
    config: Res<DebugConfig>,
    material_table: Res<MaterialTableResource>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    grids: Query<(Entity, &VoxelGridComponent, &MesherChoice), Added<VoxelGridComponent>>,
) {
    if !config.show_meshed_surface {
        return;
    }

    for (entity, VoxelGridComponent(grid), mesher) in &grids {
        let mesh_data = match mesher {
            MesherChoice::SurfaceNets => surface_nets::mesh(grid, &material_table.0),
            MesherChoice::AxisAlignedDualContouring => axis_aligned_dc::mesh(grid, &material_table.0),
            MesherChoice::HardnessBlend => hardness_blend_dc::mesh(grid, &material_table.0),
            MesherChoice::ReferenceSurfaceNets => reference_surface_nets::mesh(grid, &material_table.0),
        };

        // Still just the first non-air voxel's colour for the whole mesh —
        // fine for every demo grid so far, including the hardness-blend one
        // (which mixes materials, but the colour mismatch is a much smaller
        // issue than getting the *shape* blend right). Real per-vertex
        // colour is coming with the milestone 11 demo scene, which actually
        // needs it (three materials genuinely need to be told apart).
        let size = grid.size();
        let base_color = 'search: {
            for x in 0..size.x {
                for y in 0..size.y {
                    for z in 0..size.z {
                        let voxel = grid.get(IVec3::new(x as i32, y as i32, z as i32));
                        if voxel.material != MaterialId::AIR {
                            let color = material_table.0.get(voxel.material).color;
                            break 'search Color::srgb(color.r, color.g, color.b);
                        }
                    }
                }
            }
            Color::WHITE
        };

        let bevy_mesh = meshes.add(to_bevy_mesh(&mesh_data));
        // `cull_mode: None` renders both sides of every triangle instead of
        // the default back-face culling; `double_sided` tells Bevy's
        // lighting to flip the shading normal for back-facing fragments so
        // the far side isn't lit as if it were the front. Both driven by the
        // same config flag since one without the other looks wrong (either
        // culled, or lit incorrectly).
        let cull_mode = if config.render_backfaces { None } else { Some(Face::Back) };
        commands.entity(entity).with_children(|parent| {
            parent.spawn((
                Mesh3d(bevy_mesh),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color,
                    cull_mode,
                    double_sided: config.render_backfaces,
                    ..default()
                })),
                Transform::IDENTITY,
            ));
        });
    }
}

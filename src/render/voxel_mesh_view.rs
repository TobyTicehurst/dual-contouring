//! Spawns a real (non-debug) mesh entity for a grid, built by whichever
//! mesher its `MesherChoice` names. This is the first place `core::meshing`
//! output actually gets converted (via `mesh_convert`) and rendered.

use bevy::prelude::*;
use bevy::render::render_resource::Face;

use crate::config::DebugConfig;
use crate::core::mesh_data::MeshData;
use crate::core::meshing::{axis_aligned_dc, hardness_blend_dc, reference_surface_nets, surface_nets};

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

        spawn_mesh_child(&mut commands, entity, &mesh_data, Transform::IDENTITY, (), &config, &mut meshes, &mut materials);
    }
}

/// Converts `mesh_data` to a real `bevy::Mesh` (including its per-vertex
/// colour — see `docs/PLAN.md` Roadmap phase B) and spawns it as a child of
/// `parent` at `transform`, with `config.render_backfaces` applied, plus
/// whatever extra components `extra` bundles in (e.g. `chunk_mesh_view`
/// attaches a `ChunkCoordComponent`; `()` for none). The spawned material's
/// `base_color` is left white so the mesh's own per-vertex colour shows
/// through unmodified (a coloured `base_color` would tint it). Shared by
/// `voxel_mesh_view` and `chunk_mesh_view` so the mesh-conversion/material-
/// setup logic isn't duplicated per caller.
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_mesh_child(
    commands: &mut Commands,
    parent: Entity,
    mesh_data: &MeshData,
    transform: Transform,
    extra: impl Bundle,
    config: &DebugConfig,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let bevy_mesh = meshes.add(to_bevy_mesh(mesh_data));
    // `cull_mode: None` renders both sides of every triangle instead of the
    // default back-face culling; `double_sided` tells Bevy's lighting to
    // flip the shading normal for back-facing fragments so the far side
    // isn't lit as if it were the front. Both driven by the same config flag
    // since one without the other looks wrong (either culled, or lit
    // incorrectly).
    let cull_mode = if config.render_backfaces { None } else { Some(Face::Back) };
    commands.entity(parent).with_children(|parent| {
        parent.spawn((
            Mesh3d(bevy_mesh),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::WHITE,
                cull_mode,
                double_sided: config.render_backfaces,
                ..default()
            })),
            transform,
            extra,
        ));
    });
}

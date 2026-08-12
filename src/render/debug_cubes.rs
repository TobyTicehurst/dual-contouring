//! Debug view: a unit cube at every non-air voxel. Unlike the sphere
//! renderer, these are real spawned mesh entities (children of the grid
//! entity), not immediate-mode gizmos — needed for solid/transparent shading
//! and to use Bevy's wireframe pipeline. No optimization: one entity per
//! voxel, no face culling, no greedy meshing (see `docs/PLAN.md` milestone
//! 6) — this is a debug tool, not a real renderer.

use bevy::pbr::wireframe::{Wireframe, WireframePlugin};
use bevy::prelude::*;
use glam::IVec3;

use crate::config::{DebugConfig, DebugCubeMode};
use crate::core::voxel::MaterialId;

use super::voxel_data::{MaterialTableResource, VoxelGridComponent};

pub struct DebugCubesPlugin;

impl Plugin for DebugCubesPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(WireframePlugin::default())
            .add_systems(Update, spawn_debug_cubes);
    }
}

/// Runs once per grid entity (`Added<VoxelGridComponent>`), spawning its
/// voxels as child cube entities so they inherit the grid's `Transform`.
fn spawn_debug_cubes(
    mut commands: Commands,
    config: Res<DebugConfig>,
    material_table: Res<MaterialTableResource>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    grids: Query<(Entity, &VoxelGridComponent), Added<VoxelGridComponent>>,
) {
    if !config.show_debug_cubes {
        return;
    }

    let cube_mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));

    for (grid_entity, VoxelGridComponent(grid)) in &grids {
        let size = grid.size();
        commands.entity(grid_entity).with_children(|parent| {
            for x in 0..size.x {
                for y in 0..size.y {
                    for z in 0..size.z {
                        let local = IVec3::new(x as i32, y as i32, z as i32);
                        let voxel = grid.get(local);
                        if voxel.material == MaterialId::AIR {
                            continue;
                        }

                        let color = material_table.0.get(voxel.material).color;
                        let mut cube = parent.spawn((
                            Mesh3d(cube_mesh.clone()),
                            Transform::from_translation(local.as_vec3()),
                        ));

                        match config.debug_cube_mode {
                            DebugCubeMode::Solid => {
                                cube.insert(MeshMaterial3d(materials.add(StandardMaterial {
                                    base_color: Color::srgb(color.r, color.g, color.b),
                                    ..default()
                                })));
                            }
                            DebugCubeMode::Transparent => {
                                cube.insert(MeshMaterial3d(materials.add(StandardMaterial {
                                    base_color: Color::srgba(
                                        color.r,
                                        color.g,
                                        color.b,
                                        config.debug_cube_transparent_alpha,
                                    ),
                                    alpha_mode: AlphaMode::Blend,
                                    ..default()
                                })));
                            }
                            DebugCubeMode::Wireframe => {
                                // No MeshMaterial3d: the Wireframe component
                                // alone is enough to queue this into Bevy's
                                // wireframe render phase, so no solid fill
                                // is drawn underneath the edges.
                                cube.insert(Wireframe);
                            }
                        }
                    }
                }
            }
        });
    }
}

//! Debug view: a solid sphere at the centre of every non-air voxel, radius
//! scaled by weight, coloured opposite the voxel's own material colour so it
//! doesn't blend in. Reads directly from `VoxelGrid`, bypassing any mesher,
//! so it's useful for verifying raw grid data independent of meshing bugs.
//!
//! Spawned as real mesh entities (like `debug_cubes.rs`), not `Gizmos` —
//! gizmos are line-drawn, so there's no way to get a filled/solid look out
//! of them.

use bevy::prelude::*;
use glam::IVec3;

use crate::config::DebugConfig;
use crate::core::voxel::MaterialId;

use super::voxel_data::{MaterialTableResource, VoxelGridComponent};

pub struct DebugSpheresPlugin;

impl Plugin for DebugSpheresPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, spawn_debug_spheres);
    }
}

/// Runs once per grid entity (`Added<VoxelGridComponent>`), spawning its
/// voxels as child sphere entities so they inherit the grid's `Transform`.
fn spawn_debug_spheres(
    mut commands: Commands,
    config: Res<DebugConfig>,
    material_table: Res<MaterialTableResource>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    grids: Query<(Entity, &VoxelGridComponent), Added<VoxelGridComponent>>,
) {
    if !config.show_debug_spheres {
        return;
    }

    // A unit-radius sphere, scaled per-voxel via Transform rather than
    // building a differently-sized mesh for every weight value.
    let sphere_mesh = meshes.add(Sphere::new(1.0));

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
                        let inverted = Color::srgb(1.0 - color.r, 1.0 - color.g, 1.0 - color.b);
                        let radius = voxel.weight * config.debug_sphere_radius_scale;

                        parent.spawn((
                            Mesh3d(sphere_mesh.clone()),
                            MeshMaterial3d(materials.add(StandardMaterial {
                                base_color: inverted,
                                ..default()
                            })),
                            Transform::from_translation(local.as_vec3()).with_scale(Vec3::splat(radius)),
                        ));
                    }
                }
            }
        });
    }
}

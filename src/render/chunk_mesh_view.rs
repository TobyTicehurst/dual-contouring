//! Spawns one mesh child entity per chunk of a `VoxelWorldComponent`, built
//! by whichever mesher its `MesherChoice` names. Mirrors
//! `voxel_mesh_view::spawn_meshed_views` — this is that same idea, just
//! iterating chunks instead of meshing a single grid — and reuses its
//! mesh-conversion/material-setup helpers rather than duplicating them.
//!
//! Each chunk also picks its own mesh resolution based on distance from the
//! camera at spawn time (see `lod_stride_for_distance`) — the first half of
//! chunk-level LOD (`docs/PLAN.md` Roadmap phase C). Selection happens once,
//! not per frame: re-selecting as the camera moves needs the same reactive
//! lifecycle work already deferred for remeshing (Roadmap phase A). Seams
//! between differently-resolved neighbour chunks are an expected, visible
//! artifact for now — stitching them (Transvoxel-style transition cells) is
//! a separate, not-yet-done follow-up.

use bevy::prelude::*;

use crate::config::DebugConfig;
use crate::core::meshing::{axis_aligned_dc, hardness_blend_dc, surface_nets};
use crate::core::voxel_source::DownsampledView;

use super::voxel_data::{ChunkCoordComponent, MaterialTableResource, VoxelWorldComponent};
use super::voxel_mesh_view::{spawn_mesh_child, MesherChoice};

pub struct ChunkMeshViewPlugin;

impl Plugin for ChunkMeshViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, spawn_chunked_meshed_views);
    }
}

/// Distance-from-camera -> mesh stride (1 = full resolution, higher = every
/// Nth voxel — see `DownsampledView`). Starting values, not yet backed by
/// real performance data — tunable later, same framing as
/// `chunk::DEFAULT_CHUNK_SIZE`. Only power-of-two divisors of the chunk size
/// are valid strides (`DownsampledView::new` asserts this in debug builds).
fn lod_stride_for_distance(distance: f32) -> u32 {
    if distance < 100.0 {
        1
    } else if distance < 200.0 {
        2
    } else if distance < 400.0 {
        4
    } else {
        8
    }
}

fn spawn_chunked_meshed_views(
    mut commands: Commands,
    config: Res<DebugConfig>,
    material_table: Res<MaterialTableResource>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    worlds: Query<(Entity, &VoxelWorldComponent, &MesherChoice, &Transform), Added<VoxelWorldComponent>>,
    camera: Query<&Transform, With<Camera3d>>,
) {
    if !config.show_meshed_surface {
        return;
    }
    let Ok(camera_transform) = camera.single() else {
        return;
    };

    for (entity, VoxelWorldComponent(world), mesher, world_transform) in &worlds {
        let chunk_size = world.chunk_size() as f32;
        let mut stride_counts: std::collections::BTreeMap<u32, u32> = std::collections::BTreeMap::new();
        for (coord, _) in world.chunks() {
            // Meshing runs against a `ChunkView` (this chunk's voxels, plus
            // real neighbouring-chunk data for the single ring `mesh_with`
            // queries just past a chunk's own boundary — see
            // `VoxelWorld::view`), not the chunk's own `VoxelGrid` directly,
            // so a mesher sees genuine neighbouring geometry at the chunk
            // boundary instead of treating it as bordered by air. Every
            // mesher already only consumes a `&dyn VoxelSource`, so this
            // needed no mesher-side changes beyond that abstraction.
            let view = world.view(coord);

            // LOD: pick a stride from this chunk's distance to the camera
            // (in world space, so the parent entity's own placement counts),
            // then mesh a `DownsampledView` of the chunk instead of the bare
            // view. Stride 1 samples every voxel, same as before this pass.
            let chunk_centre = world_transform.translation + world.chunk_origin(coord).as_vec3() + Vec3::splat(chunk_size / 2.0);
            let stride = lod_stride_for_distance(camera_transform.translation.distance(chunk_centre));
            *stride_counts.entry(stride).or_insert(0) += 1;
            let downsampled = DownsampledView::new(&view, stride);

            let mesh_data = match mesher {
                MesherChoice::SurfaceNets => surface_nets::mesh(&downsampled, &material_table.0),
                MesherChoice::AxisAlignedDualContouring => axis_aligned_dc::mesh(&downsampled, &material_table.0),
                MesherChoice::HardnessBlend => hardness_blend_dc::mesh(&downsampled, &material_table.0),
                MesherChoice::ReferenceSurfaceNets => {
                    // The `fast_surface_nets` adapter builds its own SDF
                    // directly against a concrete `VoxelGrid` rather than
                    // through the shared `VoxelSource` abstraction (see
                    // `meshing::reference_surface_nets`) — it exists purely
                    // to visually cross-check `SurfaceNets` on a single
                    // grid, so chunked worlds just skip it.
                    continue;
                }
            };

            // A downsampled mesh's positions are in downsampled-local space
            // (vertex `n` along an axis = real chunk-local position `n *
            // stride` — see `DownsampledView`), so the chunk's own scale
            // corrects it back to world space alongside its translation.
            let transform =
                Transform::from_translation(world.chunk_origin(coord).as_vec3()).with_scale(Vec3::splat(stride as f32));
            spawn_mesh_child(
                &mut commands,
                entity,
                &mesh_data,
                transform,
                ChunkCoordComponent(coord),
                &config,
                &mut meshes,
                &mut materials,
            );
        }
        info!("chunked world at {:?}: chunk stride distribution {:?}", world_transform.translation, stride_counts);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lod_stride_picks_full_resolution_up_close() {
        assert_eq!(lod_stride_for_distance(0.0), 1);
        assert_eq!(lod_stride_for_distance(99.9), 1);
    }

    #[test]
    fn lod_stride_increases_at_each_band_boundary() {
        assert_eq!(lod_stride_for_distance(100.0), 2);
        assert_eq!(lod_stride_for_distance(199.9), 2);
        assert_eq!(lod_stride_for_distance(200.0), 4);
        assert_eq!(lod_stride_for_distance(399.9), 4);
        assert_eq!(lod_stride_for_distance(400.0), 8);
        assert_eq!(lod_stride_for_distance(10_000.0), 8);
    }
}

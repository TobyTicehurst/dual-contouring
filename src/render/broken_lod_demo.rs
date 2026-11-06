//! Visualizes the known-broken LOD boundary stitching attempt from
//! `core::meshing::transition` (see its module docs and `docs/PLAN.md`
//! Roadmap phase C, "Increment 2, attempted, not working yet") — spawned
//! purely so the crack/mismatch is visible in-app, not because the
//! mechanism is considered working. Deliberately bypasses the normal
//! `ChunkMeshViewPlugin` path: it reproduces exactly what
//! `transition::tests::fine_and_coarse_chunks_mesh_watertight_across_a_2_to_1_boundary`
//! does (same shape, same override computation), just converted to a real
//! Bevy mesh instead of an in-memory `MeshData` assertion.

use bevy::prelude::*;
use glam::IVec3;

use crate::core::chunk::{ChunkCoord, VoxelWorld};
use crate::core::meshing::common::{mesh_with_overrides, Axis, BoundarySuppression};
use crate::core::meshing::surface_nets;
use crate::core::meshing::transition::{coarse_override_cell, fine_override_cells, transition_vertex, TangentialSpan};
use crate::core::voxel::{Color as CoreColor, MaterialId, MaterialTable, Voxel};
use crate::core::voxel_source::DownsampledView;

use super::mesh_convert::to_bevy_mesh;

pub struct BrokenLodDemoPlugin;

impl Plugin for BrokenLodDemoPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_broken_lod_demo);
    }
}

fn spawn_broken_lod_demo(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let chunk_size = 8;
    let mut world = VoxelWorld::new(chunk_size);
    // The exact shape the failing test uses: a flat slab spanning both
    // chunks' full x/z extent, so the only interesting geometry is the top
    // cap and the shared X boundary between them.
    for x in 0..16 {
        for z in 0..8 {
            world.set(IVec3::new(x, 0, z), Voxel::new(1.0, MaterialId::DIRT));
        }
    }

    let materials_table = MaterialTable::standard();
    let fine_coord = ChunkCoord(IVec3::new(0, 0, 0));
    let coarse_coord = ChunkCoord(IVec3::new(1, 0, 0));
    let fine_stride = 1;
    let coarse_stride = 2;
    let footprints = (chunk_size / coarse_stride) as i32;

    // Every tangential span along the shared face: the regular interior
    // footprints plus the RingLow/RingHigh perimeter on each axis — see
    // `TangentialSpan`'s docs.
    let spans = |count: i32| -> Vec<TangentialSpan> {
        std::iter::once(TangentialSpan::RingLow)
            .chain((0..count).map(TangentialSpan::Regular))
            .chain(std::iter::once(TangentialSpan::RingHigh))
            .collect()
    };
    let t1_spans = spans(footprints);
    let t2_spans = spans(footprints);

    let mut fine_overrides = Vec::new();
    let mut coarse_overrides = Vec::new();
    let fine_raw_view = world.view(fine_coord);
    for &t1_span in &t1_spans {
        for &t2_span in &t2_spans {
            let Some((position, color)) =
                transition_vertex(&fine_raw_view, &materials_table, chunk_size, Axis::X, true, t1_span, t2_span, coarse_stride)
            else {
                continue;
            };
            for cell in fine_override_cells(Axis::X, true, t1_span, t2_span, chunk_size, fine_stride) {
                fine_overrides.push((cell, position, color));
            }
            let coarse_cell = coarse_override_cell(Axis::X, true, t1_span, t2_span, chunk_size, coarse_stride);
            coarse_overrides.push((coarse_cell, position / coarse_stride as f32, color));
        }
    }

    let mut fine_mesh = mesh_with_overrides(
        &fine_raw_view,
        &materials_table,
        surface_nets::place_vertex,
        BoundarySuppression::default(),
        &fine_overrides,
    );
    let coarse_raw_view = world.view(coarse_coord);
    let coarse_downsampled = DownsampledView::new(&coarse_raw_view, coarse_stride);
    let mut coarse_mesh = mesh_with_overrides(
        &coarse_downsampled,
        &materials_table,
        surface_nets::place_vertex,
        BoundarySuppression::default(),
        &coarse_overrides,
    );

    // Flat, contrasting colours (instead of the natural dirt brown, which
    // would be identical on both sides) so any crack/misalignment at the
    // shared boundary is obvious rather than needing to be spotted by
    // subtle shading differences.
    let fine_color = CoreColor::rgb(0.85, 0.2, 0.2);
    let coarse_color = CoreColor::rgb(0.2, 0.4, 0.9);
    fine_mesh.colors.iter_mut().for_each(|c| *c = fine_color);
    coarse_mesh.colors.iter_mut().for_each(|c| *c = coarse_color);

    // Positioned well clear of every other demo. Backface rendering is
    // forced on (independent of `DebugConfig::render_backfaces`) since the
    // whole point here is to see the raw, possibly-inconsistently-wound
    // output of a known-broken mesher, not to hide half of it behind
    // culling.
    let demo_origin = Vec3::new(-60.0, 0.0, 0.0);
    let parent = commands
        .spawn((Transform::from_translation(demo_origin), Visibility::default(), Name::new("broken LOD stitching demo (red=fine, blue=coarse)")))
        .id();

    let material = materials.add(StandardMaterial { base_color: Color::WHITE, cull_mode: None, double_sided: true, ..default() });

    let fine_offset = world.chunk_origin(fine_coord).as_vec3();
    commands.entity(parent).with_children(|parent| {
        parent.spawn((Mesh3d(meshes.add(to_bevy_mesh(&fine_mesh))), MeshMaterial3d(material.clone()), Transform::from_translation(fine_offset)));
    });

    let coarse_offset = world.chunk_origin(coarse_coord).as_vec3();
    commands.entity(parent).with_children(|parent| {
        parent.spawn((
            Mesh3d(meshes.add(to_bevy_mesh(&coarse_mesh))),
            MeshMaterial3d(material),
            Transform::from_translation(coarse_offset).with_scale(Vec3::splat(coarse_stride as f32)),
        ));
    });

    info!(
        "broken LOD stitching demo spawned at {demo_origin:?}: fine (red) chunk at local x=0..8, coarse (blue) chunk at local x=8..16 — fly there to see the crack (see docs/PLAN.md Roadmap phase C)"
    );
}

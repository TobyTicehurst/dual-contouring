mod config;
mod core;
mod render;

use bevy::diagnostic::FrameCount;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use glam::IVec3;

use crate::config::DebugConfig;
use crate::core::chunk::{VoxelWorld, DEFAULT_CHUNK_SIZE};
use crate::core::demo_scenes;
use crate::core::grid::VoxelGrid;
use crate::core::voxel::{MaterialId, MaterialTable};
use render::broken_lod_demo::BrokenLodDemoPlugin;
use render::chunk_mesh_view::ChunkMeshViewPlugin;
use render::debug_active_cells::DebugActiveCellsPlugin;
use render::debug_cubes::DebugCubesPlugin;
use render::debug_spheres::DebugSpheresPlugin;
use render::fly_camera::{FlyCamera, FlyCameraPlugin};
use render::voxel_data::{MaterialTableResource, VoxelGridComponent, VoxelWorldComponent};
use render::voxel_mesh_view::{MesherChoice, VoxelMeshViewPlugin};

fn main() {
    App::new()
        // The window starts invisible and is only shown once the first real
        // frame has actually been rendered (`make_window_visible` below) —
        // without this, the OS paints its own default (white) background for
        // the window's first frame or two before Bevy's own render output is
        // ready, which reads as a startup flash unrelated to anything this
        // app draws.
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window { visible: false, ..default() }),
            ..default()
        }))
        .add_plugins((
            FlyCameraPlugin,
            DebugSpheresPlugin,
            DebugCubesPlugin,
            DebugActiveCellsPlugin,
            VoxelMeshViewPlugin,
            ChunkMeshViewPlugin,
            BrokenLodDemoPlugin,
        ))
        .insert_resource(DebugConfig::default())
        .insert_resource(MaterialTableResource(MaterialTable::standard()))
        .add_systems(Startup, setup)
        .add_systems(Update, make_window_visible)
        .run();
}

/// Waits a few frames past the first (matching the delay Bevy's own
/// `window_settings` example uses) before revealing the window, so the
/// swapchain has definitely presented a real frame first — revealing it
/// immediately on frame 1 risks the same one-frame flash this exists to
/// avoid.
fn make_window_visible(mut windows: Query<&mut Window, With<PrimaryWindow>>, frames: Res<FrameCount>) {
    if frames.0 != 3 {
        return;
    }
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    window.visible = true;
}

fn setup(mut commands: Commands) {
    commands.spawn((
        DirectionalLight {
            illuminance: 8000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.9, 0.4, 0.0)),
    ));

    // Overridable via env vars (each "x,y,z") so a specific demo can be
    // framed for an automated screenshot (see `bevy_ci_testing`, enabled in
    // Cargo.toml) without editing this default. Falls back to the original
    // hardcoded view of the first few demos if unset or unparseable.
    let camera_pos = env_vec3("VOXELS2_CAMERA_POS").unwrap_or(Vec3::new(8.0, 14.0, 34.0));
    let camera_look_at = env_vec3("VOXELS2_CAMERA_LOOK_AT").unwrap_or(Vec3::new(8.0, 3.0, 8.0));
    let camera_transform = Transform::from_translation(camera_pos).looking_at(camera_look_at, Vec3::Y);
    commands.spawn((
        Camera3d::default(),
        camera_transform,
        FlyCamera::from_rotation(camera_transform.rotation),
    ));

    // Three demo scenes side by side along X. The actual drawing happens in
    // render/debug_spheres.rs, render/debug_cubes.rs (both opt-in, off by
    // default — see config.rs) and render/voxel_mesh_view.rs (the real
    // meshed surface, on by default) — this system just builds the grids
    // and hands them to the ECS.
    //
    // The wall uses the new axis-aligned mesher (milestone 8) since that's
    // exactly what it's meant for; the hill and branch stay on SurfaceNets
    // until the hardness-blend mesher (milestone 9) exists to choose per
    // material. A second copy of the wall, meshed by SurfaceNets, sits right
    // behind the first for the same "flat and sharp vs. rounded" comparison
    // the plan calls for.
    // spawn_demo_grid(
    //     &mut commands,
    //     "rolling hill",
    //     demo_scenes::rolling_hill_slab(16, 16, 4),
    //     Vec3::new(0.0, 0.0, 0.0),
    //     MesherChoice::SurfaceNets,
    // );
    spawn_demo_grid(
        &mut commands,
        "brick wall",
        demo_scenes::brick_wall_with_window(9, 9, 1),
        Vec3::new(18.0, 0.0, 0.0),
        MesherChoice::AxisAlignedDualContouring,
    );
    spawn_demo_grid(
        &mut commands,
        "brick wall (SurfaceNets, for comparison)",
        demo_scenes::brick_wall_with_window(9, 9, 1),
        Vec3::new(18.0, 0.0, -18.0),
        MesherChoice::SurfaceNets,
    );
    spawn_demo_grid(
        &mut commands,
        "wood branch",
        demo_scenes::wood_branch(12),
        Vec3::new(29.0, 0.0, 0.0),
        MesherChoice::SurfaceNets,
    );

    // The diagonal-saddle configuration (two voxels touching only
    // diagonally) that used to crack under naive SurfaceNets before the
    // opaque-component-splitting fix (see `common::opaque_components`).
    // Tiny and separated from everything else so it's easy to fly right up
    // to and check it's actually watertight now.
    spawn_demo_grid(
        &mut commands,
        "diagonal saddle (now fixed via component splitting)",
        demo_scenes::diagonal_saddle_pair(),
        Vec3::new(40.0, 0.0, 0.0),
        MesherChoice::SurfaceNets,
    );

    // Same diagonal-saddle grid, meshed by the independent `fast_surface_nets`
    // reference crate instead — it has no equivalent of the
    // opaque-component-splitting fix, so this should still show the crack our
    // own mesher no longer has, right next to the fixed version for a direct
    // comparison.
    // spawn_demo_grid(
    //     &mut commands,
    //     "diagonal saddle (fast_surface_nets reference, still cracked)",
    //     demo_scenes::diagonal_saddle_pair(),
    //     Vec3::new(46.0, 0.0, 0.0),
    //     MesherChoice::ReferenceSurfaceNets,
    // );

    // Same rolling hill, meshed by the independent `fast_surface_nets`
    // reference crate instead of our own mesher — placed just behind the
    // original (along -Z, out of the other three's way) for a direct visual
    // comparison. See core::meshing::reference_surface_nets for why this
    // exists: cross-checking whether the saddle-point cracks in our own
    // mesher are a bug in our code or an inherent trait of naive Surface
    // Nets that a mature, independent implementation shares.
    // spawn_demo_grid(
    //     &mut commands,
    //     "rolling hill (fast_surface_nets reference)",
    //     demo_scenes::rolling_hill_slab(16, 16, 4),
    //     Vec3::new(0.0, 0.0, -18.0),
    //     MesherChoice::ReferenceSurfaceNets,
    // );

    // A block half dirt (hardness 0), half brick (hardness 1), meshed by the
    // new hardness-blend mesher (milestone 9): should read as rounded on the
    // dirt side, sharp on the brick side, with a smooth transition at the
    // seam rather than a hard discontinuity.
    // spawn_demo_grid(
    //     &mut commands,
    //     "mixed hardness block",
    //     demo_scenes::mixed_hardness_block(8, 4, 4),
    //     Vec3::new(29.0, 0.0, -18.0),
    //     MesherChoice::HardnessBlend,
    // );

    // The exact same voxel sphere, three times, differing only in material
    // (and so only in hardness) — dirt/wood/brick side by side, all through
    // the hardness-blend mesher. Where the mixed block shows the blend
    // *across* a material boundary, this isolates what each hardness value
    // alone looks like: smooth throughout (0.0), in between (0.5), or
    // faceted/blocky throughout (1.0).
    // let sphere_row_z = -36.0;
    // spawn_demo_grid(
    //     &mut commands,
    //     "voxel sphere, hardness 0.0 (dirt)",
    //     demo_scenes::voxel_sphere(6, MaterialId::DIRT),
    //     Vec3::new(0.0, 0.0, sphere_row_z),
    //     MesherChoice::HardnessBlend,
    // );
    // spawn_demo_grid(
    //     &mut commands,
    //     "voxel sphere, hardness 0.5 (wood)",
    //     demo_scenes::voxel_sphere(6, MaterialId::WOOD),
    //     Vec3::new(15.0, 0.0, sphere_row_z),
    //     MesherChoice::HardnessBlend,
    // );
    // spawn_demo_grid(
    //     &mut commands,
    //     "voxel sphere, hardness 1.0 (brick)",
    //     demo_scenes::voxel_sphere(6, MaterialId::BRICK),
    //     Vec3::new(30.0, 0.0, sphere_row_z),
    //     MesherChoice::HardnessBlend,
    // );

    // A rolling hill big enough (80x80) to span multiple chunks in both X
    // and Z at the default 32^3 chunk size, split into a `VoxelWorld` via
    // `VoxelWorld::from_dense` so the existing single-grid demo generator can
    // be reused as-is. Proves chunk boundaries mesh seamlessly (via
    // `VoxelWorld::padded_grid_for_meshing`'s cross-chunk halo) rather than
    // as if each chunk were bordered by air — see `docs/PLAN.md` Roadmap
    // phase A. Placed well clear of the other demos.
    // let big_hill = demo_scenes::rolling_hill_slab(80, 80, 4);
    // let chunked_world = VoxelWorld::from_dense(&big_hill, DEFAULT_CHUNK_SIZE);
    // spawn_chunked_demo_world(
    //     &mut commands,
    //     "chunked rolling hill (multi-chunk)",
    //     chunked_world,
    //     Vec3::new(60.0, 0.0, 0.0),
    //     MesherChoice::SurfaceNets,
    // );

    // A much bigger hill (4x4 chunks at the default 32^3 chunk size), placed
    // diagonally far from the default camera position (8,14,34) so its near
    // corner still meshes at full resolution but its far corner is well past
    // the coarsest LOD band (see `render::chunk_mesh_view::lod_stride_for_distance`)
    // — proves distance-based per-chunk resolution selection actually varies
    // across a single scene (Roadmap phase C, increment 1). The visible seams
    // between differently-resolved neighbour chunks here are expected, not a
    // bug — see `docs/PLAN.md`.
    // let lod_hill = demo_scenes::rolling_hill_slab(128, 128, 4);
    // let lod_world = VoxelWorld::from_dense(&lod_hill, DEFAULT_CHUNK_SIZE);
    // spawn_chunked_demo_world(
    //     &mut commands,
    //     "chunked rolling hill (LOD demo, varying resolution with distance)",
    //     lod_world,
    //     Vec3::new(-8.0, 0.0, -160.0),
    //     MesherChoice::SurfaceNets,
    // );
}

/// Parses `name`'s env var as `"x,y,z"`, if set and well-formed. Returns
/// `None` on anything else (unset, wrong field count, unparseable number) so
/// callers can fall back to their own default rather than panicking on a
/// malformed override.
fn env_vec3(name: &str) -> Option<Vec3> {
    let value = std::env::var(name).ok()?;
    let mut parts = value.splitn(3, ',').map(|s| s.trim().parse::<f32>());
    let x = parts.next()?.ok()?;
    let y = parts.next()?.ok()?;
    let z = parts.next()?.ok()?;
    Some(Vec3::new(x, y, z))
}

fn spawn_demo_grid(
    commands: &mut Commands,
    name: &str,
    grid: VoxelGrid,
    position: Vec3,
    mesher: MesherChoice,
) {
    let size = grid.size();
    let mut non_air = 0u32;
    for x in 0..size.x {
        for y in 0..size.y {
            for z in 0..size.z {
                if grid.get(IVec3::new(x as i32, y as i32, z as i32)).material != MaterialId::AIR {
                    non_air += 1;
                }
            }
        }
    }
    info!(
        "demo scene '{name}': {}x{}x{} grid at {position:?}, {non_air} non-air voxels",
        size.x, size.y, size.z
    );

    commands.spawn((
        VoxelGridComponent(grid),
        mesher,
        Transform::from_translation(position),
        Visibility::default(),
    ));
}

fn spawn_chunked_demo_world(
    commands: &mut Commands,
    name: &str,
    world: VoxelWorld,
    position: Vec3,
    mesher: MesherChoice,
) {
    let chunk_count = world.chunks().count();
    info!("demo scene '{name}': chunked world at {position:?}, {chunk_count} chunks (chunk size {})", world.chunk_size());

    commands.spawn((
        VoxelWorldComponent(world),
        mesher,
        Transform::from_translation(position),
        Visibility::default(),
    ));
}

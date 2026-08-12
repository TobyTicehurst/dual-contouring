mod config;
mod core;
mod render;

use bevy::prelude::*;
use glam::IVec3;

use crate::config::DebugConfig;
use crate::core::demo_scenes;
use crate::core::grid::VoxelGrid;
use crate::core::voxel::{MaterialId, MaterialTable};
use render::debug_cubes::DebugCubesPlugin;
use render::debug_spheres::DebugSpheresPlugin;
use render::fly_camera::{FlyCamera, FlyCameraPlugin};
use render::voxel_data::{MaterialTableResource, VoxelGridComponent};
use render::voxel_mesh_view::{MesherChoice, VoxelMeshViewPlugin};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins((
            FlyCameraPlugin,
            DebugSpheresPlugin,
            DebugCubesPlugin,
            VoxelMeshViewPlugin,
        ))
        .insert_resource(DebugConfig::default())
        .insert_resource(MaterialTableResource(MaterialTable::standard()))
        .add_systems(Startup, setup)
        .run();
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

    let camera_transform =
        Transform::from_xyz(8.0, 14.0, 34.0).looking_at(Vec3::new(8.0, 3.0, 8.0), Vec3::Y);
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
    spawn_demo_grid(
        &mut commands,
        "rolling hill",
        demo_scenes::rolling_hill_slab(16, 16, 4),
        Vec3::new(0.0, 0.0, 0.0),
        MesherChoice::SurfaceNets,
    );
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

    // Minimal repro of the diagonal-saddle crack (milestone 7's known
    // limitation): two voxels touching only diagonally, meshed by
    // SurfaceNets — the mesher whose naive winding rule produces the
    // ambiguous-face artifact. Tiny and separated from everything else so
    // it's easy to fly right up to and inspect closely.
    spawn_demo_grid(
        &mut commands,
        "diagonal saddle (known SurfaceNets crack)",
        demo_scenes::diagonal_saddle_pair(),
        Vec3::new(40.0, 0.0, 0.0),
        MesherChoice::SurfaceNets,
    );

    // Same rolling hill, meshed by the independent `fast_surface_nets`
    // reference crate instead of our own mesher — placed just behind the
    // original (along -Z, out of the other three's way) for a direct visual
    // comparison. See core::meshing::reference_surface_nets for why this
    // exists: cross-checking whether the saddle-point cracks in our own
    // mesher are a bug in our code or an inherent trait of naive Surface
    // Nets that a mature, independent implementation shares.
    spawn_demo_grid(
        &mut commands,
        "rolling hill (fast_surface_nets reference)",
        demo_scenes::rolling_hill_slab(16, 16, 4),
        Vec3::new(0.0, 0.0, -18.0),
        MesherChoice::ReferenceSurfaceNets,
    );

    // A block half dirt (hardness 0), half brick (hardness 1), meshed by the
    // new hardness-blend mesher (milestone 9): should read as rounded on the
    // dirt side, sharp on the brick side, with a smooth transition at the
    // seam rather than a hard discontinuity.
    spawn_demo_grid(
        &mut commands,
        "mixed hardness block",
        demo_scenes::mixed_hardness_block(8, 4, 4),
        Vec3::new(29.0, 0.0, -18.0),
        MesherChoice::HardnessBlend,
    );

    // The exact same voxel sphere, three times, differing only in material
    // (and so only in hardness) — dirt/wood/brick side by side, all through
    // the hardness-blend mesher. Where the mixed block shows the blend
    // *across* a material boundary, this isolates what each hardness value
    // alone looks like: smooth throughout (0.0), in between (0.5), or
    // faceted/blocky throughout (1.0).
    let sphere_row_z = -36.0;
    spawn_demo_grid(
        &mut commands,
        "voxel sphere, hardness 0.0 (dirt)",
        demo_scenes::voxel_sphere(6, MaterialId::DIRT),
        Vec3::new(0.0, 0.0, sphere_row_z),
        MesherChoice::HardnessBlend,
    );
    spawn_demo_grid(
        &mut commands,
        "voxel sphere, hardness 0.5 (wood)",
        demo_scenes::voxel_sphere(6, MaterialId::WOOD),
        Vec3::new(15.0, 0.0, sphere_row_z),
        MesherChoice::HardnessBlend,
    );
    spawn_demo_grid(
        &mut commands,
        "voxel sphere, hardness 1.0 (brick)",
        demo_scenes::voxel_sphere(6, MaterialId::BRICK),
        Vec3::new(30.0, 0.0, sphere_row_z),
        MesherChoice::HardnessBlend,
    );
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

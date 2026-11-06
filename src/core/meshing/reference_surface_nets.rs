//! A second implementation of "mesher 1", built on the well-established
//! `fast_surface_nets` crate (Naive Surface Nets over a signed distance
//! field) instead of our own `common.rs` scanning code. This exists purely
//! so the two can be rendered side by side and visually compared — see the
//! saddle-point/ambiguous-face discussion around `surface_nets.rs`, and
//! `fast_surface_nets`'s own `maybe_make_quad`, which uses the same
//! winding-selection rule ours does (verified by hand against its source).
//!
//! Building a signed distance field from our material/weight model is an
//! approximation — our voxels don't carry a real distance field, just a
//! per-voxel weight meaningful only at opaque/non-opaque crossings — so
//! don't expect pixel-identical output. The point of this comparison is
//! whether the same *characteristic* artifacts (e.g. cracks at saddle
//! points) show up in a mature, independent implementation too, not to
//! reproduce our own mesher exactly.

use fast_surface_nets::ndshape::{RuntimeShape, Shape};
use fast_surface_nets::{surface_nets, SurfaceNetsBuffer};
use glam::{IVec3, Vec3};

use crate::core::grid::VoxelGrid;
use crate::core::mesh_data::MeshData;
use crate::core::voxel::{Color, MaterialId, MaterialTable};

/// This adapter builds its own SDF directly from `grid`/`materials` rather
/// than going through `common.rs`'s `Cell` abstraction, so it has no
/// per-vertex material data to draw a real blend from (see
/// `docs/PLAN.md` Roadmap phase B) — every vertex just gets the grid's
/// first non-air voxel's colour instead, same flat-colour simplification
/// `render::voxel_mesh_view` used before per-vertex colour existed. Fine
/// here: every demo scene this mesher is used on is single-material.
fn first_non_air_color(grid: &VoxelGrid, materials: &MaterialTable) -> Color {
    let size = grid.size();
    for x in 0..size.x {
        for y in 0..size.y {
            for z in 0..size.z {
                let voxel = grid.get(IVec3::new(x as i32, y as i32, z as i32));
                if voxel.material != MaterialId::AIR {
                    return materials.get(voxel.material).color;
                }
            }
        }
    }
    Color::rgb(1.0, 1.0, 1.0)
}

pub fn mesh(grid: &VoxelGrid, materials: &MaterialTable) -> MeshData {
    let color = first_non_air_color(grid, materials);
    let size = grid.size();
    // 1-voxel padding on every side: matches our own out-of-bounds-is-Air
    // convention, and is required by fast_surface_nets itself (see its
    // module docs — chunks must be padded with a 1-voxel border, or the
    // boundary faces of the shape simply won't be generated).
    let padded = [size.x + 2, size.y + 2, size.z + 2];
    let shape = RuntimeShape::<u32, 3>::new(padded);

    let mut sdf = vec![1.0f32; shape.usize()];
    for x in 0..padded[0] {
        for y in 0..padded[1] {
            for z in 0..padded[2] {
                // Shift back by 1 to map the padded array onto our grid's
                // own local coordinates; the resulting out-of-range reads
                // (the padding ring itself) come back as Air for free.
                let local = IVec3::new(x as i32 - 1, y as i32 - 1, z as i32 - 1);
                let voxel = grid.get(local);
                let opaque = materials.get(voxel.material).opaque;
                // Negative = inside/solid, positive = outside, per
                // fast_surface_nets's SDF convention. There's no true
                // distance here, just voxel weight standing in for it.
                let value = if opaque { -voxel.weight } else { 1.0 };
                let index = shape.linearize([x, y, z]);
                sdf[index as usize] = value;
            }
        }
    }

    let mut buffer = SurfaceNetsBuffer::default();
    let max = [padded[0] - 1, padded[1] - 1, padded[2] - 1];
    surface_nets(&sdf, &shape, [0, 0, 0], max, &mut buffer);

    // Shift positions back into our grid's local coordinate space, and use
    // our own flat-per-face normal convention (see docs/DESIGN.md) rather
    // than the crate's smooth gradient-based normals, so the side-by-side
    // comparison isn't confounded by a difference in shading style too.
    let offset = Vec3::new(1.0, 1.0, 1.0);
    let mut mesh_data = MeshData::new();
    for tri in buffer.indices.chunks_exact(3) {
        let p: [Vec3; 3] = std::array::from_fn(|i| Vec3::from(buffer.positions[tri[i] as usize]) - offset);
        let normal = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
        mesh_data.push_triangle(p, normal, [color; 3]);
    }

    mesh_data
}

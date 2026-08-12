//! Small hardcoded voxel scenes for exercising meshers during development,
//! before any real world-building/authoring tooling exists (see
//! `docs/PLAN.md` milestone 4). Each one is deliberately shaped to stress a
//! specific thing a mesher needs to get right.

use glam::{IVec3, UVec3, Vec3};

use super::grid::VoxelGrid;
use super::voxel::{MaterialId, Voxel};

/// A slab of dirt with a gently undulating top surface, including fractional
/// top-layer weight — useful for testing weight-based interpolation as soon
/// as a smooth mesher exists (dirt is meant to read as rolling hills).
pub fn rolling_hill_slab(width: u32, depth: u32, base_height: u32) -> VoxelGrid {
    let max_bump = 2.5_f32;
    let height_at = |x: u32, z: u32| -> f32 {
        base_height as f32 + max_bump * (x as f32 * 0.35).sin() * (z as f32 * 0.35).cos()
    };

    let max_height = base_height + max_bump.ceil() as u32 + 1;
    let mut grid = VoxelGrid::new(UVec3::new(width, max_height, depth));

    for x in 0..width {
        for z in 0..depth {
            let height = height_at(x, z).max(1.0);
            let full_layers = (height.floor() as u32).min(max_height);
            let fractional = height - height.floor();

            for y in 0..full_layers {
                grid.set(IVec3::new(x as i32, y as i32, z as i32), Voxel::new(1.0, MaterialId::DIRT));
            }
            if fractional > 0.0 && full_layers < max_height {
                grid.set(
                    IVec3::new(x as i32, full_layers as i32, z as i32),
                    Voxel::new(fractional, MaterialId::DIRT),
                );
            }
        }
    }

    grid
}

/// A brick wall with a rectangular window cut out of the middle, so it
/// exercises right angles at both outer edges and an interior opening (not
/// just a plain solid block).
pub fn brick_wall_with_window(width: u32, height: u32, thickness: u32) -> VoxelGrid {
    let mut grid = VoxelGrid::new(UVec3::new(width, height, thickness));

    let window_x = width / 3..(width * 2 / 3).max(width / 3 + 1);
    let window_y = height / 3..(height * 2 / 3).max(height / 3 + 1);

    for x in 0..width {
        for y in 0..height {
            if window_x.contains(&x) && window_y.contains(&y) {
                continue;
            }
            for z in 0..thickness {
                grid.set(IVec3::new(x as i32, y as i32, z as i32), Voxel::new(1.0, MaterialId::BRICK));
            }
        }
    }

    grid
}

/// A thin wood branch that steps diagonally on (almost) every voxel —
/// specifically meant to probe the open question in `docs/DESIGN.md` about
/// whether plain Dual Contouring connectivity looks acceptable across voxels
/// that only touch diagonally, without any special-cased neighbour sampling.
/// Weight tapers along its length so the branch thins out toward the tip.
pub fn wood_branch(length: u32) -> VoxelGrid {
    let size = length + 2;
    let mut grid = VoxelGrid::new(UVec3::new(size, size, size));

    for i in 0..length {
        let pos = IVec3::new(1 + (i / 2) as i32, 1 + i as i32, 1 + (i / 3) as i32);
        let taper = 1.0 - 0.5 * (i as f32 / length.max(1) as f32);
        grid.set(pos, Voxel::new(taper, MaterialId::WOOD));
    }

    grid
}

/// A solid block, dirt on one half and brick on the other, both at full
/// weight — isolates the hardness-blend mesher's own effect (a smooth-to-
/// sharp transition at the material seam) from weight-driven smoothness,
/// and unlike a flat open-topped slab, a standalone block has real corners/
/// edges where "rounded vs. sharp" is actually visible (see
/// `hardness_blend_dc.rs`).
pub fn mixed_hardness_block(width: u32, height: u32, depth: u32) -> VoxelGrid {
    let mut grid = VoxelGrid::new(UVec3::new(width, height, depth));

    for x in 0..width {
        let material = if x < width / 2 { MaterialId::DIRT } else { MaterialId::BRICK };
        for y in 0..height {
            for z in 0..depth {
                grid.set(IVec3::new(x as i32, y as i32, z as i32), Voxel::new(1.0, material));
            }
        }
    }

    grid
}

/// A voxelised sphere, filled entirely with one material — the same set of
/// voxel positions regardless of `material`, so calling this with `DIRT`,
/// `WOOD`, and `BRICK` gives three shapes that are identical except for
/// hardness. Unlike a flat slab or a plain box, a sphere's curved surface
/// puts corner-ish geometry in essentially every cell, so smooth-vs-blocky
/// differences between meshers show up everywhere on it, not just at a few
/// edges.
pub fn voxel_sphere(radius: u32, material: MaterialId) -> VoxelGrid {
    let diameter = radius * 2 + 1;
    let mut grid = VoxelGrid::new(UVec3::splat(diameter));
    let centre = Vec3::splat(radius as f32);

    for x in 0..diameter {
        for y in 0..diameter {
            for z in 0..diameter {
                let offset = Vec3::new(x as f32, y as f32, z as f32) - centre;
                if offset.length() <= radius as f32 {
                    grid.set(IVec3::new(x as i32, y as i32, z as i32), Voxel::new(1.0, material));
                }
            }
        }
    }

    grid
}

/// Two opaque voxels touching only diagonally, with the other two corners of
/// their shared face left as air — the minimal case that triggers the
/// "diagonal saddle" / ambiguous-face crack in mesher 1 (see
/// `surface_nets.rs`'s `diagonal_saddle_is_a_known_naive_limitation` test,
/// which this mirrors exactly). Visualising it directly, rather than only
/// checking it in a unit test, makes the artifact easy to go find and look
/// at when revisiting the winding logic.
pub fn diagonal_saddle_pair() -> VoxelGrid {
    let mut grid = VoxelGrid::new(UVec3::new(4, 3, 4));
    grid.set(IVec3::new(1, 0, 1), Voxel::new(1.0, MaterialId::DIRT));
    grid.set(IVec3::new(2, 0, 1), Voxel::new(1.0, MaterialId::DIRT));
    grid.set(IVec3::new(1, 0, 2), Voxel::new(1.0, MaterialId::DIRT));
    grid.set(IVec3::new(2, 0, 2), Voxel::new(1.0, MaterialId::DIRT));
    grid.set(IVec3::new(1, 1, 1), Voxel::new(1.0, MaterialId::DIRT));
    grid.set(IVec3::new(2, 1, 2), Voxel::new(1.0, MaterialId::DIRT));
    grid
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count_non_air(grid: &VoxelGrid) -> usize {
        let size = grid.size();
        let mut count = 0;
        for x in 0..size.x {
            for y in 0..size.y {
                for z in 0..size.z {
                    if grid.get(IVec3::new(x as i32, y as i32, z as i32)).material != MaterialId::AIR {
                        count += 1;
                    }
                }
            }
        }
        count
    }

    #[test]
    fn rolling_hill_slab_has_dirt() {
        assert!(count_non_air(&rolling_hill_slab(8, 8, 4)) > 0);
    }

    #[test]
    fn brick_wall_has_a_window_hole() {
        let grid = brick_wall_with_window(9, 9, 1);
        let size = grid.size();
        let center = IVec3::new((size.x / 2) as i32, (size.y / 2) as i32, 0);
        assert_eq!(grid.get(center).material, MaterialId::AIR);
        assert_eq!(grid.get(IVec3::new(0, 0, 0)).material, MaterialId::BRICK);
    }

    #[test]
    fn wood_branch_has_wood() {
        assert!(count_non_air(&wood_branch(10)) > 0);
    }

    #[test]
    fn mixed_hardness_block_has_both_materials() {
        let grid = mixed_hardness_block(8, 4, 4);
        assert_eq!(grid.get(IVec3::new(0, 0, 0)).material, MaterialId::DIRT);
        assert_eq!(grid.get(IVec3::new(7, 0, 0)).material, MaterialId::BRICK);
    }

    #[test]
    fn diagonal_saddle_pair_has_exactly_the_two_diagonal_voxels() {
        let grid = diagonal_saddle_pair();
        assert_eq!(count_non_air(&grid), 2);
        assert_eq!(grid.get(IVec3::new(1, 1, 1)).material, MaterialId::DIRT);
        assert_eq!(grid.get(IVec3::new(2, 1, 2)).material, MaterialId::DIRT);
        // The other two corners of the shared face must stay air, or this
        // isn't the ambiguous configuration anymore.
        assert_eq!(grid.get(IVec3::new(1, 1, 2)).material, MaterialId::AIR);
        assert_eq!(grid.get(IVec3::new(2, 1, 1)).material, MaterialId::AIR);
    }

    #[test]
    fn voxel_sphere_is_the_same_shape_regardless_of_material() {
        let dirt = voxel_sphere(4, MaterialId::DIRT);
        let brick = voxel_sphere(4, MaterialId::BRICK);
        assert!(count_non_air(&dirt) > 0);
        assert_eq!(count_non_air(&dirt), count_non_air(&brick));

        // Centre voxel is filled, a far corner of the bounding box isn't.
        assert_eq!(dirt.get(IVec3::new(4, 4, 4)).material, MaterialId::DIRT);
        assert_eq!(dirt.get(IVec3::new(0, 0, 0)).material, MaterialId::AIR);
    }
}

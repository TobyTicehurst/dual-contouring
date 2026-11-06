//! Debug/tuning flags. Recompiling to change these is fine for now (see
//! `docs/PLAN.md` milestone 12) — the `Resource` wrapper just keeps the door
//! open for a runtime debug UI later without another refactor.

use bevy::prelude::*;

// Spheres, cubes, and the real meshed surface are all independent and
// toggle-able at once, but only the meshed surface is on by default now
// that it exists — it's the thing these milestones are actually building
// toward. Debug spheres/cubes are still useful for cross-checking raw grid
// data against the meshed output, just opt-in rather than default-on.
pub const SHOW_DEBUG_SPHERES: bool = false;
/// Multiplier applied to a voxel's `weight` to get its debug-sphere radius.
/// `0.4` keeps a full-weight (`1.0`) voxel's sphere comfortably inside its
/// unit cube (half-extent `0.5`).
pub const DEBUG_SPHERE_RADIUS_SCALE: f32 = 0.4;

pub const SHOW_DEBUG_CUBES: bool = false;
pub const DEBUG_CUBE_MODE: DebugCubeMode = DebugCubeMode::Wireframe;
pub const DEBUG_CUBE_TRANSPARENT_ALPHA: f32 = 0.35;

/// A wireframe cube at every "active" Dual-Contouring cell (one with at
/// least one vertex placed inside it) — the dual grid itself, offset by
/// half a unit from the voxel debug cubes above. Off by default; useful for
/// seeing exactly which cell an ambiguous/split configuration lands in.
pub const SHOW_DEBUG_ACTIVE_CELLS: bool = true;
pub const DEBUG_ACTIVE_CELL_COLOR: Color = Color::srgb(0.1, 1.0, 1.0);

pub const SHOW_MESHED_SURFACE: bool = true;
/// Renders both sides of every meshed triangle instead of culling back
/// faces. Off by default (normal single-sided rendering); useful for
/// inspecting mesh winding issues — e.g. flying inside/behind the
/// diagonal-saddle crack demo, where the crack's far side would otherwise be
/// invisible.
pub const RENDER_BACKFACES: bool = false;

/// How the debug cube renderer draws each non-air voxel's unit cube.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugCubeMode {
    Solid,
    Transparent,
    Wireframe,
}

#[derive(Resource, Clone, Copy)]
pub struct DebugConfig {
    pub show_debug_spheres: bool,
    pub debug_sphere_radius_scale: f32,
    pub show_debug_cubes: bool,
    pub debug_cube_mode: DebugCubeMode,
    pub debug_cube_transparent_alpha: f32,
    pub show_debug_active_cells: bool,
    pub debug_active_cell_color: Color,
    pub show_meshed_surface: bool,
    pub render_backfaces: bool,
}

impl Default for DebugConfig {
    fn default() -> Self {
        Self {
            show_debug_spheres: SHOW_DEBUG_SPHERES,
            debug_sphere_radius_scale: DEBUG_SPHERE_RADIUS_SCALE,
            show_debug_cubes: SHOW_DEBUG_CUBES,
            debug_cube_mode: DEBUG_CUBE_MODE,
            debug_cube_transparent_alpha: DEBUG_CUBE_TRANSPARENT_ALPHA,
            show_debug_active_cells: SHOW_DEBUG_ACTIVE_CELLS,
            debug_active_cell_color: DEBUG_ACTIVE_CELL_COLOR,
            show_meshed_surface: SHOW_MESHED_SURFACE,
            render_backfaces: RENDER_BACKFACES,
        }
    }
}

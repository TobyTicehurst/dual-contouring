//! Engine-agnostic mesh buffers produced by meshers. Rendering code (see
//! `src/render/mesh_convert.rs`, once it exists) is responsible for turning
//! this into a `bevy::render::mesh::Mesh`; nothing in here knows Bevy exists.

use glam::Vec3;

use super::voxel::Color;

/// A triangle mesh as flat position/normal/colour/index buffers, matching
/// the shape Bevy (and most engines) expect a mesh's vertex attributes to
/// arrive in. `indices` is always a flat list of triangles:
/// `[a0, b0, c0, a1, b1, c1, ...]`.
#[derive(Debug, Clone, Default)]
pub struct MeshData {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    /// Per-vertex colour — lets a single continuous mesh show *which*
    /// material is where (e.g. a blend across a material boundary) instead
    /// of one flat colour for the whole mesh. See `docs/PLAN.md` Roadmap
    /// phase B.
    pub colors: Vec<Color>,
    pub indices: Vec<u32>,
}

impl MeshData {
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a triangle as three fresh vertices (no vertex sharing/welding).
    pub fn push_triangle(&mut self, positions: [Vec3; 3], normal: Vec3, colors: [Color; 3]) {
        let base = u32::try_from(self.positions.len()).expect("MeshData overflowed u32 indices");
        self.positions.extend(positions);
        self.normals.extend([normal; 3]);
        self.colors.extend(colors);
        self.indices.extend([base, base + 1, base + 2]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_triangle_keeps_colors_in_sync_with_positions() {
        let mut mesh = MeshData::new();
        let color = Color::rgb(1.0, 0.0, 0.0);

        mesh.push_triangle([Vec3::ZERO, Vec3::X, Vec3::Y], Vec3::Z, [color, color, color]);

        assert_eq!(mesh.colors.len(), mesh.positions.len());
        assert_eq!(mesh.colors, vec![color; 3]);
    }
}

//! Converts `core::MeshData` (Bevy-agnostic) into a real `bevy::Mesh`.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};

use crate::core::mesh_data::MeshData;

pub fn to_bevy_mesh(mesh_data: &MeshData) -> Mesh {
    let positions: Vec<[f32; 3]> = mesh_data.positions.iter().map(|p| p.to_array()).collect();
    let normals: Vec<[f32; 3]> = mesh_data.normals.iter().map(|n| n.to_array()).collect();
    // `core::voxel::Color`'s r/g/b are authored as sRGB (see `MaterialTable`),
    // same as `Color::srgb` elsewhere in this codebase, but
    // `Mesh::ATTRIBUTE_COLOR` is read by the shader as linear — convert
    // explicitly rather than feeding sRGB values in directly, which would
    // wash the blended colour out.
    let colors: Vec<[f32; 4]> = mesh_data
        .colors
        .iter()
        .map(|c| {
            let linear = Color::srgb(c.r, c.g, c.b).to_linear();
            [linear.red, linear.green, linear.blue, c.a]
        })
        .collect();

    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
        .with_inserted_indices(Indices::U32(mesh_data.indices.clone()))
}

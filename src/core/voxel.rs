//! Voxel and material data. No rendering-engine dependency lives here — see
//! `docs/DESIGN.md` for the reasoning behind the fields and invariants below.

/// Index into a [`MaterialTable`]. Materials are looked up by index rather
/// than matched on by name, so adding a new material is just a new table
/// entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MaterialId(pub u8);

impl MaterialId {
    /// `Air` is always index 0: it's the only non-opaque material today, it's
    /// the implicit value of any out-of-bounds voxel read, and every material
    /// table is expected to define it first.
    pub const AIR: MaterialId = MaterialId(0);

    // The remaining indices below are only meaningful paired with
    // `MaterialTable::standard()` — they name that table's fixed layout, not
    // some universal constant. A table built by other means is free to put
    // these materials wherever it likes.
    pub const DIRT: MaterialId = MaterialId(1);
    pub const BRICK: MaterialId = MaterialId(2);
    pub const WOOD: MaterialId = MaterialId(3);
}

/// A single voxel. See `docs/DESIGN.md` for the precise meaning of `weight`
/// and how it's used during meshing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Voxel {
    /// Fullness of the voxel, in `(0, 1]`. A "0% full" voxel is represented
    /// as `Air` instead of a `Voxel` with `weight = 0.0` — this invariant is
    /// enforced here, at construction, not by downstream meshing code.
    pub weight: f32,
    pub material: MaterialId,
}

impl Voxel {
    /// The filler value for any voxel that isn't explicitly authored — also
    /// what out-of-bounds `VoxelGrid` reads return. The weight value is
    /// arbitrary (air's weight is never read by meshing) but still has to
    /// satisfy the same `(0, 1]` invariant every voxel does.
    pub const AIR: Voxel = Voxel {
        weight: 1.0,
        material: MaterialId::AIR,
    };

    pub fn new(weight: f32, material: MaterialId) -> Self {
        debug_assert!(
            weight > 0.0 && weight <= 1.0,
            "voxel weight must be in (0, 1], got {weight}"
        );
        Self { weight, material }
    }
}

/// Flat RGBA colour, kept local to `core` so material definitions don't need
/// to depend on a rendering engine's colour type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }
}

/// Static properties of a material, shared by every voxel of that material.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MaterialProperties {
    pub name: &'static str,
    /// Whether this material counts as "solid" for sign/cell-activity
    /// purposes during meshing. `false` only for `Air` today; named to leave
    /// room for a future translucent category rather than being read as a
    /// strict opposite of "transparent" (see `docs/DESIGN.md`).
    pub opaque: bool,
    /// `0.0` = fully smooth (dirt), `1.0` = fully rectilinear (brick).
    pub hardness: f32,
    /// Whether two voxels of this material that touch only diagonally (no
    /// shared face) should still mesh as one connected surface, instead of
    /// each getting their own separate patch. Meshing normally treats a
    /// cell's opaque corners as connected only when a path of real grid
    /// edges between them stays opaque the whole way (see
    /// `meshing::common::opaque_components`) — correct in general, but wrong
    /// for materials meant to read as thin connected structures (`wood`'s
    /// branches) where a diagonal step is the norm, not an ambiguous edge
    /// case. `true` only for `wood` today.
    pub diagonal_bridging: bool,
    pub color: Color,
}

/// Indexed table of material properties. `MaterialId(i)` names row `i`.
#[derive(Debug, Clone)]
pub struct MaterialTable {
    materials: Vec<MaterialProperties>,
}

impl MaterialTable {
    /// The four materials named in the project brief, in a fixed order with
    /// `Air` at index 0.
    pub fn standard() -> Self {
        let mut table = MaterialTable {
            materials: Vec::new(),
        };
        table.push(MaterialProperties {
            name: "air",
            opaque: false,
            hardness: 0.0,
            diagonal_bridging: false,
            color: Color::rgb(0.0, 0.0, 0.0),
        });
        table.push(MaterialProperties {
            name: "dirt",
            opaque: true,
            hardness: 0.0,
            diagonal_bridging: false,
            color: Color::rgb(0.45, 0.31, 0.19),
        });
        table.push(MaterialProperties {
            name: "brick",
            opaque: true,
            hardness: 1.0,
            diagonal_bridging: false,
            color: Color::rgb(0.65, 0.24, 0.20),
        });
        table.push(MaterialProperties {
            name: "wood",
            opaque: true,
            hardness: 0.5,
            diagonal_bridging: true,
            color: Color::rgb(0.36, 0.25, 0.14),
        });
        table
    }

    /// Appends a new material and returns the `MaterialId` it was assigned.
    pub fn push(&mut self, properties: MaterialProperties) -> MaterialId {
        let id = MaterialId(
            u8::try_from(self.materials.len()).expect("MaterialTable overflowed u8 indices"),
        );
        self.materials.push(properties);
        id
    }

    pub fn get(&self, id: MaterialId) -> &MaterialProperties {
        &self.materials[id.0 as usize]
    }
}

//! Chunk-level LOD boundary stitching (`docs/PLAN.md` Roadmap phase C).
//!
//! **Status: not yet working — not wired into `render::chunk_mesh_view`.**
//! `transition_vertex` (the per-footprint merged-vertex computation),
//! `fine_override_cells`/`coarse_override_cell`, and
//! `common::mesh_with_overrides` are each independently implemented and
//! tested, but the end-to-end proof
//! (`tests::fine_and_coarse_chunks_mesh_watertight_across_a_2_to_1_boundary`)
//! still fails and is left `#[ignore]`-d with the specific unresolved
//! symptom documented on it, rather than wired into real rendering. See
//! `docs/PLAN.md` Roadmap phase C for the full status.
//!
//! Two chunks meshed at different resolutions each already produce a
//! complete, correct surface at their shared boundary — every chunk always
//! reads real neighbouring-chunk voxel data via `ChunkView`, regardless of
//! its own LOD stride. The seam isn't a hole to fill; it's two
//! independently-complete surfaces at different fidelity. Bridging them
//! with new triangles after the fact (an earlier attempt at this module)
//! either duplicates geometry or leaves edges with no matching partner.
//!
//! The actual fix: treat the boundary region as **one cell that produces
//! one vertex**, the same way every other cell in this mesher already
//! does, and have both chunks' otherwise-completely-normal scans reference
//! *that* vertex instead of computing their own independent one there (via
//! `common::mesh_with_overrides`). Since both sides compute the vertex from
//! the same real voxel data through the same pure function, they get
//! bit-identical results without anything being passed between chunks —
//! the same principle that already makes `voxel_source::DownsampledView`
//! and cross-chunk boundary reads work.
//!
//! The sample layout (13 real points per coarse-resolution footprint: a 3×3
//! fine-resolution grid on the boundary plane, plus the coarse cell's own
//! other 4 corners one `coarse_stride` further back) mirrors the classic
//! Transvoxel transition cell, adapted from its per-cell Marching-Cubes
//! triangle table to this mesher's "one vertex = mean of edge crossings"
//! model instead.
//!
//! **Known limitations** (see `docs/PLAN.md`): a transition cell always
//! produces exactly one vertex from the mean of all its crossings — the
//! rarer saddle-point case that `common::opaque_components` splits into
//! multiple vertices for regular cells isn't specially handled here, so a
//! saddle sitting exactly on an LOD boundary may show a small residual
//! crack. Placement always uses the plain averaged (surface-nets-style)
//! rule regardless of which mesher the chunk uses, since axis-aligned/
//! hardness-blend placement assumes a regular unit cube. `chunk::balance_lod_levels`
//! guarantees a chunk has at most one transitioning face, so only the
//! face-adjacent (not edge/corner) case is handled at all.

use glam::{IVec3, Vec3};

use crate::core::voxel::{Color, MaterialTable};
use crate::core::voxel_source::VoxelSource;

use super::common::Axis;

fn point_at(axis: Axis, axis_value: i32, t1: Axis, t1_value: i32, t2: Axis, t2_value: i32) -> IVec3 {
    axis.unit() * axis_value + t1.unit() * t1_value + t2.unit() * t2_value
}

/// A transitioning face's own boundary layer has a perimeter too — wherever
/// *another* chunk boundary (even an ordinary same-resolution one) crosses
/// it, that other boundary's own "one ring beyond" cells (the same
/// mechanism that lets `ChunkView` read neighbour data at all) sit right on
/// the transitioning layer, and need the *same* shared-vertex treatment as
/// the face's regular interior — otherwise the fine and coarse sides
/// independently compute mismatched vertices there too, recreating the
/// exact problem this module exists to fix, just one ring over. A tangential
/// axis is therefore either `Regular` (the face's own interior, subdivided
/// between fine and coarse resolution) or `Ring` (one of the chunk's own
/// other-axis boundary rings, at this axis's `-1` or `size` position — not
/// subdivided at all, since a ring is always exactly one cell wide on
/// *both* sides, just a different real width each side's own resolution).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TangentialSpan {
    /// A regular coarse-resolution footprint index along this axis.
    Regular(i32),
    /// This axis's own `-1` ring — i.e. the unit-width cell spanning
    /// `[-1, 0]`, the same one `ChunkView`'s "one ring past size" mechanism
    /// always makes available.
    RingLow,
    /// This axis's own `size` ring — the unit-width cell spanning
    /// `[size - 1, size]`.
    RingHigh,
}

impl TangentialSpan {
    /// The tangential positions to sample on the *near* (boundary) plane —
    /// 3 fine-resolution points spanning one coarse-cell-width for
    /// `Regular`. For `Ring*`, this isn't actually a "near vs. far" split at
    /// all (that distinction only applies to the transitioning axis itself)
    /// — it's the ring cell's own natural unit-width span, `[-1, 0]` or
    /// `[size-1, size]`, sampled identically on both the near and far
    /// planes, since a ring cell doesn't get subdivided or extended by
    /// anything to do with the coarse/fine split.
    fn near_offsets(self, fine_stride: i32, coarse_stride: i32, chunk_size: i32) -> Vec<i32> {
        match self {
            TangentialSpan::Regular(idx) => {
                let base = idx * coarse_stride;
                vec![base, base + fine_stride, base + coarse_stride]
            }
            TangentialSpan::RingLow => vec![-1, 0],
            TangentialSpan::RingHigh => vec![chunk_size - 1, chunk_size],
        }
    }

    /// The tangential positions to sample on the *far* (one coarse-cell
    /// further back) plane — the 2 corners of `Regular`'s span, or the same
    /// ring span as `near_offsets` for `Ring*` (see its docs).
    fn far_offsets(self, coarse_stride: i32, chunk_size: i32) -> Vec<i32> {
        match self {
            TangentialSpan::Regular(idx) => {
                let base = idx * coarse_stride;
                vec![base, base + coarse_stride]
            }
            TangentialSpan::RingLow => vec![-1, 0],
            TangentialSpan::RingHigh => vec![chunk_size - 1, chunk_size],
        }
    }

    /// The fine chunk's own (`DownsampledView`-by-`fine_stride`) cell
    /// coordinates along this axis to override — 2 sub-cells for `Regular`,
    /// or the single fine-resolution ring cell for `Ring*`.
    fn fine_cells(self, fine_stride: i32, chunk_size: i32) -> Vec<i32> {
        match self {
            TangentialSpan::Regular(idx) => vec![idx * 2, idx * 2 + 1],
            TangentialSpan::RingLow => vec![-1],
            TangentialSpan::RingHigh => vec![chunk_size / fine_stride],
        }
    }

    /// The coarse neighbour's own (`DownsampledView`-by-`coarse_stride`)
    /// cell coordinate along this axis — its own footprint index for
    /// `Regular`, or its own (coarser-resolution) ring cell for `Ring*`.
    fn coarse_cell(self, coarse_stride: i32, chunk_size: i32) -> i32 {
        match self {
            TangentialSpan::Regular(idx) => idx,
            TangentialSpan::RingLow => -1,
            TangentialSpan::RingHigh => chunk_size / coarse_stride,
        }
    }
}

/// Computes the shared vertex for one footprint of a transitioning face —
/// either its regular interior or a piece of its perimeter, depending on
/// `t1_span`/`t2_span` (see `TangentialSpan`) — or `None` if it isn't active
/// (every sample point agrees in opaqueness). `raw_view` must be the fine
/// chunk's own real (stride-1) voxel data (a `ChunkView`) — both chunks on
/// either side of the boundary call this identically (same real data, same
/// axis/direction, same spans) to independently arrive at the same result;
/// see `fine_override_cells`/`coarse_override_cell` for where each side
/// installs it.
///
/// `axis`/`positive_direction` name the *fine* chunk's own face (matching
/// `common::BoundarySuppression`'s naming, even though suppression itself
/// isn't used by this mechanism). `coarse_stride` must be exactly double the
/// fine chunk's own stride — the only ratio `chunk::balance_lod_levels`
/// ever allows between face-adjacent chunks.
pub fn transition_vertex(
    raw_view: &dyn VoxelSource,
    materials: &MaterialTable,
    chunk_size: u32,
    axis: Axis,
    positive_direction: bool,
    t1_span: TangentialSpan,
    t2_span: TangentialSpan,
    coarse_stride: u32,
) -> Option<(Vec3, Color)> {
    let (t1, t2) = axis.others();
    let fine_stride = (coarse_stride / 2) as i32;
    let coarse_stride = coarse_stride as i32;
    let chunk_size = chunk_size as i32;
    let boundary = if positive_direction { chunk_size } else { 0 };
    // The coarse neighbour's own boundary-ring cell reaches backward, into
    // the fine chunk's own territory, by one coarse-cell-width — the same
    // ring `VoxelWorld::view` already uses for every chunk boundary. This
    // "far" plane is *not* further into the coarse chunk's own interior.
    let outward = if positive_direction { 1 } else { -1 };
    let far_axis = boundary - outward * coarse_stride;

    let t1_near = t1_span.near_offsets(fine_stride, coarse_stride, chunk_size);
    let t2_near = t2_span.near_offsets(fine_stride, coarse_stride, chunk_size);
    let t1_far = t1_span.far_offsets(coarse_stride, chunk_size);
    let t2_far = t2_span.far_offsets(coarse_stride, chunk_size);
    let (nm, nn) = (t1_near.len(), t2_near.len());
    let (fm, fn_) = (t1_far.len(), t2_far.len());

    let near = |i: usize, j: usize| point_at(axis, boundary, t1, t1_near[i], t2, t2_near[j]);
    let far = |fi: usize, fj: usize| point_at(axis, far_axis, t1, t1_far[fi], t2, t2_far[fj]);

    // Adjacency (analogous to `common::CUBE_EDGES`, but for this non-cube,
    // variably-sized shape): the near grid's own unit-step edges, "through"
    // edges connecting each far corner to the near-grid corner directly in
    // front of it, and the far grid's own unit-step edges. A `Ring` axis
    // collapses its grid to a single row/column/point, which just makes
    // some of these coincide (e.g. a "through" edge with equal endpoints) —
    // naturally inert below, since equal endpoints can never be sign-
    // changing, not a case that needs its own separate handling.
    let mut edges: Vec<(IVec3, IVec3)> = Vec::new();
    for i in 0..nm.saturating_sub(1) {
        for j in 0..nn {
            edges.push((near(i, j), near(i + 1, j)));
        }
    }
    for i in 0..nm {
        for j in 0..nn.saturating_sub(1) {
            edges.push((near(i, j), near(i, j + 1)));
        }
    }
    for fi in 0..fm {
        for fj in 0..fn_ {
            let ni = if fi == 0 { 0 } else { nm - 1 };
            let nj = if fj == 0 { 0 } else { nn - 1 };
            edges.push((far(fi, fj), near(ni, nj)));
        }
    }
    for fi in 0..fm.saturating_sub(1) {
        for fj in 0..fn_ {
            edges.push((far(fi, fj), far(fi + 1, fj)));
        }
    }
    for fi in 0..fm {
        for fj in 0..fn_.saturating_sub(1) {
            edges.push((far(fi, fj), far(fi, fj + 1)));
        }
    }

    let is_opaque = |pos: IVec3| materials.get(raw_view.get(pos).material).opaque;

    // Position: mean of every sign-changing edge's crossing, using the same
    // interpolation formula `common::build_cell`/`build_cell_group` use
    // (already generic over the two endpoints' distance, not just
    // unit-cube edges).
    let mut crossing_sum = Vec3::ZERO;
    let mut crossing_count: u32 = 0;
    for &(a, b) in &edges {
        let (opaque_a, opaque_b) = (is_opaque(a), is_opaque(b));
        if opaque_a == opaque_b {
            continue;
        }
        let (opaque_pos, other_pos) = if opaque_a { (a, b) } else { (b, a) };
        let weight = raw_view.get(opaque_pos).weight;
        crossing_sum += opaque_pos.as_vec3() + (other_pos - opaque_pos).as_vec3() * (0.5 * weight);
        crossing_count += 1;
    }
    if crossing_count == 0 {
        return None;
    }

    // Colour: the weight-averaged colour of every opaque sample point, the
    // same rule `build_cell` uses over a regular cell's 8 corners.
    let mut weighted_color_sum = (0.0f32, 0.0f32, 0.0f32);
    let mut weight_sum = 0.0f32;
    for i in 0..nm {
        for j in 0..nn {
            accumulate_color(raw_view, materials, near(i, j), &mut weighted_color_sum, &mut weight_sum);
        }
    }
    for fi in 0..fm {
        for fj in 0..fn_ {
            accumulate_color(raw_view, materials, far(fi, fj), &mut weighted_color_sum, &mut weight_sum);
        }
    }
    let color = Color {
        r: weighted_color_sum.0 / weight_sum,
        g: weighted_color_sum.1 / weight_sum,
        b: weighted_color_sum.2 / weight_sum,
        a: 1.0,
    };

    Some((crossing_sum / crossing_count as f32, color))
}

fn accumulate_color(
    raw_view: &dyn VoxelSource,
    materials: &MaterialTable,
    pos: IVec3,
    weighted_color_sum: &mut (f32, f32, f32),
    weight_sum: &mut f32,
) {
    let voxel = raw_view.get(pos);
    let properties = materials.get(voxel.material);
    if properties.opaque {
        weighted_color_sum.0 += voxel.weight * properties.color.r;
        weighted_color_sum.1 += voxel.weight * properties.color.g;
        weighted_color_sum.2 += voxel.weight * properties.color.b;
        *weight_sum += voxel.weight;
    }
}

/// The fine chunk's own (`DownsampledView`-by-`fine_stride`) cell
/// coordinates to override for this footprint — 1, 2, or 4 fine sub-cells
/// depending on whether `t1_span`/`t2_span` are `Regular` or `Ring`, all
/// sharing `transition_vertex`'s result.
pub fn fine_override_cells(
    axis: Axis,
    positive_direction: bool,
    t1_span: TangentialSpan,
    t2_span: TangentialSpan,
    chunk_size: u32,
    fine_stride: u32,
) -> Vec<IVec3> {
    let (t1, t2) = axis.others();
    let fine_size = (chunk_size / fine_stride) as i32;
    let layer = if positive_direction { fine_size - 1 } else { 0 };
    let t1_cells = t1_span.fine_cells(fine_stride as i32, chunk_size as i32);
    let t2_cells = t2_span.fine_cells(fine_stride as i32, chunk_size as i32);
    let mut cells = Vec::with_capacity(t1_cells.len() * t2_cells.len());
    for &i in &t1_cells {
        for &j in &t2_cells {
            cells.push(point_at(axis, layer, t1, i, t2, j));
        }
    }
    cells
}

/// The coarse neighbour's own (`DownsampledView`-by-`coarse_stride`)
/// boundary-ring cell coordinate for this footprint — the single cell on
/// its side that shares `transition_vertex`'s result. `positive_direction`
/// is still named from the *fine* chunk's own face (matching every other
/// parameter here) — the coarse neighbour sits on the fine chunk's `+`
/// side, which means the fine chunk sits on the coarse neighbour's `-`
/// side, so the coarse chunk's own ring cell uses the *opposite* sense of
/// `VoxelWorld::view`'s ring convention (`size - 1` vs. `-1`) from what the
/// fine chunk itself uses.
pub fn coarse_override_cell(
    axis: Axis,
    positive_direction: bool,
    t1_span: TangentialSpan,
    t2_span: TangentialSpan,
    chunk_size: u32,
    coarse_stride: u32,
) -> IVec3 {
    let (t1, t2) = axis.others();
    let coarse_size = (chunk_size / coarse_stride) as i32;
    let layer = if positive_direction { -1 } else { coarse_size - 1 };
    let t1_cell = t1_span.coarse_cell(coarse_stride as i32, chunk_size as i32);
    let t2_cell = t2_span.coarse_cell(coarse_stride as i32, chunk_size as i32);
    point_at(axis, layer, t1, t1_cell, t2, t2_cell)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::chunk::{ChunkCoord, VoxelWorld};
    use crate::core::meshing::common::{count_non_manifold_edges, mesh_with_overrides, BoundarySuppression};
    use crate::core::meshing::surface_nets;
    use crate::core::voxel::{MaterialId, Voxel};

    /// A footprint straddling a real dirt/air boundary should produce a
    /// vertex; a footprint whose 13 sample points all agree in opaqueness
    /// (an empty world: everything air) should not.
    #[test]
    fn transition_vertex_is_only_present_where_the_footprint_is_active() {
        let materials = MaterialTable::standard();

        // +Y face, coarse footprint (0,0), coarse_stride=2: "near" plane is
        // y=8 (the chunk's own boundary, left as air — never set), "far"
        // plane is y=6. Filling just the far plane's tangential extent
        // (x,z in 0..3, covering both the near grid's and far corners'
        // range) makes far solid, near air: a real crossing via the
        // "through" edges.
        let mut active_world = VoxelWorld::new(8);
        for x in 0..3 {
            for z in 0..3 {
                active_world.set(IVec3::new(x, 6, z), Voxel::new(1.0, MaterialId::DIRT));
            }
        }
        let active_view = active_world.view(ChunkCoord(IVec3::new(0, 0, 0)));
        let regular = TangentialSpan::Regular(0);
        let active = transition_vertex(&active_view, &materials, 8, Axis::Y, true, regular, regular, 2);
        assert!(active.is_some(), "a footprint with a real solid/air crossing should be active");

        // An entirely empty world: every one of the 13 points reads Air, so
        // there's no sign-changing edge anywhere in the footprint.
        let empty_world = VoxelWorld::new(8);
        let empty_view = empty_world.view(ChunkCoord(IVec3::new(0, 0, 0)));
        let inactive = transition_vertex(&empty_view, &materials, 8, Axis::Y, true, regular, regular, 2);
        assert!(inactive.is_none(), "a footprint with no sign changes at all should be inactive");
    }

    /// A `Ring` span's own perimeter footprint should also detect a real
    /// crossing — the case increment 2's first attempt missed: where a
    /// transitioning face's edge meets another (even ordinary, same-
    /// resolution) boundary of the same chunk, like the bottom of a slab.
    #[test]
    fn ring_span_footprints_are_also_active_where_they_cross_real_geometry() {
        let materials = MaterialTable::standard();
        // Solid at y=-1 (below the chunk's own regular content, an
        // overhang-like shape), just at z=0 — everywhere the footprint's
        // "near"/"far" X positions (6, 7, 8) reach, so there's a real
        // crossing purely within the RingLow(Y) x Regular(Z) footprint,
        // without needing any y=0 content at all.
        let mut world = VoxelWorld::new(8);
        for x in 6..9 {
            world.set(IVec3::new(x, -1, 0), Voxel::new(1.0, MaterialId::DIRT));
        }
        let view = world.view(ChunkCoord(IVec3::new(0, 0, 0)));

        // Axis::X face, t1=Y (RingLow: the y=-1 ring), t2=Z (Regular(0)).
        let vertex = transition_vertex(&view, &materials, 8, Axis::X, true, TangentialSpan::RingLow, TangentialSpan::Regular(0), 2);
        assert!(vertex.is_some(), "a Ring-spanning footprint crossing real geometry should be active");
    }

    /// The core proof this module is meant to provide: two chunks straddling
    /// a boundary-crossing shape, one forced to full resolution, its
    /// neighbour to half — mesh both via `mesh_with_overrides` (no
    /// suppression at all, just each side's transition-vertex overrides),
    /// merge, and the result should be watertight despite the resolution
    /// mismatch.
    ///
    /// **Currently failing — known, unresolved issue, not yet a documented
    /// limitation.** Debugging narrowed this down a long way (see
    /// `docs/PLAN.md` Roadmap phase C): `transition_vertex` and the fine/
    /// coarse override *do* correctly share bit-identical vertices (verified
    /// directly), and the perimeter (`TangentialSpan::Ring*`) fix resolved
    /// the mismatch between fine and coarse sides specifically. What
    /// remains is a connectivity gap *within a single side's own mesh*,
    /// where geometry adjacent to two different override footprints (e.g. a
    /// regular interior footprint next to a perimeter-ring footprint)
    /// doesn't connect the way manual analysis of `scan_edge`/
    /// `get_or_build_vertex` predicts it should — a specific edge that
    /// opacity data confirms should be produced by an active,
    /// correctly-scanned quad simply doesn't appear in the output, for a
    /// reason not yet identified. Left `#[ignore]` rather than deleted so
    /// the next attempt starts from a known, reproducible failure instead
    /// of from scratch.
    #[test]
    #[ignore = "known unresolved bug: see this test's doc comment and docs/PLAN.md Roadmap phase C"]
    fn fine_and_coarse_chunks_mesh_watertight_across_a_2_to_1_boundary() {
        let chunk_size = 8;
        let mut world = VoxelWorld::new(chunk_size);
        // A flat slab (y=0 only), spanning both chunks' full x and z
        // extent, so there's no side wall in Z and only a top/bottom cap
        // plus the shared X boundary to worry about.
        for x in 0..16 {
            for z in 0..8 {
                world.set(IVec3::new(x, 0, z), Voxel::new(1.0, MaterialId::DIRT));
            }
        }

        let materials = MaterialTable::standard();
        let fine_coord = ChunkCoord(IVec3::new(0, 0, 0));
        let coarse_coord = ChunkCoord(IVec3::new(1, 0, 0));
        let fine_stride = 1;
        let coarse_stride = 2;
        let footprints = (chunk_size / coarse_stride) as i32;

        // Every tangential span along this face: the regular interior
        // footprints, plus the RingLow/RingHigh perimeter on each axis —
        // together covering the whole boundary layer, interior and edges
        // alike (see `TangentialSpan`'s docs for why the perimeter needs
        // the same treatment as the interior).
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
                    transition_vertex(&fine_raw_view, &materials, chunk_size, Axis::X, true, t1_span, t2_span, coarse_stride)
                else {
                    continue;
                };
                for cell in fine_override_cells(Axis::X, true, t1_span, t2_span, chunk_size, fine_stride) {
                    fine_overrides.push((cell, position, color));
                }
                let coarse_cell = coarse_override_cell(Axis::X, true, t1_span, t2_span, chunk_size, coarse_stride);
                // `transition_vertex` returns a real (un-downsampled)
                // position; the coarse chunk's own mesh is built against a
                // `DownsampledView` at `coarse_stride`, which works in
                // downsampled-local coordinates (real / coarse_stride) —
                // its own `Transform` scale corrects it back afterward, the
                // same way its normal cells' vertices already do.
                coarse_overrides.push((coarse_cell, position / coarse_stride as f32, color));
            }
        }

        let fine_mesh = mesh_with_overrides(
            &fine_raw_view,
            &materials,
            surface_nets::place_vertex,
            BoundarySuppression::default(),
            &fine_overrides,
        );
        let coarse_raw_view = world.view(coarse_coord);
        let coarse_downsampled = crate::core::voxel_source::DownsampledView::new(&coarse_raw_view, coarse_stride);
        let coarse_mesh = mesh_with_overrides(
            &coarse_downsampled,
            &materials,
            surface_nets::place_vertex,
            BoundarySuppression::default(),
            &coarse_overrides,
        );

        let mut merged = crate::core::mesh_data::MeshData::new();
        let fine_offset = world.chunk_origin(fine_coord).as_vec3();
        merged.positions.extend(fine_mesh.positions.iter().map(|&p| p + fine_offset));
        merged.normals.extend(&fine_mesh.normals);
        merged.indices.extend(&fine_mesh.indices);

        let coarse_offset = world.chunk_origin(coarse_coord).as_vec3();
        let base = merged.positions.len() as u32;
        merged.positions.extend(coarse_mesh.positions.iter().map(|&p| p * coarse_stride as f32 + coarse_offset));
        merged.normals.extend(&coarse_mesh.normals);
        merged.indices.extend(coarse_mesh.indices.iter().map(|&i| i + base));

        assert_eq!(
            count_non_manifold_edges(&merged),
            0,
            "a 2:1 chunk boundary should mesh watertight via shared transition vertices, not crack"
        );
    }

    /// When both sides of a boundary already share the same resolution, no
    /// transition vertices are computed at all — confirms the existing
    /// (Phase A) equal-resolution watertightness behaviour is untouched.
    #[test]
    fn equal_resolution_neighbours_need_no_transition_vertices() {
        let chunk_size = 4;
        let mut world = VoxelWorld::new(chunk_size);
        for x in 2..6 {
            for y in 0..2 {
                for z in 0..2 {
                    world.set(IVec3::new(x, y, z), Voxel::new(1.0, MaterialId::DIRT));
                }
            }
        }
        let materials = MaterialTable::standard();
        let mut merged = crate::core::mesh_data::MeshData::new();

        for coord in [ChunkCoord(IVec3::new(0, 0, 0)), ChunkCoord(IVec3::new(1, 0, 0))] {
            let view = world.view(coord);
            let mesh = surface_nets::mesh(&view, &materials);
            let offset = world.chunk_origin(coord).as_vec3();
            let base = merged.positions.len() as u32;
            merged.positions.extend(mesh.positions.iter().map(|&p| p + offset));
            merged.normals.extend(&mesh.normals);
            merged.indices.extend(mesh.indices.iter().map(|&i| i + base));
        }

        assert_eq!(count_non_manifold_edges(&merged), 0);
    }
}

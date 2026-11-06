# Notes

Early research notes and in-progress thoughts, combined from two separate files. Both are incomplete.

## Context

The goal is to create a voxel meshing algorithm for destructible and reconstructible objects, similar to Minecraft. I would like to be able to create meshes with more detail and features than cube-based meshing. I would like to be able to generate:



 - Each voxel has a material
 - Each material should have a list of properties. Examples of properties:
  - Transparent (air), has no visible surface mesh but causes other materials next to the transparent material to generate a surface mesh
  - Translucent (water), has a visible surface mesh next to air and also causes opaque materials next to the translucent material to generate a surface mesh. Different translucent materials which are adjacent should generate surface meshes (an assumption about non-equal refractive indices is being made here).
  - Opaque (stone), has a visible surface mesh next to transparent and translucent materials. Different opaque materials which are adjacent should not generate surface meshes.
  - Others (which I haven't thought through yet) which allow for the geometric features I would like, for example, a "hardness" to allow a material to have sharper edges, a "treelike" to allow long, thin, branches.

1. Terrain
 - should be smooth but also have cliffs with right angles or steeper
 - different materials should mesh with no seam
 - the visible volume added to the mesh by a given voxel should be roughly proportional to its "amount". An amount of zero and the voxel becomes air. A voxel with a small amount next to air should only add a small volume to the mesh. A voxel completely surrounded by non-air and non-transparent should be 
 - the boundary between 

### Algorithms

#### Cubes

Minecraft style, draw a cube at each voxel. Doesn't show the amount of a voxel and can't show detailed features such as slopes.

#### Marching Cubes

Performs a lookup for each "cube" of 8 voxels. With a scalar field only a 45 degree slope can be shown and the right angles are aligned at a 45 degree rotation compared to cubes (produces octahedrons rather than cubes). Using linear interpolation between voxels, the amount of the voxel can be shown.

There are 2 ambiguities with marching cubes, cube face ambiguity and interior ambiguity. Cube face ambiguity happens when the 4 voxels on a face of the cube have alternating signs (signs are materials/air in this project). This can lead to discontinuity in the surface mesh, however, it can be solved by adding global rules about how to solve these cases. This solution may not be sufficient since I would like to build a new algorithm which (adds extra vertices to the surface mesh) on top of the Marching Cubes algorithm, which may not be compatible. For this reason I would like to explore marching tetrahedra as an alternate solution. The second ambiguity, interior ambiguity, came up when trying to generate the lookup table from first principles. When triangulating the vertices on the cube there are multiple options (see polygonizing). I solved this by rotating the cube (and flipping all 8 signs) each time I created a lookup table solution so they would all be the same triangulation. I can't see an easy way to extend this when adding extra vertices though so not sure how to solve this just yet.

#### Marching Tetrahedra

Works similarly to Marching Cubes but splits the cube up into 6 tetrahedra. This removes the ambiguity in the Marching Cubes case but I am not sure if it does when I add an extra vertex to the surface mesh. There also exists Marching Diamond Lattice Cells which looks quite complex to visualise.

#### Asymptotic Decider

An extension to Marching Cubes which solves both ambiguity issues. I would like to create a visualiser in 2D (cube faces/squares) and 3D which shows the isolines (hyperbolae) and their asymptotes, to better understand what is going on visually and to see if I can extend the solution to my added vertex algorithm

#### Cubic Marching Squares

Cubical Marching Squares uses Hermite Data which contains normal vectors. While I don't intend to store normal vectors, I may use different data to place the extra vertices. Cubical Marching Squares first considers each of the 6 faces of the cube. Each face runs marching squares to obtain the vertices, but before connecting these vertices with lines a new vertex may be added. The normals and tangents at each of the vertices are calculated from the Hermite data and the intersection of each of the tangents is found (if the normals don't point in the same direction). A new vertex is added at the intersection point of the tangents. The faces are then recombined to form one or more segment loops. The normals and tangents (planes in 3D) are once again calculated and the intersection of the tangents is found. If this intersection is a single point, a vertex is added here. If the intersection of the tangent planes is a line then some fancy averaging is done to get a single point. If there is no intersection (normals are pointing in the same direction) then no vertex is added. A triangle fan is then formed around this extra vertex using the segment loop.

I would like to be able to vary the position of these extra vertices depending on certain material properties. By placing the extra vertices by calculating the average position of existing vertices, I think I can reproduce marching cubes with extra steps (and higher triangle count). By placing the extra vertices always assuming normals are in the direction of the sign change, I think I can reproduce the cubes algorithm. The idea is to use a material property (like hardness) to linearly interpolate between these 2 points.

#### Surface Nets

Surface nets is similar to Marching Cubes but places vertices in each cube which has a sign change rather than on each edge with a sign change. This results in fewer triangles and a smoother mesh, a positive overall, so long as it can be adapted to show sharp features when needed.

#### Dual Contouring



- Asymptotic decider
- Marching Cubes 33
- Marching Squares

### Terminology

 - Amount - a property of a voxel. The amount of a voxel should be roughly proportional to the volume added to the mesh. For a voxel surrounded by air, using marching cubes, a small amount would produce a small octahedron while a large amount would produce a large octahedron. For a voxel surrounded by the same material, amount won't have any effect since no surface mesh will be generated for this voxel.
 - Seam - a gap or a divot in a mesh. A mesh is "seamless" when every boundary is smooth, with a natural transition. For example, in Minecraft, a dirt and sand block next to each other will have no gap between them and the transition is flat. Using marching cubes independently on the dirt and sand would generate 2 octahedrons joined by a single vertex; this would be a seam. Seams may appear at the boundary between LODs, between 2 different materials or be caused by a voxel meshing artifact.

## Thoughts

### Data Storage - Chunks and Sparse Octrees

 - A Voxel is 1x1x1 world coordinates
 - A Chunk is 32x32x32 world coordinates
 - Chunks are stored in a HashMap, alongside their world coordinates
 - Chunks contain a Sparse Octree

### Algorithm

- Based on section III of this paper: https://people.engr.tamu.edu/schaefer/research/dualsimp_tvcg.pdf, which is based of this paper: http://vis.computer.org/vis2004/DVD/vis/papers/nielson2.pdf
- Instead of using Dual Contouring (DC) for vertex placement rules, instead use the following vertex placement rules.

### Vertex Placement Rules

1. Consider each Voxel which is "related" to the Vertex in a given Cell. There may be multiple Vertices in a Cell, each Voxel in that Cell must be related to exactly 1 Vertex.

 

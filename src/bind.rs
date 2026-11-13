//! Layer 2: the uvar normal-binding policy.
//!
//! Turns a loss-preserving [`Obj`] into something a renderer can shade:
//! positions, one unit normal per position, and triangle indices. This is where
//! a normal stops being a table entry and becomes a property of a surface point,
//! and the rules that decide it are the reason this crate exists — they were
//! learned the hard way, on a comparison that lied.
//!
//! 1. A `vn` binds to the **position that references it**, by index, in face
//!    order; the first referencing corner wins. Nothing binds by arrival order:
//!    a flat-shaded export indexes `vn` differently from `v`, and a corner's
//!    `b` in `a/b/c` is a texture coordinate, never a normal.
//! 2. A `vn` index that does not address the normal table binds to nothing. The
//!    corner is unbound, not an error.
//! 3. Unbound positions get area-weighted geometric normals: each triangle adds
//!    its unnormalized `(b − a) × (c − a)` to all three corners, accumulated in
//!    triangle order so the float sum is reproducible.
//! 4. Everything is then divided by `max(‖n‖, 1e-12)`. Exported normals are not
//!    always unit, and a genuine zero stays exactly zero instead of becoming
//!    NaN — the Stanford dragon really does have zero-area vertices.
//!
//! Binding is **total**: there is no error path.

use crate::model::Obj;

/// A mesh ready for shading: `positions`, `unit_normals` parallel to them, and
/// 0-based triangle indices.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BoundMesh {
    /// One `[x, y, z]` per [`Obj::vertices`] entry, same order.
    pub positions: Vec<[f64; 3]>,
    /// One unit `[x, y, z]` per position. A vertex no face references, or one
    /// whose incident faces enclose no area, is exactly `[0.0, 0.0, 0.0]`.
    pub unit_normals: Vec<[f64; 3]>,
    /// One `[i, j, k]` per triangle, indexing `positions`/`unit_normals`.
    pub triangles: Vec<[u32; 3]>,
}

impl BoundMesh {
    /// Number of positions.
    #[must_use]
    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    /// Number of triangles.
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.triangles.len()
    }

    /// True when the normal at `vertex` is exactly zero: the mesh has no
    /// orientation there (a folded, or unreferenced, vertex).
    #[must_use]
    pub fn is_zero_normal(&self, vertex: usize) -> bool {
        let n = self.unit_normals[vertex];
        n[0] == 0.0 && n[1] == 0.0 && n[2] == 0.0
    }
}

impl Obj {
    /// Resolve normals against positions. See the [module docs](self) for the
    /// exact rules; this cannot fail.
    ///
    /// # Panics
    ///
    /// Never on a parsed model. A hand-built [`Obj`] whose corner addresses a
    /// vertex that does not exist is a programming error and may panic here —
    /// the parser rejects that input with
    /// [`ObjError::IndexOutOfRange`](crate::ObjError::IndexOutOfRange).
    #[must_use]
    pub fn bind(&self) -> BoundMesh {
        let positions: Vec<[f64; 3]> = self.vertices.iter().map(|vertex| vertex.position).collect();
        let count = positions.len();
        let mut normals = vec![[0.0f64; 3]; count];
        let mut bound = vec![false; count];
        // Most faces are triangles, so this is usually the exact size.
        let mut triangles: Vec<[u32; 3]> = Vec::with_capacity(self.faces.len());

        // One walk over the fan: collect the indices, and bind each `vn` to the
        // position that references it. First reference wins; an index that does
        // not address the normal table leaves the corner unbound rather than
        // guessing at a neighbour. Streaming off the faces keeps this to one
        // triangle buffer.
        for face in &self.faces {
            for triangle in face.triangulate() {
                let corners = triangle.corners;
                triangles.push([corners[0].v, corners[1].v, corners[2].v]);
                for corner in corners {
                    let Some(index) = corner.vn else { continue };
                    let Some(&normal) = self.normals.get(index as usize) else {
                        continue;
                    };
                    debug_assert!((corner.v as usize) < count, "corner past the vertex table");
                    let slot = corner.v as usize;
                    if !bound[slot] {
                        normals[slot] = normal;
                        bound[slot] = true;
                    }
                }
            }
        }

        // Whatever is left unbound takes the geometric normal.
        if !bound.iter().all(|bound| *bound) {
            let geometric = geometric_normals(&positions, &triangles);
            for (slot, (value, bound)) in normals.iter_mut().zip(geometric.iter().zip(&bound)) {
                if !bound {
                    *slot = *value;
                }
            }
        }

        // Re-normalize: exported `vn` values are not always unit length. The
        // `max` floor keeps a genuine zero vector exactly zero instead of
        // turning it into NaN.
        for normal in &mut normals {
            let len =
                (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
            let divisor = len.max(1e-12);
            for component in normal {
                *component /= divisor;
            }
        }

        BoundMesh {
            positions,
            unit_normals: normals,
            triangles,
        }
    }
}

/// Area-weighted vertex normals: each triangle adds its unnormalized
/// `(b − a) × (c − a)` to all three corners, so the sum over the incident faces
/// is proportional to their mean area. Accumulated in triangle order, which is
/// what makes the float sum reproducible across runs and machines.
#[must_use]
pub fn geometric_normals(positions: &[[f64; 3]], triangles: &[[u32; 3]]) -> Vec<[f64; 3]> {
    let mut acc = vec![[0.0f64; 3]; positions.len()];
    for tri in triangles {
        let a = positions[tri[0] as usize];
        let b = positions[tri[1] as usize];
        let c = positions[tri[2] as usize];
        let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let cross = [
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
        ];
        for &vertex in tri {
            for (slot, component) in acc[vertex as usize].iter_mut().zip(cross) {
                *slot += component;
            }
        }
    }
    acc
}

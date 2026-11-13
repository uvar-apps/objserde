//! The raw, loss-preserving OBJ model.
//!
//! Every record the parser models is stored as it was read: `v` lines keep
//! their trailing floats, the `vn` table keeps the values written (they are
//! **not** normalized here — see [`Obj::bind`](crate::Obj::bind)), `vt` lines
//! keep their arity, faces keep all their corners instead of being
//! pre-triangulated, and each face remembers the material it was drawn under.
//! Nothing in this layer resolves a normal against a position; that is the
//! binding layer's job.

use crate::mtl::{Material, MaterialLib};

/// A parsed OBJ file: the tables the parser models.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Obj {
    /// One entry per `v` line, in file order.
    pub vertices: Vec<Vertex>,
    /// One `[x, y, z]` per `vn` line, in file order, **as written** (exported
    /// normals are not always unit length).
    pub normals: Vec<[f64; 3]>,
    /// One entry per `vt` line, in file order.
    pub tex_coords: Vec<TexCoord>,
    /// One entry per `f` line that carried at least one corner, in file order.
    pub faces: Vec<Face>,
    /// One entry per `mtllib` line, in the order the lines appeared. Status
    /// tells the caller whether the named library was actually read — a missing
    /// `.mtl` is not an error, but it is also not silently nothing.
    pub material_libs: Vec<MaterialLib>,
    /// The material table: every material defined by a loaded library, plus one
    /// entry per `usemtl` name that no loaded library defines (see
    /// [`Material::defined`]). Indexed by [`Face::material`].
    pub materials: Vec<Material>,
}

impl Obj {
    /// The material a face was drawn under, if any.
    #[must_use]
    pub fn material_of(&self, face: &Face) -> Option<&Material> {
        face.material.and_then(|i| self.materials.get(i as usize))
    }

    /// Index of a material by name, as `usemtl` would spell it.
    #[must_use]
    pub fn material_named(&self, name: &str) -> Option<u32> {
        // Linear on purpose: material tables hold dozens of entries, and the
        // crate keeps a hash map out of the read/write path entirely so no
        // output could ever depend on iteration order.
        self.materials
            .iter()
            .position(|m| m.name == name)
            .map(|i| i as u32)
    }

    /// Faces drawn under the material at `index`, in file order.
    pub fn faces_using(&self, index: u32) -> impl Iterator<Item = (usize, &Face)> + use<'_> {
        self.faces
            .iter()
            .enumerate()
            .filter(move |(_, face)| face.material == Some(index))
    }

    /// Fan-triangulate every face: corners `(0, k, k+1)` for
    /// `k in 1..corners.len() - 1`. Faces with fewer than three corners
    /// produce no triangles — the parser still keeps them.
    #[must_use]
    pub fn triangulate(&self) -> Vec<Triangle> {
        let mut out = Vec::new();
        for face in &self.faces {
            out.extend(face.triangulate());
        }
        out
    }
}

/// A `v` line: the position plus any trailing floats that followed it.
///
/// Vertex colours (`xyzrgb`-family exports) and other vendor extras live in
/// [`extra`](Self::extra), which keeps them through a round trip.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Vertex {
    /// The first three fields of the line.
    pub position: [f64; 3],
    /// Fields four onwards, kept in order. Empty for a plain `v x y z` line.
    pub extra: Vec<f64>,
}

impl Vertex {
    /// A vertex with no trailing fields.
    #[must_use]
    pub fn new(position: [f64; 3]) -> Self {
        Self {
            position,
            extra: Vec::new(),
        }
    }
}

/// A `vt` line. OBJ defines one to three components; the count is kept so a
/// round trip writes the same arity back.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TexCoord {
    /// The components read, valid up to [`arity`](Self::arity).
    pub components: [f64; 3],
    /// How many of `components` came from the file: `1..=3`.
    pub arity: u8,
}

impl TexCoord {
    /// The most components a `vt` line can carry.
    pub const MAX_COMPONENTS: usize = 3;

    /// Build from the components actually present. `values.len()` must be
    /// `1..=3`; anything else yields `None`.
    #[must_use]
    pub fn new(values: &[f64]) -> Option<Self> {
        if values.is_empty() || values.len() > Self::MAX_COMPONENTS {
            return None;
        }
        let mut components = [0.0f64; Self::MAX_COMPONENTS];
        components[..values.len()].copy_from_slice(values);
        Some(Self {
            components,
            // Casting a validated `1..=3` length.
            arity: values.len() as u8,
        })
    }

    /// The components that came from the file.
    #[must_use]
    pub fn as_slice(&self) -> &[f64] {
        let n = (self.arity as usize).min(Self::MAX_COMPONENTS);
        &self.components[..n]
    }
}

/// An `f` line: its corners, unreordered and un-triangulated, plus the material
/// in effect when the face was written.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Face {
    /// The corner records of the line, in order. At least one: a face line
    /// with no corners is skipped by the parser.
    pub corners: Vec<Corner>,
    /// Index into [`Obj::materials`] — the material set by the last `usemtl`
    /// line before this face, or `None` if nothing was in effect.
    pub material: Option<u32>,
}

impl Face {
    /// Fan this face into triangles: `(0, k, k+1)`. Fewer than three corners
    /// yields none.
    pub fn triangulate(&self) -> impl Iterator<Item = Triangle> + use<'_> {
        (1..self.corners.len().saturating_sub(1)).map(|k| Triangle {
            // A triangulating iterator only runs for `len >= 3`, so every
            // index below is in range.
            corners: [self.corners[0], self.corners[k], self.corners[k + 1]],
        })
    }
}

/// One corner of a face: `v`, `v/vt`, `v//vn` or `v/vt/vn`, all 0-based.
///
/// `vt` and `vn` are the indices as written minus one. An out-of-range `vn` is
/// not filtered here: [`Obj::bind`](crate::Obj::bind) treats it as unbound,
/// which is what meshview does.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Corner {
    /// Index into [`Obj::vertices`].
    pub v: u32,
    /// Index into [`Obj::tex_coords`], when the corner carried a `vt`.
    pub vt: Option<u32>,
    /// Index into [`Obj::normals`], when the corner carried a `vn`.
    pub vn: Option<u32>,
}

impl Corner {
    /// A position-only corner (`v` form).
    #[must_use]
    pub fn position(v: u32) -> Self {
        Self {
            v,
            vt: None,
            vn: None,
        }
    }
}

/// One corner triple of a fan triangulation.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Triangle {
    /// The three corners, in winding order as produced by the fan.
    pub corners: [Corner; 3],
}

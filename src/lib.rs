//! Read and write Wavefront OBJ files without quietly changing the mesh.
//!
//! `objserde` is one home for the OBJ code the same maintainer kept rewriting
//! per project: a **loss-preserving parser**, a **fixed-policy normal-binding
//! layer**, and **byte-deterministic writers**, with no required dependencies —
//! the core is `std` only, `serde` derives are one optional feature away.
//!
//! # Three layers, deliberately separate
//!
//! | Layer | Type | Keeps | Deliberately does not |
//! |---|---|---|---|
//! | 1 · raw model | [`Obj`] | every `v`/`vn`/`vt`/`f` record read, in file order, polygons whole | resolve a normal against a surface point |
//! | 2 · binding | [`BoundMesh`] | one unit normal per position, triangle indices | model materials, textures, groups |
//! | 3 · writers | [`Obj::to_string`], [`write_mesh`] | the same bytes every run | carry a timestamp or a locale |
//!
//! Layer 2 is a *policy*, not a data structure: it is the uvar normal-binding
//! contract, and if you need different semantics you parse with layer 1 and
//! write your own resolve step. Layer 1 round-trips; layer 2 is for shading.
//!
//! ```
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let obj = objserde::parse("v 0 0 0\nv 1 0 0\nv 0 1 0\nvn 0 0 1\nf 1//1 2//1 3//1\n")?;
//! assert_eq!(obj.vertices.len(), 3);
//!
//! let mesh = obj.bind();
//! assert_eq!(mesh.triangle_count(), 1);
//! assert_eq!(mesh.unit_normals[2], [0.0, 0.0, 1.0]);
//! # Ok(())
//! # }
//! ```
//!
//! # Which lines are read
//!
//! `v`, `vn`, `vt`, `f` — matched as exact line prefixes, one record per line.
//! Every other line is skipped: comments, `usemtl`, `g`, `o`, `s`, blanks,
//! unknown prefixes, and anything with leading whitespace before the keyword.
//! Skipping is not an error, but a skipped line is **not** round-tripped: v1
//! stores four record types and writes those back.
//!
//! # The binding rules, and why they exist
//!
//! A normal binds to the position that *references* it, by `vn` index; the first
//! corner to reference a position wins. Not by arrival order, and never by
//! reading `b` in `a/b/c` as a normal — that field is a texture coordinate. Two
//! bugs come from getting this wrong, both real:
//!
//! A flat-shaded export writes a `vn` per face corner, so its `vn` indices do
//! not line up with its `v` indices at all. Bind by arrival order and every
//! vertex gets some normal from the neighbourhood: the silhouette still looks
//! right, the shading is subtly wrong, and a side-by-side comparison of two such
//! meshes reports a difference that is not in the geometry. That is how one
//! comparison once lied.
//!
//! The other half: a corner whose `vn` index does not address the normal table
//! is left **unbound** rather than guessed at, and unbound positions fall back to
//! area-weighted geometric normals. Then everything is re-normalized by 1/length,
//! because exported normals are not always unit — with one exception: a zero
//! vector stays exactly zero, since a scan of folded geometry genuinely has
//! vertices with no orientation. The Stanford dragon has them. Neither the
//! parser nor the binder makes those vertices look readable, because they are
//! not.
//!
//! # Errors
//!
//! [`ObjError::NoVertices`], [`ObjError::BadFloat`], [`ObjError::NegativeIndex`],
//! [`ObjError::IndexOutOfRange`], [`ObjError::Io`]. Every parse error names the
//! line it happened on. Binding cannot fail. Relative (negative) face indices are
//! reported, never wrapped.
//!
//! # Limits, stated plainly
//!
//! * Skipped records (comments, materials, groups, `vp`/`curv`/… ) do not survive
//!   a round trip.
//! * Face indices use the `u32` space; a file with more than 2^32 vertices is not
//!   addressable.
//! * No material, group or smoothing-group model — this is geometry, not a scene.
//! * Mode A writes the shortest round-trip float form, which is not the form the
//!   input file used. Re-parsing is exact; diffing against the original text is
//!   not the point.

#![warn(missing_docs)]
#![forbid(unsafe_code)]

mod bind;
mod error;
mod model;
mod parse;
mod write;

pub use bind::BoundMesh;
pub use error::ObjError;
pub use model::{Corner, Face, Obj, TexCoord, Triangle, Vertex};
pub use parse::{parse, parse_bytes, parse_file};
pub use write::{DEFAULT_PRECISION, Precision, WriteOptions, write_mesh, write_mesh_into};

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
//! | 1 · raw model | [`Obj`] | every `v`/`vn`/`vt`/`f` record read, in file order, polygons whole, each face under its material, object and group, the header comments | resolve a normal against a surface point |
//! | 2 · binding | [`BoundMesh`] | one unit normal per position, triangle indices | read a material, or fail |
//! | 3 · writers | `obj.to_string()`, [`write_mesh`], [`write_material_lib`] | the same bytes every run | carry a timestamp or a locale |
//!
//! Layer 2 is a *policy*, not a data structure: it is the uvar normal-binding
//! contract, and if you need different semantics you parse with layer 1 and
//! write your own resolve step. Layer 1 round-trips; layer 2 is for shading.
//!
//! The modules are those layers, and the crate root re-exports the types you
//! reach for most, so `objserde::Material` and `objserde::mtl::Material` are the
//! same item.
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
//! # Materials and textures
//!
//! `mtllib` and `usemtl` are modelled, so shading arrives with the geometry:
//!
//! ```no_run
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let obj = objserde::parse_file("model.obj")?; // reads model.mtl if it is there
//! for face in &obj.faces {
//!     if let Some(material) = obj.material_of(face) {
//!         let _colour = material.diffuse;                 // Option<[f64; 3]>
//!         let _texture = material.diffuse_map();          // Option<&Map>
//!     }
//! }
//! // Write the library the OBJ's `mtllib` line points at:
//! let mtl = objserde::write_material_lib(
//!     &obj.materials,
//!     &objserde::MtlWriteOptions { header: vec!["# from model.obj".into()] },
//! );
//! # Ok(())
//! # }
//! ```
//!
//! Materials in OBJ are *state*: `usemtl` applies to every face after it, so
//! [`Face::material`] is the material in effect at that face and the writer emits
//! a `usemtl` line wherever it changes — not one per face.
//!
//! [`parse_file`] resolves `mtllib` against the OBJ's directory. A library that
//! is missing or unreadable does not fail the load; it is recorded on
//! [`Obj::material_libs`] as a [`LibraryStatus`], so "no materials" and "I could
//! not find them" stay different facts. Parsing from text has no directory, so
//! references come back [`LibraryStatus::Unresolved`] until you call
//! [`Obj::resolve_material_libs`].
//!
//! Parsing a `.mtl` **cannot fail**: a tag this model cannot hold — a spectral
//! `Kd 0.44 wood.spe`, a vendor `sharpini`, a misspelled value — is preserved
//! verbatim in [`Material::unknown`] and written back unchanged, instead of
//! dropping or failing the whole load. See the [`mtl`] module docs.
//!
//! Two round-trip caveats, both inherent to the format rather than to this
//! implementation: definitions live in a separate `.mtl` file, so writing the OBJ
//! text carries *references* only (call [`write_material_lib`] for the
//! definitions); and OBJ has no way to clear a material, so a hand-built face
//! with `material: None` following a referenced one inherits the previous material
//! when the text is re-read.
//!
//! # Objects, groups and the header
//!
//! `o` and `g` are state too: [`Face::object`] and [`Face::group`] index
//! [`Obj::objects`] and [`Obj::groups`], tables that hold each distinct value once
//! in order of first appearance, and the writer emits `o` and `g` where they
//! change. A mesh split into named groups — one per connected component, say —
//! keeps its groups through a round trip. The comment lines before the first
//! record are [`Obj::header`]: a file's provenance (the program that wrote it,
//! its parameters) survives a rewrite, and the writer puts it back first.
//!
//! ```
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let src = "# made by hand\nv 0 0 0\nv 1 0 0\nv 0 1 0\ng component_0\nf 1 2 3\n";
//! let obj = objserde::parse(src)?;
//! assert_eq!(obj.header, [" made by hand"]);
//! assert_eq!(obj.group_of(&obj.faces[0]), Some("component_0"));
//! assert_eq!(obj.to_string(), "# made by hand\nv 0 0 0\nv 1 0 0\nv 0 1 0\ng component_0\nf 1 2 3\n");
//! # Ok(())
//! # }
//! ```
//!
//! # Which lines are read
//!
//! `v`, `vn`, `vt`, `f`, `usemtl`, `mtllib`, `o`, `g` — matched as exact line
//! prefixes, one record per line — and the comments before the first record.
//! Every other line is skipped: later comments, `s`, `vp`, blanks, unknown
//! prefixes, and anything with leading whitespace before the keyword. Skipping is
//! not an error, but a skipped line is **not** round-tripped.
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
//! * Skipped records (comments after the first record, `s`, `vp`/`curv`/… ) do
//!   not survive a round trip. Materials, objects, groups and the header
//!   comments **do** — see above.
//! * Face indices use the `u32` space; a file with more than 2^32 vertices is not
//!   addressable.
//! * Smoothing groups (`s`) are still not modelled, and `bind` knows nothing
//!   about materials or groups: this is geometry plus shading, not a scene graph.
//! * OBJ cannot clear an object or group any more than a material: a hand-built
//!   face with `None` after one that had a value reads back with the earlier
//!   value in effect.
//! * Mode A writes the shortest round-trip float form, which is not the form the
//!   input file used. Re-parsing is exact; diffing against the original text is
//!   not the point.

#![warn(missing_docs)]
#![forbid(unsafe_code)]

pub mod bind;
pub mod error;
pub mod model;
pub mod mtl;
pub mod parse;
pub mod write;

pub use bind::BoundMesh;
pub use error::ObjError;
pub use model::{Corner, Face, Obj, TexCoord, Triangle, Vertex};
pub use mtl::{
    Channel, LibraryStatus, Map, MapKind, MapOption, Material, MaterialLib, UnknownTag, parse_mtl,
    parse_mtl_file,
};
pub use parse::{parse, parse_bytes, parse_file};
pub use write::{
    DEFAULT_PRECISION, MtlWriteOptions, Precision, WriteOptions, write_material_lib,
    write_material_lib_into, write_mesh, write_mesh_into,
};

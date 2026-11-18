//! Layer 1: the loss-preserving parser.
//!
//! A plain std line scanner. Records are recognised by their exact line prefix
//! — `v `, `vn `, `vt `, `f `, `usemtl `, `mtllib `, `o`, `g`, `s` — and the
//! comment lines before the first record are kept as the header. Everything
//! else (later comments, `vp`, blanks, unknown prefixes) is skipped: a reader
//! that dies on a record it does not model cannot read most OBJ files in the
//! wild. Skipped lines are not stored, so they are **not** round-tripped; the
//! crate docs carry the full contract.
//!
//! Ordinary face indices are not resolved while reading. They are validated at
//! end-of-parse against the finished tables, which is what lets every offending
//! face report its own line instead of the first failure shadowing the rest.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::bind::BoundMesh;
use crate::error::ObjError;
use crate::model::{Corner, Face, Obj, TexCoord, Vertex};
use crate::mtl::{LibraryStatus, Material, MaterialLib, parse_mtl_file};

/// Split an `mtllib` argument list into paths, honouring double quotes so a
/// filename with spaces survives: `mtllib "my mats.mtl" other.mtl`.
///
/// An unbalanced quote takes the rest of the line as one path rather than
/// failing. Escaped quotes inside a name are not supported by the format.
fn split_paths(rest: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut started = false;

    for ch in rest.chars() {
        match ch {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            c if c.is_whitespace() && !quoted => {
                if started {
                    paths.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            c => {
                current.push(c);
                started = true;
            }
        }
    }
    if started {
        paths.push(current);
    }
    paths
}

/// Parse an OBJ from a file, then read the material libraries it names.
///
/// `mtllib` paths resolve against the OBJ's own directory. A library that is
/// missing or unreadable does not fail the load — it is recorded on
/// [`Obj::material_libs`] with its status, so absence stays distinguishable from
/// "loaded and empty".
///
/// Errors other than [`ObjError::Io`] name a line but not a path: a caller that
/// wants the file name in its message wraps the error itself.
pub fn parse_file(path: impl AsRef<Path>) -> Result<Obj, ObjError> {
    let path = path.as_ref();
    let bytes = std::fs::read(path).map_err(|source| ObjError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut obj = parse_bytes(&bytes)?;
    // An OBJ with no directory part resolves its libraries against the cwd.
    obj.resolve_material_libs(path.parent().unwrap_or(Path::new(".")));
    Ok(obj)
}

/// Parse an OBJ from bytes, decoding them UTF-8-lossy. A stray byte inside a
/// line the parser ignores cannot fail the read.
///
/// `mtllib` references are recorded as
/// [`LibraryStatus::Unresolved`]: bytes carry no
/// directory to resolve them against. Use [`parse_file`], or
/// [`Obj::resolve_material_libs`], to load the materials.
pub fn parse_bytes(bytes: &[u8]) -> Result<Obj, ObjError> {
    let text = String::from_utf8_lossy(bytes);
    parse(&text)
}

/// Parse an OBJ from text. Material libraries stay unresolved (no base
/// directory); material *references* are captured either way.
pub fn parse(text: &str) -> Result<Obj, ObjError> {
    let mut obj = Obj::default();
    // Source line per face, kept only for the end-of-parse index check.
    let mut face_lines: Vec<usize> = Vec::new();
    // The material, object, group and smoothing value in effect for faces that
    // follow, as `usemtl`, `o`, `g` and `s` set them.
    let mut current_material: Option<u32> = None;
    let mut current_object: Option<u32> = None;
    let mut current_group: Option<u32> = None;
    let mut current_smooth: Option<u32> = None;
    // Name → table index for `o`, `g` and `s`. A file may carry thousands of groups
    // (one per mesh component), so lookup is ordered rather than linear; a
    // BTreeMap keeps hashing out of the read path, and it is never iterated.
    let mut object_index: BTreeMap<String, u32> = BTreeMap::new();
    let mut group_index: BTreeMap<String, u32> = BTreeMap::new();
    let mut smooth_index: BTreeMap<String, u32> = BTreeMap::new();
    // Comments are the header until the first line that is neither a comment
    // nor blank.
    let mut in_header = true;

    for (idx, line) in text.lines().enumerate() {
        let line_no = idx + 1;
        if in_header {
            if let Some(comment) = line.strip_prefix('#') {
                obj.header.push(comment.to_string());
                continue;
            }
            in_header = line.trim().is_empty();
        }
        if let Some(rest) = line.strip_prefix("v ") {
            obj.vertices.push(read_vertex(line_no, rest)?);
        } else if let Some(rest) = line.strip_prefix("f ") {
            if let Some(mut face) = read_face(line_no, rest, obj.vertices.len())? {
                face.material = current_material;
                face.object = current_object;
                face.group = current_group;
                face.smooth_group = current_smooth;
                obj.faces.push(face);
                face_lines.push(line_no);
            }
        } else if let Some(rest) = line.strip_prefix("vn ") {
            obj.normals.push(read_triple(line_no, rest)?);
        } else if let Some(rest) = line.strip_prefix("vt ") {
            obj.tex_coords.push(read_tex_coord(line_no, rest)?);
        } else if let Some(rest) = line.strip_prefix("usemtl ") {
            current_material = Some(obj.reference_material(rest.trim()));
        } else if let Some(rest) = line.strip_prefix("mtllib ") {
            // Several libraries may share one line; each becomes its own record.
            for path in split_paths(rest) {
                obj.material_libs.push(MaterialLib {
                    path,
                    status: LibraryStatus::Unresolved,
                });
            }
        } else if let Some(names) = record_value(line, "g") {
            current_group = Some(intern(&mut obj.groups, &mut group_index, names));
        } else if let Some(name) = record_value(line, "o") {
            current_object = Some(intern(&mut obj.objects, &mut object_index, name));
        } else if let Some(value) = record_value(line, "s") {
            // Stored verbatim: `"off"`, `"0"`, `"12"`. What disables smoothing
            // is a shading question; keeping the statement is a serialization
            // one, and only the second one is this crate's job.
            current_smooth = Some(intern(&mut obj.smooth_groups, &mut smooth_index, value));
        }
        // Later comments, `vp`, curve bodies, unknown records: skipped, not fatal.
    }

    if obj.vertices.is_empty() {
        return Err(ObjError::NoVertices);
    }
    check_face_indices(&obj, &face_lines)?;
    Ok(obj)
}

/// The value of an `o`/`g` line: the rest of the line after the keyword and one
/// space, trimmed, or `""` for the bare keyword. Like every other record, the
/// keyword must start the line and be followed by a space, so `gx` and ` g a`
/// are not group lines.
fn record_value<'a>(line: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(keyword)?;
    if rest.is_empty() {
        Some("")
    } else {
        rest.strip_prefix(' ').map(str::trim)
    }
}

/// The table index of `name`, appending it on first sight.
fn intern(table: &mut Vec<String>, index: &mut BTreeMap<String, u32>, name: &str) -> u32 {
    if let Some(&i) = index.get(name) {
        return i;
    }
    // Same `u32` addressing rule as every other table.
    let i = table.len() as u32;
    table.push(name.to_string());
    index.insert(name.to_string(), i);
    i
}

/// A `usemtl` name as a table index: an existing entry if the name is known,
/// otherwise a new `defined: false` entry standing in for a reference whose
/// definition was never loaded.
impl Obj {
    fn reference_material(&mut self, name: &str) -> u32 {
        // Linear search: `usemtl` lines are rare (a handful per file, not one
        // per face), so this avoids putting a hash map in the read path at all.
        if let Some(index) = self.material_named(name) {
            return index;
        }
        self.materials.push(Material::referenced(name));
        // A file with more than 2^32 distinct material names is not addressable,
        // same rule as every other index here.
        (self.materials.len() - 1) as u32
    }
}

/// Read a `v` line: three coordinates plus any trailing floats, kept so
/// `xyzrgb`-family vertex colours survive a round trip.
fn read_vertex(line: usize, rest: &str) -> Result<Vertex, ObjError> {
    let mut fields = rest.split_whitespace();
    let mut position = [0.0f64; 3];
    for slot in position.iter_mut() {
        *slot = next_float(line, &mut fields)?;
    }
    let mut extra = Vec::new();
    for field in fields {
        extra.push(parse_float(line, field)?);
    }
    Ok(Vertex { position, extra })
}

/// Read a `vn` line: the first three fields, stored exactly as written — never
/// normalized here, that is [`Obj::bind`]'s job. Fields past the third
/// (coloured `vn` exports exist) are ignored.
fn read_triple(line: usize, rest: &str) -> Result<[f64; 3], ObjError> {
    let mut fields = rest.split_whitespace();
    let mut out = [0.0f64; 3];
    for slot in out.iter_mut() {
        *slot = next_float(line, &mut fields)?;
    }
    Ok(out)
}

/// Read a `vt` line, keeping how many components it carried (1..=3). Fields
/// past the third are outside what OBJ defines and are ignored.
fn read_tex_coord(line: usize, rest: &str) -> Result<TexCoord, ObjError> {
    let mut components = [0.0f64; TexCoord::MAX_COMPONENTS];
    let mut arity = 0usize;
    for field in rest.split_whitespace().take(TexCoord::MAX_COMPONENTS) {
        components[arity] = parse_float(line, field)?;
        arity += 1;
    }
    TexCoord::new(&components[..arity]).ok_or_else(|| ObjError::BadFloat {
        line,
        got: "<missing field>".to_string(),
    })
}

/// Read one `f` line into a polygon. Faces with one or two corners are kept so
/// the model can write the file back unchanged; a line whose `f ` prefix carries
/// no corner tokens at all produces no face, since there is nothing to write
/// back and a bare `f` would not read as one.
fn read_face(line: usize, rest: &str, vertex_count: usize) -> Result<Option<Face>, ObjError> {
    let mut corners: Vec<Corner> = Vec::new();
    for token in rest.split_whitespace() {
        corners.push(read_corner(line, token, vertex_count)?);
    }
    if corners.is_empty() {
        return Ok(None);
    }
    Ok(Some(Face {
        corners,
        material: None,
        object: None,
        group: None,
        smooth_group: None,
    }))
}

/// Read one corner: `v`, `v/vt`, `v//vn` or `v/vt/vn`.
///
/// Fields are taken one `/`-split step at a time instead of collecting a `Vec`
/// per corner — this runs once per corner of files holding millions of faces.
/// Two semantics are preserved from the reference reader: `b` in `a/b/c` is a
/// texture index and is **never** read as a normal, and a normal exists only
/// when the third field is present and non-empty. Fields past the third are
/// ignored, as the reference reader ignores them.
fn read_corner(line: usize, token: &str, vertex_count: usize) -> Result<Corner, ObjError> {
    // A malformed field blames the whole token: that is what the reader sees in
    // the file, and which field broke is visible inside it.
    let mut fields = token.split('/');
    // `split` always yields a first slice.
    let head = fields.next().unwrap_or("");
    let bad = || ObjError::BadFloat {
        line,
        got: token.to_string(),
    };

    let v = head.parse::<i64>().map_err(|_| bad())?;
    if v <= 0 {
        // Relative (`-3`) indices and position `0`: refuse, never wrap.
        return Err(ObjError::NegativeIndex { line, value: v });
    }
    // Past the whole `u32` index space nothing can address the corner, and the
    // model has no slot that could hold it, so this is decided now rather than
    // deferred; the count shown is the vertex table as read so far.
    let v = u32::try_from(v - 1).map_err(|_| ObjError::IndexOutOfRange {
        line,
        value: v,
        count: vertex_count,
    })?;
    let vt = read_reference(fields.next(), token, line)?;
    let vn = read_reference(fields.next(), token, line)?;
    Ok(Corner { v, vt, vn })
}

/// A `vt`/`vn` field: absent when there is no field, when it is empty, or when
/// it is written `0` (OBJ's "unset" marker, which the reference reader also
/// leaves bound to nothing); otherwise kept exactly as written. An index past its
/// table is *not* a parse error here: [`Obj::bind`] treats an out-of-range `vn`
/// as unbound, and an out-of-range `vt` belongs to whoever consumes texture
/// coordinates.
///
/// A malformed number blames the whole corner token, because that is what the
/// reader sees in the file.
fn read_reference(field: Option<&str>, token: &str, line: usize) -> Result<Option<u32>, ObjError> {
    let Some(field) = field.filter(|field| !field.is_empty()) else {
        return Ok(None);
    };
    let value = field.parse::<i64>().map_err(|_| ObjError::BadFloat {
        line,
        got: token.to_string(),
    })?;
    if value < 0 {
        // Relative indices are relative in all three fields. Refusing beats
        // binding the corner to the wrong entry, and beats dropping the
        // reference without a word.
        return Err(ObjError::NegativeIndex { line, value });
    }
    if value == 0 {
        return Ok(None);
    }
    Ok(Some(
        u32::try_from(value - 1)
            // Past the `u32` space the value stays unaddressable rather than
            // aliasing a real entry: saturating keeps it out of every table.
            .unwrap_or(u32::MAX),
    ))
}

fn parse_float(line: usize, field: &str) -> Result<f64, ObjError> {
    field.parse::<f64>().map_err(|_| ObjError::BadFloat {
        line,
        got: field.to_string(),
    })
}

fn next_float<'a>(
    line: usize,
    fields: &mut impl Iterator<Item = &'a str>,
) -> Result<f64, ObjError> {
    match fields.next() {
        Some(field) => parse_float(line, field),
        None => Err(ObjError::BadFloat {
            line,
            got: "<missing field>".to_string(),
        }),
    }
}

/// Check the face indices against the finished vertex table.
///
/// Only `v` is validated: an out-of-range `vn` is meaningful to the binding
/// layer, which leaves such a corner unbound, and an out-of-range `vt` is left
/// for whoever consumes texture coordinates. Walking the faces here rather than
/// reporting during the read is what gives every offending face its own line.
fn check_face_indices(obj: &Obj, face_lines: &[usize]) -> Result<(), ObjError> {
    let count = obj.vertices.len();
    for (face, &line) in obj.faces.iter().zip(face_lines) {
        for corner in &face.corners {
            if corner.v as usize >= count {
                return Err(ObjError::IndexOutOfRange {
                    line,
                    value: i64::from(corner.v) + 1,
                    count,
                });
            }
        }
    }
    Ok(())
}

impl Obj {
    /// Read every `mtllib` reference against `base` and merge the materials it
    /// finds into [`Obj::materials`].
    ///
    /// Called for you by [`parse_file`]; call it yourself after [`parse`] or
    /// [`parse_bytes`], which have no directory to work from. Libraries already
    /// loaded are left alone, so calling this twice is harmless. Nothing here is
    /// fatal: what could not be read is recorded as a status on
    /// [`Obj::material_libs`].
    pub fn resolve_material_libs(&mut self, base: &Path) {
        for index in 0..self.material_libs.len() {
            if self.material_libs[index].is_loaded() {
                continue;
            }
            let written = self.material_libs[index].path.clone();
            let written = Path::new(&written);
            // An absolute path in the file means what it says; anything else is
            // relative to the OBJ, which is how authoring tools write it.
            let full = if written.is_absolute() {
                PathBuf::from(written)
            } else {
                base.join(written)
            };
            let status = match parse_mtl_file(&full) {
                Ok(materials) => {
                    self.merge_library(index as u32, materials);
                    LibraryStatus::Loaded
                }
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => LibraryStatus::Missing,
                Err(_) => LibraryStatus::Unreadable,
            };
            self.material_libs[index].status = status;
        }
    }

    /// Fold one library's materials into the table.
    ///
    /// A name that only existed as a `usemtl` reference becomes the real
    /// definition — that is the common case when a file writes `usemtl` before
    /// `mtllib`. A name already defined by an earlier library keeps that
    /// definition: first definition wins, the same instinct as first binding
    /// wins.
    fn merge_library(&mut self, library: u32, materials: Vec<Material>) {
        for mut material in materials {
            match self.material_named(&material.name) {
                Some(index) if !self.materials[index as usize].defined => {
                    material.library = Some(library);
                    self.materials[index as usize] = material;
                }
                Some(_) => {}
                None => {
                    material.library = Some(library);
                    self.materials.push(material);
                }
            }
        }
    }

    /// Load and bind a file in one call: [`parse_file`] then
    /// [`bind`](Self::bind).
    pub fn load(path: impl AsRef<Path>) -> Result<BoundMesh, ObjError> {
        parse_file(path).map(|obj| obj.bind())
    }
}

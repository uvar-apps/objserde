//! Layer 1: the loss-preserving parser.
//!
//! A plain std line scanner. Records are recognised by their exact line prefix
//! — `v `, `vn `, `vt `, `f ` — and everything else (comments, `usemtl`, `g`,
//! `o`, `s`, blanks, unknown prefixes) is skipped: a reader that dies on a
//! record it does not model cannot read most OBJ files in the wild. Skipped
//! lines are not stored, so they are **not** round-tripped; the crate docs
//! carry the full contract.
//!
//! Ordinary face indices are not resolved while reading. They are validated at
//! end-of-parse against the finished tables, which is what lets every offending
//! face report its own line instead of the first failure shadowing the rest.

use std::path::Path;

use crate::bind::BoundMesh;
use crate::error::ObjError;
use crate::model::{Corner, Face, Obj, TexCoord, Vertex};

/// Parse an OBJ from a file.
///
/// Errors other than [`ObjError::Io`] name a line but not a path: a caller that
/// wants the file name in its message wraps the error itself.
pub fn parse_file(path: impl AsRef<Path>) -> Result<Obj, ObjError> {
    let path = path.as_ref();
    let bytes = std::fs::read(path).map_err(|source| ObjError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    parse_bytes(&bytes)
}

/// Parse an OBJ from bytes, decoding them UTF-8-lossy. A stray byte inside a
/// line the parser ignores cannot fail the read.
pub fn parse_bytes(bytes: &[u8]) -> Result<Obj, ObjError> {
    let text = String::from_utf8_lossy(bytes);
    parse(&text)
}

/// Parse an OBJ from text.
pub fn parse(text: &str) -> Result<Obj, ObjError> {
    let mut obj = Obj::default();
    // Source line per face, kept only for the end-of-parse index check.
    let mut face_lines: Vec<usize> = Vec::new();

    for (idx, line) in text.lines().enumerate() {
        let line_no = idx + 1;
        if let Some(rest) = line.strip_prefix("v ") {
            obj.vertices.push(read_vertex(line_no, rest)?);
        } else if let Some(rest) = line.strip_prefix("vn ") {
            obj.normals.push(read_triple(line_no, rest)?);
        } else if let Some(rest) = line.strip_prefix("vt ") {
            obj.tex_coords.push(read_tex_coord(line_no, rest)?);
        } else if let Some(rest) = line.strip_prefix("f ") {
            if let Some(face) = read_face(line_no, rest, obj.vertices.len())? {
                obj.faces.push(face);
                face_lines.push(line_no);
            }
        }
        // Comments, materials, groups, unknown records: skipped, not fatal.
    }

    if obj.vertices.is_empty() {
        return Err(ObjError::NoVertices);
    }
    check_face_indices(&obj, &face_lines)?;
    Ok(obj)
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
    Ok(Some(Face { corners }))
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
    /// Load and bind a file in one call: [`parse_file`] then
    /// [`bind`](Self::bind).
    pub fn load(path: impl AsRef<Path>) -> Result<BoundMesh, ObjError> {
        parse_file(path).map(|obj| obj.bind())
    }
}

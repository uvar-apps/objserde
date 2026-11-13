//! Writers. Both modes are byte-deterministic: the same model in produces the
//! same bytes out, on every run and every machine. Comparisons and caches
//! downstream depend on that, so nothing here iterates a hash map, reads a
//! clock, or formats a number through a locale.
//!
//! **Mode A — structural round trip** ([`Obj`]'s [`Display`](std::fmt::Display),
//! reached with `obj.to_string()`): writes every record in canonical layout
//! (all `v`, then `vt`, then `vn`, then `f`) preserving each record's shape —
//! trailing floats on `v`, `vt` arity, and the corner form (`i`, `i/vt`,
//! `i//vn`, `i/vt/vn`) each corner used. Numbers use the shortest form that
//! parses back to the same `f64`, so `parse(&obj.to_string())` yields
//! bit-identical values. Byte equality with *another* producer is not promised,
//! and neither is byte equality with the file the model came from: skipped
//! records are gone, the record order is normalized, and `1.0` is written `1`.
//!
//! **Mode B — canonical mesh form** ([`write_mesh`]): the fixed-precision output
//! `molses` writes — one `v`/`vn` triple per line and `f a//a b//b c//c` faces.
//! Corner form is normalized to `i//i` here, because this mode generates fresh
//! meshes; use Mode A to preserve a file's shape.

use std::fmt;

use crate::bind::BoundMesh;
use crate::model::{Corner, Obj};

/// Mode A: every record in canonical layout, shape-preserving.
///
/// A face with no corners has nothing to address and writes nothing: the parser
/// never produces one, and a bare `f` line would not read back as a face.
impl fmt::Display for Obj {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for vertex in &self.vertices {
            let [x, y, z] = vertex.position;
            write!(f, "v {x} {y} {z}")?;
            // Trailing fields (vertex colours and other vendor extras) follow on
            // the same line they were read from.
            for extra in &vertex.extra {
                write!(f, " {extra}")?;
            }
            writeln!(f)?;
        }
        for tex in &self.tex_coords {
            write!(f, "vt")?;
            for component in tex.as_slice() {
                write!(f, " {component}")?;
            }
            writeln!(f)?;
        }
        for normal in &self.normals {
            let [x, y, z] = normal;
            writeln!(f, "vn {x} {y} {z}")?;
        }
        for face in &self.faces {
            let Some((first, rest)) = face.corners.split_first() else {
                continue;
            };
            write!(f, "f")?;
            for corner in std::iter::once(first).chain(rest) {
                write!(f, " ")?;
                write_corner(f, corner)?;
            }
            writeln!(f)?;
        }
        Ok(())
    }
}

/// Write one corner in the exact field form its components justify: `i`,
/// `i/vt`, `i//vn` or `i/vt/vn`.
fn write_corner(f: &mut fmt::Formatter<'_>, corner: &Corner) -> fmt::Result {
    // Back to the 1-based indices the file used. The widening keeps a
    // hand-built `u32::MAX` index from overflowing.
    let v = u64::from(corner.v) + 1;
    match (corner.vt, corner.vn) {
        (Some(vt), Some(vn)) => write!(f, "{v}/{}", u64::from(vt) + 1)?,
        (Some(vt), None) => write!(f, "{v}/{}", u64::from(vt) + 1)?,
        // Two slashes: the texture slot stays empty, which is how a file carries
        // a normal without a uv.
        (None, Some(vn)) => write!(f, "{v}//{}", u64::from(vn) + 1)?,
        (None, None) => write!(f, "{v}")?,
    }
    Ok(())
}

/// How [`write_mesh`] formats coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Precision {
    /// `{:.n}` fixed decimals.
    Fixed(usize),
    /// The shortest form that parses back to the same `f64` — what Mode A uses.
    Shortest,
}

/// The default fixed precision: the `molses` line format.
pub const DEFAULT_PRECISION: usize = 6;

impl Default for Precision {
    fn default() -> Self {
        Self::Fixed(DEFAULT_PRECISION)
    }
}

/// Options for [`write_mesh`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WriteOptions {
    /// Coordinate format.
    pub precision: Precision,
    /// Lines written before the geometry, verbatim — `#` included if the caller
    /// wants a comment. Header wording is the caller's business; this crate adds
    /// no timestamp and no path, so repeated writes stay byte-identical.
    pub header: Vec<String>,
}

/// Mode B: write a bound mesh in the canonical form `molses` emits —
/// `v {:.6}`, `vn {:.6}`, `f a//a b//b c//c`, 1-based indices.
#[must_use]
pub fn write_mesh(mesh: &BoundMesh, options: &WriteOptions) -> String {
    let mut out = String::new();
    // A `String` sink has no failure mode worth returning.
    write_mesh_into(&mut out, mesh, options).ok();
    out
}

/// Mode B into any [`std::fmt::Write`] sink, for callers streaming to a file.
///
/// # Errors
///
/// Whatever the sink reports; writing to a [`String`] never fails.
pub fn write_mesh_into<W: fmt::Write>(
    out: &mut W,
    mesh: &BoundMesh,
    options: &WriteOptions,
) -> fmt::Result {
    let precision = options.precision;
    for line in &options.header {
        writeln!(out, "{line}")?;
    }
    for p in &mesh.positions {
        writeln!(
            out,
            "v {} {} {}",
            num(p[0], precision),
            num(p[1], precision),
            num(p[2], precision)
        )?;
    }
    for n in &mesh.unit_normals {
        writeln!(
            out,
            "vn {} {} {}",
            num(n[0], precision),
            num(n[1], precision),
            num(n[2], precision)
        )?;
    }
    for t in &mesh.triangles {
        let (a, b, c) = (
            u64::from(t[0]) + 1,
            u64::from(t[1]) + 1,
            u64::from(t[2]) + 1,
        );
        writeln!(out, "f {a}//{a} {b}//{b} {c}//{c}")?;
    }
    Ok(())
}

/// One coordinate, formatted for Mode B.
fn num(value: f64, precision: Precision) -> Num {
    Num(value, precision)
}

struct Num(f64, Precision);

impl fmt::Display for Num {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.1 {
            Precision::Fixed(digits) => write!(f, "{:.*}", digits, self.0),
            Precision::Shortest => write!(f, "{}", self.0),
        }
    }
}

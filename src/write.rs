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
//! bit-identical values. The header comments come first, and `o`, `g` and
//! `usemtl` lines appear among the faces wherever they change. Byte equality with
//! *another* producer is not promised, and neither is byte equality with the file
//! the model came from: skipped records are gone, the record order is normalized,
//! and `1.0` is written `1`.
//!
//! **Mode B — canonical mesh form** ([`write_mesh`]): the fixed-precision output
//! `molses` writes — one `v`/`vn` triple per line and `f a//a b//b c//c` faces.
//! Corner form is normalized to `i//i` here, because this mode generates fresh
//! meshes; use Mode A to preserve a file's shape.

use std::fmt;

use crate::bind::BoundMesh;
use crate::model::{Corner, Obj};
use crate::mtl::{MapOption, Material};

/// Mode A: every record in canonical layout, shape-preserving.
///
/// The header comments come first, then `mtllib` lines, then `v`, `vt`, `vn`,
/// then faces — with an `o`, `g` or `usemtl` line wherever the object, group or
/// material changes (in that order when several change at once), since in OBJ
/// all three are state, not per-face fields.
///
/// A face with no corners has nothing to address and writes nothing: the parser
/// never produces one, and a bare `f` line would not read back as a face.
impl fmt::Display for Obj {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for comment in &self.header {
            writeln!(f, "#{comment}")?;
        }
        for lib in &self.material_libs {
            // Quote a path containing spaces so it reads back as one name.
            if lib.path.contains(char::is_whitespace) {
                writeln!(f, "mtllib \"{path}\"", path = lib.path)?;
            } else {
                writeln!(f, "mtllib {}", lib.path)?;
            }
        }
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
        let mut current: Option<u32> = None;
        let mut current_object: Option<u32> = None;
        let mut current_group: Option<u32> = None;
        for face in &self.faces {
            // State changes are the only reason a non-geometry line appears
            // among the faces; emit each exactly when it differs. A change to
            // `None` writes nothing: OBJ cannot clear an object, group or
            // material, so the earlier one stays in effect when read back.
            if face.object != current_object {
                if let Some(name) = face.object.and_then(|i| self.objects.get(i as usize)) {
                    write_state(f, "o", name)?;
                }
                current_object = face.object;
            }
            if face.group != current_group {
                if let Some(names) = face.group.and_then(|i| self.groups.get(i as usize)) {
                    write_state(f, "g", names)?;
                }
                current_group = face.group;
            }
            if face.material != current {
                if let Some(index) = face.material {
                    if let Some(material) = self.materials.get(index as usize) {
                        writeln!(f, "usemtl {}", material.name)?;
                    }
                }
                current = face.material;
            }
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

/// Write an `o` or `g` line; an empty value is the bare keyword, which is how
/// the parser reads it back.
fn write_state(f: &mut fmt::Formatter<'_>, keyword: &str, value: &str) -> fmt::Result {
    if value.is_empty() {
        writeln!(f, "{keyword}")
    } else {
        writeln!(f, "{keyword} {value}")
    }
}

/// Write one corner in the exact field form its components justify: `i`,
/// `i/vt`, `i//vn` or `i/vt/vn`.
fn write_corner(f: &mut fmt::Formatter<'_>, corner: &Corner) -> fmt::Result {
    // Back to the 1-based indices the file used. The widening keeps a
    // hand-built `u32::MAX` index from overflowing.
    let v = u64::from(corner.v) + 1;
    match (corner.vt, corner.vn) {
        (Some(vt), Some(vn)) => write!(f, "{v}/{}/{}", u64::from(vt) + 1, u64::from(vn) + 1)?,
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

/// Options for [`write_material_lib`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MtlWriteOptions {
    /// Lines written before the materials, verbatim — `#` included if the caller
    /// wants a comment. No timestamp, so repeated writes stay byte-identical.
    pub header: Vec<String>,
}

/// Write a material library in the `.mtl` format: canonical tag order per
/// material, then that material's texture maps in the order they were read, then
/// any tags the model does not hold, verbatim.
///
/// Numbers use the shortest form that parses back to the same value, so
/// [`parse_mtl`](crate::parse_mtl) of this output is bit-identical — the same
/// guarantee Mode A makes for geometry. Tag *case* is canonicalized (a file that
/// said `MAP_KD` comes back as `map_Kd`), and `bump` becomes `map_bump`.
///
/// Entries that only stand in for an unloaded `usemtl` reference
/// ([`Material::defined`] is false) are not written: a definition file should not
/// claim materials it has no properties for. The reference itself is safe — it
/// lives in the OBJ's `usemtl` line, and re-reading synthesizes the entry again.
pub fn write_material_lib(materials: &[Material], options: &MtlWriteOptions) -> String {
    let mut out = String::new();
    write_material_lib_into(&mut out, materials, options).ok();
    out
}

/// [`write_material_lib`] into any [`std::fmt::Write`] sink.
///
/// # Errors
///
/// Whatever the sink reports; writing to a [`String`] never fails.
pub fn write_material_lib_into<W: fmt::Write>(
    out: &mut W,
    materials: &[Material],
    options: &MtlWriteOptions,
) -> fmt::Result {
    for line in &options.header {
        writeln!(out, "{line}")?;
    }
    for material in materials {
        if !material.defined {
            continue;
        }
        writeln!(out, "newmtl {}", material.name)?;
        if let Some(rgb) = material.ambient {
            writeln!(out, "Ka {} {} {}", rgb[0], rgb[1], rgb[2])?;
        }
        if let Some(rgb) = material.diffuse {
            writeln!(out, "Kd {} {} {}", rgb[0], rgb[1], rgb[2])?;
        }
        if let Some(rgb) = material.specular {
            writeln!(out, "Ks {} {} {}", rgb[0], rgb[1], rgb[2])?;
        }
        if let Some(rgb) = material.emissive {
            writeln!(out, "Ke {} {} {}", rgb[0], rgb[1], rgb[2])?;
        }
        if let Some(v) = material.shininess {
            writeln!(out, "Ns {v}")?;
        }
        if let Some(v) = material.dissolve {
            writeln!(out, "d {v}")?;
        }
        if let Some(v) = material.transmission {
            writeln!(out, "Tr {v}")?;
        }
        if let Some(v) = material.illumination {
            writeln!(out, "illum {v}")?;
        }
        for map in &material.maps {
            write!(out, "{}", map.kind.tag())?;
            for option in &map.options {
                write!(out, " {}", map_option(option))?;
            }
            writeln!(out, " {}", map.path)?;
        }
        for tag in &material.unknown {
            if tag.args.is_empty() {
                writeln!(out, "{}", tag.tag)?;
            } else {
                writeln!(out, "{} {}", tag.tag, tag.args)?;
            }
        }
    }
    Ok(())
}

/// One `-flag …` group, spelled back the way it was read.
fn map_option(option: &MapOption) -> impl fmt::Display + '_ {
    struct Opt<'a>(&'a MapOption);
    impl fmt::Display for Opt<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self.0 {
                MapOption::Scale(v) => write!(f, "-s {}", Joined(v))?,
                MapOption::Offset(v) => write!(f, "-o {}", Joined(v))?,
                MapOption::BumpScale(v) => write!(f, "-bm {v}")?,
                MapOption::Channel(c) => write!(f, "-imfchan {}", c.letter())?,
                MapOption::BlendU(v) => write!(f, "-blendu {}", on_off(*v))?,
                MapOption::BlendV(v) => write!(f, "-blendv {}", on_off(*v))?,
                MapOption::Clamp(v) => write!(f, "-clamp {}", on_off(*v))?,
                MapOption::TextureResolution(v) => write!(f, "-texres {v}")?,
                MapOption::Unknown { flag, args } => {
                    write!(f, "{flag}")?;
                    for arg in args {
                        write!(f, " {arg}")?;
                    }
                }
            }
            Ok(())
        }
    }
    Opt(option)
}

struct Joined<'a>(&'a [f64]);

impl fmt::Display for Joined<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, v) in self.0.iter().enumerate() {
            if i > 0 {
                write!(f, " ")?;
            }
            write!(f, "{v}")?;
        }
        Ok(())
    }
}

const fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

//! Material Template Library: the shading and texture side of an OBJ.
//!
//! An OBJ names materials with `usemtl` and finds their definitions in files
//! listed by `mtllib`. This module holds the in-memory shape of those
//! definitions ([`Material`], [`Map`], [`MapOption`]) and the parser for the
//! `.mtl` text format.
//!
//! # Parsing here is total
//!
//! [`parse_mtl`] returns materials, never an error. That is a deliberate
//! difference from the OBJ parser and it follows the crate's own rule — die on a
//! record you model and cannot read, skip what you do not model — with one extra
//! step for decorative data: **a tag the model cannot hold is preserved verbatim
//! in [`Material::unknown`] rather than dropped.**
//!
//! The reason is blast radius. A malformed `Kd` line is a broken appearance, not
//! broken geometry, and letting it fail `Obj::load` would turn a missing paint
//! job into a file that will not open. So `Ka 0.44 spec.mts` (a spectral-colour
//! reference some SGI-era writers emit), a `Ns` that is not a number, a tag with
//! four components or a name spelled `MAP_KD` in a writer's particular mood all
//! land in `unknown`, come back out of the writer unchanged, and shade as if the
//! tag had been what the model does not know about — which is exactly what
//! happened before this module existed.
//!
//! Case-insensitive tag dispatch is done ([`MAP_Kd`](MapKind::Diffuse) and
//! `map_kd` mean the same map); the *canonical* spelling is what the writer
//! emits, so a round trip normalizes tag case the same way the OBJ writer
//! normalizes `1.0` to `1`.

use std::path::Path;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// A file named by an `mtllib` line, and what became of it.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MaterialLib {
    /// The path exactly as the `mtllib` line spelled it (after unquoting), so it
    /// can be written back and resolved again.
    pub path: String,
    /// Whether the library was actually read.
    pub status: LibraryStatus,
}

/// Outcome of resolving one `mtllib` reference.
///
/// A missing `.mtl` is recorded, never fatal: OBJ files routinely ship their
/// materials separately or not at all, and the geometry is still worth loading.
/// Recording it rather than returning nothing is what keeps "no materials" and
/// "materials I could not find" distinguishable — the same instinct that makes
/// the OBJ parser report an unreadable corner instead of guessing.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LibraryStatus {
    /// The file was read and its materials merged.
    Loaded,
    /// The reference came from [`parse`](crate::parse::parse) — text with no directory
    /// to resolve against. Call [`Obj::resolve_material_libs`] with a base path.
    ///
    /// [`Obj::resolve_material_libs`]: crate::Obj::resolve_material_libs
    #[default]
    Unresolved,
    /// No file at that path.
    Missing,
    /// A file exists but could not be read (permissions, a directory, …).
    Unreadable,
}

impl MaterialLib {
    /// True when this library's materials made it into the model.
    #[must_use]
    pub fn is_loaded(&self) -> bool {
        self.status == LibraryStatus::Loaded
    }
}

/// One `newmtl` block: the surface properties a face is shaded with.
///
/// Every optional field is `None` when the file did not state it, which matters:
/// "no `Kd` written" and "`Kd 0 0 0`" are different instructions to a renderer.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Material {
    /// The name, exactly as written after `newmtl` — it may contain spaces.
    pub name: String,
    /// Index into [`Obj::material_libs`] for the library that defined this, or
    /// `None` for a material synthesised from a `usemtl` reference
    /// ( [`defined`](Self::defined) is then `false`) or one parsed straight out
    /// of a standalone `.mtl`.
    ///
    /// [`Obj::material_libs`]: crate::Obj::material_libs
    pub library: Option<u32>,
    /// False only for an entry that stands in for a `usemtl` name no loaded
    /// library defines. Such a material carries a name and nothing else, and
    /// keeps the reference alive through a round trip instead of silently
    /// un-assigning the faces that asked for it.
    pub defined: bool,
    /// `Ka` — ambient reflectance, RGB in `[0,1]` by convention, unchecked.
    pub ambient: Option<[f64; 3]>,
    /// `Kd` — diffuse reflectance.
    pub diffuse: Option<[f64; 3]>,
    /// `Ks` — specular reflectance.
    pub specular: Option<[f64; 3]>,
    /// `Ke` — emissive.
    pub emissive: Option<[f64; 3]>,
    /// `Ns` — specular exponent (shininess).
    pub shininess: Option<f64>,
    /// `d` — opacity; 1 is opaque.
    pub dissolve: Option<f64>,
    /// `Tr` — transmission, the inverse-dissolve spelling. Stored as written:
    /// converting it to `d` would be an opinion, not a reading.
    pub transmission: Option<f64>,
    /// `illum` — the illumination model index (0..=10 in the original spec).
    pub illumination: Option<u32>,
    /// Texture map references, in the order they were written.
    pub maps: Vec<Map>,
    /// Tags this model does not hold, verbatim, in the order they appeared.
    pub unknown: Vec<UnknownTag>,
}

impl Material {
    /// A material named by `usemtl` that no loaded library defines.
    #[must_use]
    pub fn referenced(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            library: None,
            defined: false,
            ..Default::default()
        }
    }

    /// The texture mapped onto diffuse colour, if any — the map most renderers
    /// mean when they say "the object's texture".
    #[must_use]
    pub fn diffuse_map(&self) -> Option<&Map> {
        self.maps.iter().find(|map| map.kind == MapKind::Diffuse)
    }

    /// The first map of `kind`, if any.
    #[must_use]
    pub fn map(&self, kind: MapKind) -> Option<&Map> {
        self.maps.iter().find(|map| map.kind == kind)
    }
}

/// A tag the model does not model, kept as the two pieces the writer needs to
/// reproduce it exactly.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UnknownTag {
    /// The keyword, e.g. `sharpini` or a `Kd` whose value was not three floats.
    pub tag: String,
    /// Everything after the keyword, verbatim (possibly empty).
    pub args: String,
}

/// Which surface property a `map_*` tag feeds.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MapKind {
    /// `map_Ka`
    Ambient,
    /// `map_Kd` — the colour texture.
    #[default]
    Diffuse,
    /// `map_Ks`
    Specular,
    /// `map_Ke`
    Emissive,
    /// `map_Ns` — roughness/shininess map.
    Shininess,
    /// `map_d` — alpha map.
    Dissolve,
    /// `map_bump` or `bump`: the spelling is not preserved (see the
    /// module docs); both read as this kind and write as `map_bump`.
    Bump,
    /// `disp` — displacement.
    Displacement,
    /// `decal`
    Decal,
}

impl MapKind {
    /// Recognise a map tag, case-insensitively. `bump` and `map_bump` both mean
    /// [`MapKind::Bump`].
    #[must_use]
    fn parse(tag: &str) -> Option<Self> {
        let lowered = tag.to_ascii_lowercase();
        Some(match lowered.as_str() {
            "map_ka" => Self::Ambient,
            "map_kd" => Self::Diffuse,
            "map_ks" => Self::Specular,
            "map_ke" => Self::Emissive,
            "map_ns" => Self::Shininess,
            "map_d" => Self::Dissolve,
            "map_bump" | "bump" => Self::Bump,
            "disp" => Self::Displacement,
            "decal" => Self::Decal,
            _ => return None,
        })
    }

    /// The canonical tag the writer emits.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Ambient => "map_Ka",
            Self::Diffuse => "map_Kd",
            Self::Specular => "map_Ks",
            Self::Emissive => "map_Ke",
            Self::Shininess => "map_Ns",
            Self::Dissolve => "map_d",
            Self::Bump => "map_bump",
            Self::Displacement => "disp",
            Self::Decal => "decal",
        }
    }
}

/// A texture reference: which property it shades, the file, and how to sample it.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Map {
    /// Which surface property this map drives.
    pub kind: MapKind,
    /// The texture file, exactly as written. Resolution against the `.mtl`'s own
    /// directory to an actual file is the caller's business — this crate does not
    /// open image files.
    pub path: String,
    /// The `-flag` options before the path, in the order written.
    pub options: Vec<MapOption>,
}

/// A texture-sampling option from a map tag.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub enum MapOption {
    /// `-s u [v] [w]` — texture scale. Kept at the arity written (1..=3).
    Scale(Vec<f64>),
    /// `-o u [v] [w]` — texture offset.
    Offset(Vec<f64>),
    /// `-bm <float>` — bump multiplier.
    BumpScale(f64),
    /// `-imfchan <c>` — which image channel to read.
    Channel(Channel),
    /// `-blendu on|off`
    BlendU(bool),
    /// `-blendv on|off`
    BlendV(bool),
    /// `-clamp on|off`
    Clamp(bool),
    /// `-texres <float>`
    TextureResolution(f64),
    /// A flag this model does not know, with the numeric arguments that followed
    /// it. Preserved so an exotic writer's options survive the round trip.
    Unknown {
        /// The flag as written, `-switch` included.
        flag: String,
        /// Arguments consumed after it.
        args: Vec<String>,
    },
}

/// An image channel named by `-imfchan`.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Channel {
    /// `r`
    Red,
    /// `g`
    Green,
    /// `b`
    Blue,
    /// `a`
    Alpha,
    /// `z`
    Zero,
    /// `l` — luminance.
    #[default]
    Luminance,
}

impl Channel {
    fn parse(value: &str) -> Option<Self> {
        Some(match value.to_ascii_lowercase().as_str() {
            "r" => Self::Red,
            "g" => Self::Green,
            "b" => Self::Blue,
            "a" => Self::Alpha,
            "z" => Self::Zero,
            "l" => Self::Luminance,
            _ => return None,
        })
    }

    /// The `-imfchan` letter for this channel.
    #[must_use]
    pub const fn letter(self) -> char {
        match self {
            Self::Red => 'r',
            Self::Green => 'g',
            Self::Blue => 'b',
            Self::Alpha => 'a',
            Self::Zero => 'z',
            Self::Luminance => 'l',
        }
    }
}

/// Parse `.mtl` text into materials. Total: nothing in here can fail.
///
/// Duplicate `newmtl` names keep the **first** definition and ignore later ones,
/// matching how the binding layer resolves a contested reference. Tags that
/// appear before any `newmtl` describe no material and are dropped.
#[must_use]
pub fn parse_mtl(text: &str) -> Vec<Material> {
    let mut materials: Vec<Material> = Vec::new();
    let mut current: Option<Material> = None;

    for line in text.lines() {
        let line = line.trim_end();
        let Some((tag, args)) = line.split_once(char::is_whitespace) else {
            // A bare word or an empty line. `newmtl` with no name is inert.
            let tag = line.trim();
            if tag.is_empty() || tag.starts_with('#') {
                continue;
            }
            if tag == "newmtl" {
                continue;
            }
            // A valueless tag inside a material is still a tag the file said.
            if let Some(mat) = current.as_mut() {
                mat.unknown.push(UnknownTag {
                    tag: tag.to_string(),
                    args: String::new(),
                });
            }
            continue;
        };
        let args = args.trim_start();
        if tag.starts_with('#') {
            continue;
        }

        if tag == "newmtl" {
            if let Some(done) = current.replace(Material {
                name: args.to_string(),
                defined: true,
                ..Default::default()
            }) {
                push_unique(&mut materials, done);
            }
            continue;
        }

        let Some(mat) = current.as_mut() else {
            continue; // tag outside any material: describes nothing
        };
        read_tag(mat, tag, args);
    }

    if let Some(done) = current {
        push_unique(&mut materials, done);
    }
    materials
}

fn push_unique(materials: &mut Vec<Material>, material: Material) {
    if materials.iter().any(|m| m.name == material.name) {
        return; // first definition wins
    }
    materials.push(material);
}

/// Read one `tag args` inside a material, or preserve it verbatim.
fn read_tag(mat: &mut Material, tag: &str, args: &str) {
    if let Some(kind) = MapKind::parse(tag) {
        if let Some(map) = read_map(kind, args) {
            mat.maps.push(map);
            return;
        }
    } else {
        let lowered = tag.to_ascii_lowercase();
        match lowered.as_str() {
            "ka" | "kd" | "ks" | "ke" => {
                if let Some(rgb) = read_rgb(args) {
                    match lowered.as_str() {
                        "ka" => mat.ambient = Some(rgb),
                        "kd" => mat.diffuse = Some(rgb),
                        "ks" => mat.specular = Some(rgb),
                        _ => mat.emissive = Some(rgb),
                    }
                    return;
                }
            }
            "ns" => {
                if let Ok(v) = args.parse::<f64>() {
                    mat.shininess = Some(v);
                    return;
                }
            }
            "d" => {
                if let Ok(v) = args.parse::<f64>() {
                    mat.dissolve = Some(v);
                    return;
                }
            }
            "tr" => {
                if let Ok(v) = args.parse::<f64>() {
                    mat.transmission = Some(v);
                    return;
                }
            }
            "illum" => {
                if let Ok(v) = args.parse::<u32>() {
                    mat.illumination = Some(v);
                    return;
                }
            }
            _ => {}
        }
    }
    mat.unknown.push(UnknownTag {
        tag: tag.to_string(),
        args: args.to_string(),
    });
}

/// Three whitespace-separated floats exactly — not "first three", so a spectral
/// colour reference or a stray fourth value falls through to `unknown` instead
/// of quietly becoming a colour.
fn read_rgb(args: &str) -> Option<[f64; 3]> {
    let mut it = args.split_whitespace();
    let a = it.next()?.parse().ok()?;
    let b = it.next()?.parse().ok()?;
    let c = it.next()?.parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    Some([a, b, c])
}

/// `map_*` body: option flags first, then the file — which may contain spaces,
/// so the path is *everything* left after the last consumed token.
fn read_map(kind: MapKind, args: &str) -> Option<Map> {
    let tokens: Vec<&str> = args.split_whitespace().collect();
    let mut options = Vec::new();
    let mut i = 0usize;
    while i < tokens.len() {
        let token = tokens[i];
        if !token.starts_with('-') || token.len() < 2 {
            break; // first non-flag token starts the path
        }
        let flag = token.to_ascii_lowercase();
        let take_numbers = |count: usize, i: &mut usize| -> Vec<f64> {
            let mut out = Vec::new();
            while *i + 1 < tokens.len() && out.len() < count {
                match tokens[*i + 1].parse::<f64>() {
                    Ok(v) => {
                        out.push(v);
                        *i += 1;
                    }
                    Err(_) => break,
                }
            }
            out
        };
        match flag.as_str() {
            "-s" => options.push(MapOption::Scale(take_numbers(3, &mut i))),
            "-o" => options.push(MapOption::Offset(take_numbers(3, &mut i))),
            "-bm" => {
                let v = take_numbers(1, &mut i);
                match v.first() {
                    Some(&v) => options.push(MapOption::BumpScale(v)),
                    None => break,
                }
            }
            "-texres" => {
                let v = take_numbers(1, &mut i);
                match v.first() {
                    Some(&v) => options.push(MapOption::TextureResolution(v)),
                    None => break,
                }
            }
            "-imfchan" => {
                let Some(value) = tokens.get(i + 1) else {
                    break;
                };
                let Some(ch) = Channel::parse(value) else {
                    break; // not a channel: let the path start here
                };
                options.push(MapOption::Channel(ch));
                i += 1;
            }
            "-blendu" | "-blendv" | "-clamp" => {
                let Some(value) = tokens.get(i + 1) else {
                    break;
                };
                let Some(on) = read_bool(value) else { break };
                options.push(match flag.as_str() {
                    "-blendu" => MapOption::BlendU(on),
                    "-blendv" => MapOption::BlendV(on),
                    _ => MapOption::Clamp(on),
                });
                i += 1;
            }
            _ => {
                // Unknown flag: take its numeric arguments with it, so a path
                // starting on the next token is not mistaken for one of them.
                let args = take_numbers(usize::MAX, &mut i);
                options.push(MapOption::Unknown {
                    flag: token.to_string(),
                    args: args.iter().map(|v| v.to_string()).collect(),
                });
            }
        }
        i += 1;
    }

    let path = tokens[i..].join(" ");
    if path.is_empty() {
        return None; // a map tag with no file says nothing to load
    }
    Some(Map {
        kind,
        path,
        options,
    })
}

fn read_bool(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "on" | "1" | "true" | "yes" => Some(true),
        "off" | "0" | "false" | "no" => Some(false),
        _ => None,
    }
}

/// Read and parse a `.mtl` file. Only the filesystem can fail here.
pub fn parse_mtl_file(path: impl AsRef<Path>) -> std::io::Result<Vec<Material>> {
    let text = std::fs::read_to_string(path)?;
    Ok(parse_mtl(&text))
}

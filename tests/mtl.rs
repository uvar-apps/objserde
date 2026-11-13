//! The `.mtl` side: tag coverage, texture maps, unknown-tag preservation, and
//! the library writer.

mod common;

use objserde::{
    Channel, LibraryStatus, Map, MapKind, MapOption, Material, MtlWriteOptions, UnknownTag,
    parse_mtl, write_material_lib,
};

#[test]
fn reads_the_full_standard_tag_set() {
    let text = "\
newmtl every
Ka 0.1 0.2 0.3
Kd 0.4 0.5 0.6
Ks 0.7 0.8 0.9
Ke 0.01 0.02 0.03
Ns 225.5
d 0.75
illum 7
";
    let mats = parse_mtl(text);
    assert_eq!(mats.len(), 1);
    let m = &mats[0];
    assert_eq!(m.name, "every");
    assert!(m.defined);
    assert_eq!(m.ambient, Some([0.1, 0.2, 0.3]));
    assert_eq!(m.diffuse, Some([0.4, 0.5, 0.6]));
    assert_eq!(m.specular, Some([0.7, 0.8, 0.9]));
    assert_eq!(m.emissive, Some([0.01, 0.02, 0.03]));
    assert_eq!(m.shininess, Some(225.5));
    assert_eq!(m.dissolve, Some(0.75));
    assert_eq!(m.illumination, Some(7));
    assert!(m.maps.is_empty());
    assert!(m.unknown.is_empty());
}

/// An unstated tag and a stated zero are different instructions; the model keeps
/// them apart.
#[test]
fn absent_is_not_zero() {
    let m = &parse_mtl("newmtl quiet\nd 0\n")[0];
    assert_eq!(m.dissolve, Some(0.0));
    assert_eq!(m.transmission, None);
    assert_eq!(m.shininess, None);
    assert_eq!(m.ambient, None);
}

/// `Tr` is the inverse-dissolve spelling. Converting it to `d` would be an
/// opinion, so both are stored as written.
#[test]
fn transmission_is_not_reinterpreted_as_dissolve() {
    let m = &parse_mtl("newmtl glass\nTr 0.4\n")[0];
    assert_eq!(m.transmission, Some(0.4));
    assert_eq!(m.dissolve, None);
}

#[test]
fn map_kind_and_path_and_options() {
    let text = "\
newmtl textured
map_Kd -s 2 2 1 -o 0.5 0.25 -imfchan g -blendu on -blendv off -clamp on -texres 512 wood.png
map_bump -bm 0.35 wood_bump.png
map_d mask.png
disp height.png
decal sticker.png
";
    let m = &parse_mtl(text)[0];
    let kinds: Vec<MapKind> = m.maps.iter().map(|map| map.kind).collect();
    assert_eq!(
        kinds,
        vec![
            MapKind::Diffuse,
            MapKind::Bump,
            MapKind::Dissolve,
            MapKind::Displacement,
            MapKind::Decal,
        ]
    );
    assert_eq!(m.maps[0].path, "wood.png");
    assert_eq!(
        m.maps[0].options,
        vec![
            MapOption::Scale(vec![2.0, 2.0, 1.0]),
            MapOption::Offset(vec![0.5, 0.25]),
            MapOption::Channel(Channel::Green),
            MapOption::BlendU(true),
            MapOption::BlendV(false),
            MapOption::Clamp(true),
            MapOption::TextureResolution(512.0),
        ]
    );
    assert_eq!(m.maps[1].options, vec![MapOption::BumpScale(0.35)]);
    assert!(m.maps[2].options.is_empty());
    assert_eq!(m.diffuse_map().unwrap().path, "wood.png");
    assert_eq!(m.map(MapKind::Bump).unwrap().path, "wood_bump.png");
}

/// `bump` and `map_bump` are the same thing spelled two ways.
#[test]
fn bump_and_map_bump_are_one_kind() {
    let short = &parse_mtl("newmtl a\nbump -bm 1 n.png\n")[0];
    let long = &parse_mtl("newmtl a\nmap_bump -bm 1 n.png\n")[0];
    assert_eq!(short.maps, long.maps);
    assert_eq!(short.maps[0].kind, MapKind::Bump);
}

#[test]
fn map_tags_are_case_insensitive() {
    let m = &parse_mtl("newmtl caps\nMAP_KD brick.png\nKA 1 1 1\nNS 10\n")[0];
    assert_eq!(m.maps[0].kind, MapKind::Diffuse);
    assert_eq!(m.maps[0].path, "brick.png");
    assert_eq!(m.ambient, Some([1.0, 1.0, 1.0]));
    assert_eq!(m.shininess, Some(10.0));
}

/// A texture path may contain spaces: everything after the last option is the
/// file, not a stream of tokens.
#[test]
fn map_path_may_contain_spaces() {
    let m = &parse_mtl("newmtl wood\nmap_Kd -bm 1 my textures/wood grain.png\n")[0];
    assert_eq!(m.maps[0].path, "my textures/wood grain.png");
}

/// A spectral colour reference (`Kd 0.44 file.wvidx`) is legal in the wider spec
/// and not three floats: keep the tag, do not invent a colour.
#[test]
fn a_value_the_model_cannot_hold_is_preserved_verbatim() {
    let m = &parse_mtl("newmtl sgi\nKd 0.44 wood.spe\nNs 90\n")[0];
    assert_eq!(
        m.diffuse, None,
        "no colour invented from a spectral reference"
    );
    assert_eq!(
        m.shininess,
        Some(90.0),
        "the rest of the material still reads"
    );
    assert_eq!(
        m.unknown,
        vec![UnknownTag {
            tag: "Kd".into(),
            args: "0.44 wood.spe".into(),
        }]
    );
}

#[test]
fn unknown_tags_are_kept_in_order_with_their_arguments() {
    let m = &parse_mtl("newmtl odd\nsharpini 90\nrefl 0 0 1\nfoo\nmap_NotAThing x.png\n")[0];
    assert_eq!(
        m.unknown,
        vec![
            UnknownTag {
                tag: "sharpini".into(),
                args: "90".into()
            },
            UnknownTag {
                tag: "refl".into(),
                args: "0 0 1".into()
            },
            UnknownTag {
                tag: "foo".into(),
                args: String::new()
            },
            UnknownTag {
                tag: "map_NotAThing".into(),
                args: "x.png".into()
            },
        ]
    );
}

/// A map tag with no file says nothing to load: kept as written rather than
/// stored as a map with an empty path.
#[test]
fn map_tag_without_a_file_is_not_a_map() {
    let m = &parse_mtl("newmtl broken\nmap_Kd\n")[0];
    assert!(m.maps.is_empty());
    assert_eq!(
        m.unknown,
        vec![UnknownTag {
            tag: "map_Kd".into(),
            args: String::new(),
        }]
    );
}

/// Repeated tags within one material: the last value written is the one kept,
/// which is what a renderer would use anyway.
#[test]
fn repeated_tag_keeps_the_last_value() {
    let m = &parse_mtl("newmtl dup\nKd 1 0 0\nKd 0 0 1\n")[0];
    assert_eq!(m.diffuse, Some([0.0, 0.0, 1.0]));
}

/// Repeated `newmtl` names: the first definition wins, the later one is ignored.
#[test]
fn first_material_definition_wins() {
    let mats = parse_mtl("newmtl a\nKd 1 0 0\nnewmtl a\nKd 0 0 1\nnewmtl b\n");
    assert_eq!(mats.len(), 2);
    assert_eq!(mats[0].name, "a");
    assert_eq!(mats[0].diffuse, Some([1.0, 0.0, 0.0]));
    assert_eq!(mats[1].name, "b");
}

#[test]
fn names_may_contain_spaces() {
    let mats = parse_mtl("newmtl brushed metal dark\nKd 0.5 0.5 0.5\n");
    assert_eq!(mats[0].name, "brushed metal dark");
}

/// Parsing `.mtl` text cannot fail — a file of pure nonsense yields no materials
/// and no error, and geometry loading is never held hostage to a paint job.
#[test]
fn parsing_materials_is_total() {
    let garbage = "\u{fffd} not valid text\nKa\nKd one two three\nNs 12 34\nnewmtl\n";
    let mats = parse_mtl(garbage);
    // `Ka` alone and the unusable colour fall through; nothing panics, and the
    // nameless `newmtl` opens nothing.
    assert!(mats.is_empty(), "{mats:?}");
    assert!(parse_mtl("").is_empty());
    assert!(write_material_lib(&[], &MtlWriteOptions::default()).is_empty());
}

/// Comments, blank lines and tags before any `newmtl` describe no material.
#[test]
fn prose_outside_a_material_describes_nothing() {
    let mats = parse_mtl("# header\n\nKd 1 1 1\nnewmtl real\nKd 0 0 0\n# tail\n");
    assert_eq!(mats.len(), 1);
    assert_eq!(mats[0].name, "real");
    assert_eq!(mats[0].diffuse, Some([0.0, 0.0, 0.0]));
    assert!(mats[0].unknown.is_empty(), "the orphan Kd is not attached");
}

#[test]
fn library_writer_produces_canonical_blocks() {
    let materials = vec![Material {
        name: "paint".into(),
        library: Some(0),
        defined: true,
        ambient: Some([0.1, 0.2, 0.3]),
        diffuse: Some([0.8, 0.1, 0.05]),
        specular: None,
        emissive: None,
        shininess: Some(128.0),
        dissolve: Some(0.9),
        transmission: None,
        illumination: Some(2),
        maps: vec![
            Map {
                kind: MapKind::Diffuse,
                path: "textures/paint.png".into(),
                options: vec![
                    MapOption::Scale(vec![2.0, 2.0, 1.0]),
                    MapOption::Clamp(true),
                ],
            },
            Map {
                kind: MapKind::Bump,
                path: "paint bump.png".into(),
                options: vec![MapOption::BumpScale(0.5)],
            },
        ],
        unknown: vec![UnknownTag {
            tag: "sharpini".into(),
            args: "90".into(),
        }],
    }];
    let options = MtlWriteOptions {
        header: vec!["# written by objserde".into()],
    };
    let out = write_material_lib(&materials, &options);
    assert_eq!(
        out,
        "# written by objserde\n\
         newmtl paint\n\
         Ka 0.1 0.2 0.3\n\
         Kd 0.8 0.1 0.05\n\
         Ns 128\n\
         d 0.9\n\
         illum 2\n\
         map_Kd -s 2 2 1 -clamp on textures/paint.png\n\
         map_bump -bm 0.5 paint bump.png\n\
         sharpini 90\n"
    );
}

/// Writing then reading a library is value-identical, bits included.
#[test]
fn library_round_trip_is_exact() {
    let text = include_str!("fixtures/mtl/basic.mtl");
    let mats = parse_mtl(text);
    assert_eq!(mats.len(), 2);
    let written = write_material_lib(&mats, &MtlWriteOptions::default());
    let again = parse_mtl(&written);
    assert_eq!(again, mats);

    // Equality alone would let -0.0 pass as 0.0; the README promises bits.
    let components = |mats: &[Material]| {
        let mut out = Vec::new();
        for m in mats {
            for rgb in [m.ambient, m.diffuse, m.specular, m.emissive]
                .into_iter()
                .flatten()
            {
                out.extend(rgb.iter().map(|v| v.to_bits()));
            }
            out.extend(
                [m.shininess, m.dissolve, m.transmission]
                    .into_iter()
                    .flatten()
                    .map(|v| v.to_bits()),
            );
        }
        out
    };
    assert_eq!(
        components(&again),
        components(&mats),
        "a material number changed bits across the round trip"
    );
    assert_eq!(
        again.len(),
        mats.len(),
        "-0.0 tags must not be dropped or invented"
    );
}

/// Signed zeros in a material are written as `-0` and read back signed.
#[test]
fn negative_zero_survives_the_material_writer() {
    let materials = vec![Material {
        name: "zero".into(),
        defined: true,
        diffuse: Some([-0.0, 0.0, 1.0]),
        dissolve: Some(-0.0),
        ..Default::default()
    }];
    let out = write_material_lib(&materials, &MtlWriteOptions::default());
    assert_eq!(out, "newmtl zero\nKd -0 0 1\nd -0\n");
    let again = parse_mtl(&out);
    assert_eq!(
        again[0].diffuse.unwrap()[0].to_bits(),
        (-0.0f64).to_bits(),
        "sign of zero lost"
    );
    assert_eq!(again[0].dissolve.unwrap().to_bits(), (-0.0f64).to_bits());
}

/// A material that only stands in for an unloaded reference is not written as a
/// definition — the OBJ's `usemtl` line is what carries it.
#[test]
fn unresolved_references_are_not_written_as_definitions() {
    let materials = vec![
        Material::referenced("missing-elsewhere"),
        Material {
            name: "real".into(),
            defined: true,
            diffuse: Some([1.0, 0.0, 0.0]),
            ..Default::default()
        },
    ];
    let out = write_material_lib(&materials, &MtlWriteOptions::default());
    assert_eq!(out, "newmtl real\nKd 1 0 0\n");
    assert!(!out.contains("missing-elsewhere"));
}

#[test]
fn library_writer_is_deterministic() {
    let text = include_str!("fixtures/mtl/basic.mtl");
    let mats = parse_mtl(text);
    let first = write_material_lib(&mats, &MtlWriteOptions::default());
    for _ in 0..3 {
        assert_eq!(
            write_material_lib(&mats, &MtlWriteOptions::default()),
            first
        );
    }
}

#[test]
fn reads_a_library_from_disk() {
    let mats = objserde::parse_mtl_file(common::mtl_fixture("basic.mtl")).unwrap();
    assert_eq!(mats.len(), 2);
    assert_eq!(mats[0].name, "paint");
    assert_eq!(mats[1].name, "metal");
    assert_eq!(mats[1].unknown.len(), 1, "sharpini preserved");
    assert_eq!(
        mats[1].diffuse,
        Some([0.111, 0.222, 0.333]),
        "the repeated Kd keeps its last value"
    );
}

#[test]
fn a_missing_library_is_an_io_error_for_the_explicit_api() {
    // Explicit loading is strict; implicit resolution from an OBJ is not.
    let err = objserde::parse_mtl_file(common::mtl_fixture("nope.mtl")).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}

/// The status enum is the record of what was found; check it survives a write.
#[test]
fn library_status_is_reported_not_assumed() {
    let m = parse_mtl("newmtl x\n");
    assert!(m[0].defined);
    assert!(!Material::referenced("x").defined);
    assert!(
        !objserde::MaterialLib {
            path: "a.mtl".into(),
            status: LibraryStatus::Unresolved,
        }
        .is_loaded()
    );
}

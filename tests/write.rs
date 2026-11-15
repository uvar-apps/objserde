//! Writer contracts: Mode B must reproduce the `molses` line grammar exactly,
//! Mode A must write a shape-preserving file, and both must be deterministic.

mod common;

use objserde::{
    BoundMesh, Corner, Face, Obj, Precision, TexCoord, Vertex, WriteOptions, write_mesh,
};

/// The three-vertex triangle from the `molses` writer's own test.
fn triple_mesh() -> BoundMesh {
    BoundMesh {
        positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        unit_normals: vec![[0.0, 0.0, 1.0]; 3],
        triangles: vec![[0, 1, 2]],
    }
}

#[test]
fn mode_b_matches_the_molses_line_grammar() {
    let out = write_mesh(&triple_mesh(), &WriteOptions::default());
    // The three assertions molses makes about its own output, verbatim.
    assert!(out.contains("v 0.000000 0.000000 0.000000"));
    assert!(out.contains("vn 0.000000 0.000000 1.000000"));
    assert!(out.contains("f 1//1 2//2 3//3"));

    // And the whole file, byte for byte, with no header.
    let expected = "v 0.000000 0.000000 0.000000\n\
                    v 1.000000 0.000000 0.000000\n\
                    v 0.000000 1.000000 0.000000\n\
                    vn 0.000000 0.000000 1.000000\n\
                    vn 0.000000 0.000000 1.000000\n\
                    vn 0.000000 0.000000 1.000000\n\
                    f 1//1 2//2 3//3\n";
    assert_eq!(out, expected);
}

#[test]
fn mode_b_default_precision_is_six_fixed_decimals() {
    assert_eq!(
        WriteOptions::default().precision,
        Precision::Fixed(objserde::DEFAULT_PRECISION)
    );
    assert_eq!(objserde::DEFAULT_PRECISION, 6);

    // Six decimals, and negative zero keeps its sign the way `{:.6}` prints it.
    let mesh = BoundMesh {
        positions: vec![[-0.0, 1.5, -0.25]],
        unit_normals: vec![[0.123456789, -1.0, 2.0]],
        triangles: vec![],
    };
    let out = write_mesh(&mesh, &WriteOptions::default());
    let first = out.lines().next().unwrap();
    assert_eq!(first, "v -0.000000 1.500000 -0.250000");
    assert_eq!(
        out.lines().nth(1).unwrap(),
        "vn 0.123457 -1.000000 2.000000"
    );
}

#[test]
fn mode_b_writes_header_lines_verbatim() {
    let options = WriteOptions {
        precision: Precision::default(),
        header: vec![
            "# molses 0.2.0".into(),
            "# source: testdata/1crn.pdb".into(),
            "".into(),
            "no hash, still verbatim".into(),
        ],
    };
    let out = write_mesh(&triple_mesh(), &options);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "# molses 0.2.0");
    assert_eq!(lines[1], "# source: testdata/1crn.pdb");
    assert_eq!(lines[2], "");
    assert_eq!(lines[3], "no hash, still verbatim");
    assert_eq!(lines[4], "v 0.000000 0.000000 0.000000");

    // The header is a parameter, not a clock: writing twice gives one string.
    assert_eq!(out, write_mesh(&triple_mesh(), &options));
}

#[test]
fn mode_b_shortest_precision_writes_the_shortest_form() {
    let options = WriteOptions {
        precision: Precision::Shortest,
        header: vec![],
    };
    let out = write_mesh(&triple_mesh(), &options);
    assert!(out.contains("v 0 0 0\n"), "{out}");
    assert!(out.contains("vn 0 0 1\n"), "{out}");
    assert!(out.contains("f 1//1 2//2 3//3"), "{out}");
}

/// Mode A must reproduce a hand-built fixture exactly, keeping each record's
/// shape: trailing floats on `v`, `vt` arity, and the corner form per corner.
#[test]
fn mode_a_writes_a_shape_preserving_file() {
    let obj = Obj {
        vertices: vec![
            Vertex::new([0.0, 0.0, 0.0]),
            Vertex {
                // An xyzrgb-family export: three coordinates, three colours.
                position: [1.0, 0.0, 0.0],
                extra: vec![0.0, 128.0 / 255.0, 0.0],
            },
            Vertex::new([0.25, 0.5, -0.75]),
            Vertex::new([0.0, 1.0, 0.0]),
        ],
        normals: vec![[0.0, 0.0, 1.0], [0.0, 0.0, 2.5]],
        tex_coords: vec![
            TexCoord::new(&[0.5]).unwrap(),
            TexCoord::new(&[0.25, 0.75]).unwrap(),
            TexCoord::new(&[1.0, 0.0, 3.0]).unwrap(),
        ],
        faces: vec![
            // One corner of each supported form, and a 4-corner polygon.
            Face {
                corners: vec![
                    Corner::position(0),
                    Corner {
                        v: 1,
                        vt: Some(0),
                        vn: None,
                    },
                    Corner {
                        v: 2,
                        vt: None,
                        vn: Some(1),
                    },
                ],
                ..Default::default()
            },
            Face {
                corners: vec![
                    Corner {
                        v: 3,
                        vt: Some(2),
                        vn: Some(0),
                    },
                    Corner::position(0),
                    Corner::position(2),
                    Corner {
                        v: 1,
                        vt: Some(1),
                        vn: Some(1),
                    },
                ],
                ..Default::default()
            },
        ],
        // No materials at all: this golden must stay byte-identical to what the
        // crate wrote before material support existed.
        ..Default::default()
    };

    let expected = "v 0 0 0\n\
                    v 1 0 0 0 0.5019607843137255 0\n\
                    v 0.25 0.5 -0.75\n\
                    v 0 1 0\n\
                    vt 0.5\n\
                    vt 0.25 0.75\n\
                    vt 1 0 3\n\
                    vn 0 0 1\n\
                    vn 0 0 2.5\n\
                    f 1 2/1 3//2\n\
                    f 4/3/1 1 3 2/2/2\n";
    assert_eq!(obj.to_string(), expected);

    // Reading it back gives the same model, including vt arity 1/2/3 and the
    // vertex colour triple.
    let again = objserde::parse(&obj.to_string()).unwrap();
    assert_eq!(again, obj);
    assert_eq!(again.tex_coords[0].arity, 1);
    assert_eq!(again.tex_coords[2].arity, 3);
    assert_eq!(again.vertices[1].extra.len(), 3);
}

#[test]
fn mode_a_reorders_records_into_canonical_layout() {
    // The parser stores the four tables separately, so a file whose records are
    // interleaved writes back grouped — same model, normalized order.
    let obj = objserde::parse("f 1 2 3\nv 0 0 0\nv 1 0 0\nv 0 1 0\n").unwrap();
    assert_eq!(obj.to_string(), "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n");
}

#[test]
fn a_face_with_no_corners_writes_nothing() {
    let obj = Obj {
        vertices: vec![Vertex::new([0.0, 0.0, 0.0])],
        faces: vec![Face {
            corners: vec![],
            ..Default::default()
        }],
        ..Default::default()
    };
    assert_eq!(obj.to_string(), "v 0 0 0\n");
}

#[test]
fn skipped_records_do_not_come_back() {
    // Documented limit, stated as a test: comments after the first record and
    // smoothing groups are not modelled, so a round trip through Mode A drops
    // them. (`usemtl` moved out of this list in 0.2, and the header comments,
    // `o` and `g` in 0.3 — see the test below and tests/materials.rs.)
    let src = "# generated by something\ng group_one\ns off\nv 0 0 0\n# later\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";
    let obj = objserde::parse(src).unwrap();
    let out = obj.to_string();
    assert!(!out.contains("s off"), "no smoothing record: {out}");
    assert!(!out.contains("# later"), "{out}");
    assert!(out.starts_with("# generated by something\n"), "{out}");
    assert!(out.contains("\ng group_one\nf 1 2 3\n"), "{out}");
    assert_eq!(obj.triangulate().len(), 1);
}

/// The header comes first; `o` and `g` appear among the faces exactly where
/// they change, in the order `o`, `g`, `usemtl`, and a bare `g` is written bare.
#[test]
fn header_objects_and_groups_are_written_where_they_change() {
    let src = "\
# sesmesh 1.1.2
# area_A2: 2314.2175
o ses
v 0 0 0
v 1 0 0
v 0 1 0
g component_0
usemtl paint
f 1 2 3
f 1 2 3
g cavity_1
f 1 2 3
g
f 1 2 3
";
    let obj = objserde::parse(src).unwrap();
    let out = obj.to_string();
    assert_eq!(
        out,
        "# sesmesh 1.1.2\n# area_A2: 2314.2175\n\
         v 0 0 0\nv 1 0 0\nv 0 1 0\n\
         o ses\ng component_0\nusemtl paint\nf 1 2 3\nf 1 2 3\n\
         g cavity_1\nf 1 2 3\n\
         g\nf 1 2 3\n"
    );
    assert_eq!(objserde::parse(&out).unwrap(), obj);
}

#[test]
fn writers_are_deterministic_across_repeated_calls() {
    let path = common::sample("1crn_ses_0.5A.obj");
    let obj = objserde::parse_file(&path).unwrap();
    let first = obj.to_string();
    for _ in 0..3 {
        assert_eq!(obj.to_string(), first, "Mode A bytes moved between calls");
    }
    let mesh = obj.bind();
    let options = WriteOptions {
        precision: Precision::Fixed(6),
        header: vec!["# determinism".into()],
    };
    let first_mesh = write_mesh(&mesh, &options);
    for _ in 0..3 {
        assert_eq!(
            write_mesh(&mesh, &options),
            first_mesh,
            "Mode B bytes moved between calls"
        );
    }
    // A model that came from the same bytes writes the same bytes again.
    let again = objserde::parse(&first).unwrap();
    assert_eq!(again.to_string(), first);
}

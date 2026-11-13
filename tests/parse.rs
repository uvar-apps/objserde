//! The parser's own contract: which lines are read, which are skipped, how
//! bytes and line endings are handled, what the errors say.

use std::error::Error as _;

use objserde::ObjError;

/// Everything the model does not hold is skipped rather than fatal. (`usemtl`
/// used to live in this list; it is modelled now — see tests/materials.rs.)
#[test]
fn unknown_and_unmodelled_records_are_skipped_not_fatal() {
    let src = "\
# a comment
o widget
g group_name
s 1
vp 0.1 0.2 0.3
vnp 0 0 1
cstyp
#{no space}

v 0 0 0
v 1 0 0
v 0 1 0
f 1 2 3
";
    let obj = objserde::parse(src).unwrap();
    assert_eq!(obj.vertices.len(), 3);
    assert_eq!(obj.faces.len(), 1);
    assert!(obj.normals.is_empty());
    assert!(obj.tex_coords.is_empty());
}

#[test]
fn line_prefixes_match_exactly() {
    // A keyword needs its space; anything else is not a vertex line.
    let obj = objserde::parse("v 0 0 0\nvertex 1 1 1\nv\t9 9 9\nvv 2 2 2\nv-1 -1 -1\n").unwrap();
    assert_eq!(obj.vertices.len(), 1, "only the `v ` line counts");
    assert_eq!(obj.vertices[0].position, [0.0, 0.0, 0.0]);

    // Leading whitespace makes it not a record either, as in the reference reader.
    let obj = objserde::parse("v 0 0 0\n v 1 0 0\n").unwrap();
    assert_eq!(obj.vertices.len(), 1);
}

#[test]
fn tabs_and_runs_of_whitespace_separate_fields() {
    let obj = objserde::parse("v 0\t\t1   2  \t 3\nv 4 5\t6\nv 7 8\t\t9\nf 1   2    3\n").unwrap();
    // Three coordinates, and the fourth field lands in `extra` like any other.
    assert_eq!(obj.vertices[0].position, [0.0, 1.0, 2.0]);
    assert_eq!(obj.vertices[0].extra, vec![3.0]);
    assert_eq!(obj.vertices[1].position, [4.0, 5.0, 6.0]);
    assert_eq!(obj.vertices[2].position, [7.0, 8.0, 9.0]);
    assert_eq!(obj.faces[0].corners.len(), 3);
}

#[test]
fn a_tab_after_the_keyword_is_not_the_record_delimiter() {
    // The prefix is `v ` with a space, exactly. `v\t…` is not a vertex line —
    // it is the same "match the prefix exactly" rule that makes `vertex 1 1 1`
    // not one either, and it is inherited from the reference reader.
    let obj = objserde::parse("v 0 0 0\nv\t9 9 9\n").unwrap();
    assert_eq!(obj.vertices.len(), 1);
    assert_eq!(obj.vertices[0].position, [0.0, 0.0, 0.0]);
}

#[test]
fn crlf_line_endings_parse() {
    let src = "v 0 0 0\r\nv 1 0 0\r\nv 0 1 0\r\nvn 0 0 1\r\nf 1//1 2//1 3//1\r\n";
    let mesh = objserde::parse(src).unwrap().bind();
    assert_eq!(mesh.triangle_count(), 1);
    assert_eq!(mesh.unit_normals[0], [0.0, 0.0, 1.0]);
}

#[test]
fn bytes_decode_lossily_so_a_bad_comment_cannot_kill_the_file() {
    let mut bytes = b"# comment ".to_vec();
    bytes.extend_from_slice(&[0xff, 0xfe]); // not UTF-8
    bytes.extend_from_slice(b"\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n");
    let obj = objserde::parse_bytes(&bytes).unwrap();
    assert_eq!(obj.vertices.len(), 3);
    assert_eq!(obj.triangulate().len(), 1);
}

#[test]
fn a_bad_byte_inside_a_record_still_reports_the_line() {
    // Lossy decoding turns an invalid byte into U+FFFD, which is not a number:
    // the error lands on the line that actually held the garbage.
    let mut bytes = b"v 0 0 0\nv 1 ".to_vec();
    bytes.extend_from_slice(&[0xff]);
    bytes.extend_from_slice(b" 0\n");
    let err = objserde::parse_bytes(&bytes).unwrap_err();
    match err {
        ObjError::BadFloat { line, .. } => assert_eq!(line, 2),
        other => panic!("expected BadFloat, got {other}"),
    }
}

#[test]
fn trailing_fields_on_a_vertex_are_kept_in_order() {
    let obj = objserde::parse("v 1 2 3 4 5 6 7 8\n").unwrap();
    assert_eq!(obj.vertices[0].position, [1.0, 2.0, 3.0]);
    assert_eq!(obj.vertices[0].extra, vec![4.0, 5.0, 6.0, 7.0, 8.0]);
}

#[test]
fn extra_fields_past_three_on_vn_are_ignored() {
    // The normal table is three components wide; a coloured `vn` export keeps
    // its first three, as the reference loader does.
    let obj = objserde::parse("v 0 0 0\nvn 0 0 2 abc\n").unwrap();
    assert_eq!(obj.normals[0], [0.0, 0.0, 2.0]);
}

#[test]
fn texture_coordinates_keep_their_arity() {
    let obj = objserde::parse("v 0 0 0\nvt 0.5\nvt 0.25 0.75\nvt 1 2 3\n").unwrap();
    let arities: Vec<u8> = obj.tex_coords.iter().map(|t| t.arity).collect();
    assert_eq!(arities, vec![1, 2, 3]);
    assert_eq!(obj.tex_coords[0].as_slice(), &[0.5]);
    assert_eq!(obj.tex_coords[2].as_slice(), &[1.0, 2.0, 3.0]);
}

#[test]
fn fields_past_the_third_on_vt_are_dropped() {
    // Outside what OBJ defines: keep the three it defines, ignore the rest.
    let obj = objserde::parse("v 0 0 0\nvt 1 2 3 4 5\n").unwrap();
    assert_eq!(obj.tex_coords[0].arity, 3);
    assert_eq!(obj.tex_coords[0].as_slice(), &[1.0, 2.0, 3.0]);
}

#[test]
fn bare_record_lines_name_the_missing_field() {
    for line in ["v \n", "vn \n"] {
        let src = format!("v 0 0 0\n{line}");
        let err = objserde::parse(&src).unwrap_err();
        assert!(
            matches!(&err, ObjError::BadFloat { line: 2, got } if got == "<missing field>"),
            "{line:?} -> {err}"
        );
    }
}

#[test]
fn empty_texture_coordinate_line_is_an_error() {
    let err = objserde::parse("v 0 0 0\nvt \n").unwrap_err();
    assert!(matches!(&err, ObjError::BadFloat { line: 2, .. }), "{err}");
}

#[test]
fn a_face_line_with_no_corners_produces_no_face() {
    let obj = objserde::parse("v 0 0 0\nf \nf\t\nf 1 1 1\n").unwrap();
    assert_eq!(obj.faces.len(), 1);
    assert_eq!(obj.faces[0].corners.len(), 3);
}

#[test]
fn unset_zero_texture_or_normal_index_means_absent() {
    // OBJ spells "none" as 0; it must not become table entry 1.
    let obj = objserde::parse("v 0 0 0\nvt 0.5\nvn 0 0 1\nf 1/0/0 1/0 1//0\n").unwrap();
    for corner in &obj.faces[0].corners {
        assert_eq!(corner.vt, None, "0 is unset, not index 0");
        assert_eq!(corner.vn, None, "0 is unset, not index 0");
    }
    assert_eq!(
        obj.bind().unit_normals[0],
        [0.0, 0.0, 0.0],
        "nothing bound, nothing referenced: no direction"
    );
}

#[test]
fn negative_texture_or_normal_index_is_refused() {
    for corner in ["1/1/-1", "1//-1", "1/-1"] {
        let src = format!("v 0 0 0\nvt 0 0\nvn 0 0 1\nf {corner} {corner} {corner}\n");
        let err = objserde::parse(&src).unwrap_err();
        match err {
            ObjError::NegativeIndex { line, value } => {
                assert_eq!(line, 4, "{corner} blamed the wrong line");
                assert_eq!(value, -1);
            }
            other => panic!("expected NegativeIndex for {corner}, got {other}"),
        }
    }
}

#[test]
fn extra_slash_fields_on_a_corner_are_ignored_like_the_reference_reader() {
    // `a/b/c/d`: the reference loader reads a and c and ignores d, so this file
    // still loads rather than dying on the surplus field.
    let obj = objserde::parse("v 0 0 0\nvt 1 1\nvn 0 0 1\nf 1/1/1/9 1/1/1/9 1/1/1/9\n").unwrap();
    assert_eq!(obj.faces[0].corners[0].vn, Some(0));
    assert_eq!(obj.faces[0].corners[0].vt, Some(0));
}

#[test]
fn an_index_past_the_addressable_space_is_out_of_range_not_wrapped() {
    let src = "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 4294967296\n";
    let err = objserde::parse(src).unwrap_err();
    match err {
        ObjError::IndexOutOfRange { line, value, .. } => {
            assert_eq!(line, 4);
            assert_eq!(value, 4294967296, "reported as written");
        }
        other => panic!("expected IndexOutOfRange, got {other}"),
    }
}

#[test]
fn corner_indices_are_zero_based_in_the_model() {
    let obj = objserde::parse("v 0 0 0\nv 1 0 0\nvn 0 0 1\nf 1//1 2//1 2//1\n").unwrap();
    assert_eq!(obj.faces[0].corners[0].v, 0);
    assert_eq!(obj.faces[0].corners[1].v, 1);
    assert_eq!(obj.faces[0].corners[0].vn, Some(0));
}

#[test]
fn an_index_far_past_the_addressable_space_is_reported_not_clamped() {
    let src = "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 99999999999\n";
    let err = objserde::parse(src).unwrap_err();
    match err {
        ObjError::IndexOutOfRange { line, value, .. } => {
            assert_eq!(line, 4);
            assert_eq!(
                value, 99999999999,
                "the number in the file, not a clamped one"
            );
        }
        other => panic!("expected IndexOutOfRange, got {other}"),
    }
}

#[test]
fn missing_file_reports_the_path() {
    let err = objserde::parse_file("/nonexistent/definitely-not-here.obj").unwrap_err();
    match err {
        ObjError::Io { path, .. } => {
            assert!(path.to_string_lossy().contains("definitely-not-here.obj"));
        }
        other => panic!("expected Io, got {other}"),
    }
    assert!(
        objserde::parse_file("/nonexistent/x.obj")
            .unwrap_err()
            .source()
            .is_some(),
        "the OS error stays reachable through source()"
    );
}

#[test]
fn a_directory_is_an_io_error_not_a_panic() {
    let err = objserde::parse_file(env!("CARGO_MANIFEST_DIR")).unwrap_err();
    assert!(matches!(err, ObjError::Io { .. }), "{err}");
}

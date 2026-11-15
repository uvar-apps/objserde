//! Materials as an OBJ feature: `usemtl` state on faces, `mtllib` resolution
//! against the file's own directory, and what absence looks like.

mod common;

use objserde::{LibraryStatus, parse};

#[test]
fn usemtl_is_state_not_a_per_face_field() {
    let obj = parse(
        "v 0 0 0\nv 1 0 0\nv 0 1 0\nv 1 1 0\n\
         usemtl first\nf 1 2 3\nf 1 3 4\n\
         usemtl second\nf 1 2 4\n",
    )
    .unwrap();
    assert_eq!(obj.materials.len(), 2);
    assert_eq!(
        obj.faces.iter().map(|f| f.material).collect::<Vec<_>>(),
        vec![Some(0), Some(0), Some(1)],
        "a material applies to every face until the next usemtl"
    );
    assert_eq!(obj.material_of(&obj.faces[0]).unwrap().name, "first");
    assert_eq!(obj.material_of(&obj.faces[2]).unwrap().name, "second");
}

#[test]
fn a_file_without_materials_stays_empty() {
    let obj = parse("v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n").unwrap();
    assert!(obj.materials.is_empty());
    assert!(obj.material_libs.is_empty());
    assert_eq!(obj.faces[0].material, None);
}

/// The samples predate material support; they must not gain phantom materials.
#[test]
fn samples_still_carry_no_materials() {
    let obj = objserde::parse_file(common::sample("1crn_ses_0.5A.obj")).unwrap();
    assert!(obj.materials.is_empty());
    assert!(obj.material_libs.is_empty());
    assert_eq!(obj.vertices.len(), 37_930);
}

#[test]
fn text_parse_leaves_libraries_unresolved_but_references_intact() {
    let obj = parse("mtllib mats.mtl\nv 0 0 0\nv 1 0 0\nv 0 1 0\nusemtl paint\nf 1 2 3\n").unwrap();
    assert_eq!(obj.material_libs.len(), 1);
    assert_eq!(obj.material_libs[0].path, "mats.mtl");
    assert_eq!(obj.material_libs[0].status, LibraryStatus::Unresolved);
    // The face keeps its reference even though nothing has been read yet.
    assert_eq!(obj.faces[0].material, Some(0));
    assert!(!obj.materials[0].defined, "a reference, not a definition");
}

#[test]
fn parse_file_loads_the_sibling_library() {
    let obj = objserde::parse_file(common::mtl_fixture("basic.obj")).unwrap();
    assert_eq!(obj.faces.len(), 3);
    assert_eq!(obj.material_libs.len(), 1);
    assert_eq!(
        obj.material_libs[0].status,
        LibraryStatus::Loaded,
        "basic.mtl sits next to basic.obj"
    );

    assert_eq!(obj.materials.len(), 2);
    let paint = obj.material_of(&obj.faces[0]).unwrap();
    assert_eq!(paint.name, "paint");
    assert!(paint.defined);
    assert_eq!(paint.diffuse, Some([0.8, 0.1, 0.05]));
    assert_eq!(
        paint.diffuse_map().unwrap().path,
        "textures/paint.png",
        "the texture reference is attached to the material that owns it"
    );
    let metal = obj.material_of(&obj.faces[2]).unwrap();
    assert_eq!(metal.name, "metal");
    assert_eq!(metal.illumination, Some(7));
}

/// Faces keep pointing at the right material once the library fills in.
#[test]
fn faces_see_the_loaded_definitions() {
    let obj = objserde::parse_file(common::mtl_fixture("basic.obj")).unwrap();
    let by_name: Vec<(&str, usize)> = (0..obj.materials.len() as u32)
        .map(|i| {
            (
                obj.materials[i as usize].name.as_str(),
                obj.faces_using(i).count(),
            )
        })
        .collect();
    assert_eq!(by_name, vec![("paint", 2), ("metal", 1)]);
}

#[test]
fn a_missing_library_is_recorded_not_fatal() {
    let obj = objserde::parse_file(common::mtl_fixture("missing.obj")).unwrap();
    assert_eq!(obj.faces.len(), 1, "the geometry loaded anyway");
    assert_eq!(obj.material_libs[0].status, LibraryStatus::Missing);
    // The reference survives as an unresolved entry rather than vanishing.
    assert_eq!(obj.materials.len(), 1);
    assert!(!obj.materials[0].defined);
    assert_eq!(obj.faces[0].material, Some(0));
}

#[test]
fn an_unreadable_library_is_distinguished_from_a_missing_one() {
    // `dir-as-library.mtl` is a directory: present, but not readable as a file.
    let obj = objserde::parse_file(common::mtl_fixture("unreadable.obj")).unwrap();
    assert_eq!(obj.material_libs[0].status, LibraryStatus::Unreadable);
}

#[test]
fn one_line_may_name_several_libraries() {
    let obj = objserde::parse_file(common::mtl_fixture("two_libs.obj")).unwrap();
    assert_eq!(obj.material_libs.len(), 2);
    assert_eq!(obj.material_libs[0].path, "first.mtl");
    assert_eq!(obj.material_libs[1].path, "second.mtl");
    assert!(obj.material_libs.iter().all(|l| l.is_loaded()));
    // A name defined twice keeps the first library's definition...
    let shared = obj.material_named("shared").unwrap();
    assert_eq!(
        obj.materials[shared as usize].diffuse,
        Some([1.0, 0.0, 0.0])
    );
    assert_eq!(obj.materials[shared as usize].library, Some(0));
    // ...and the second library's other materials still arrive.
    assert!(obj.material_named("only-second").is_some());
}

#[test]
fn quoted_library_names_survive_the_trip() {
    let obj = objserde::parse_file(common::mtl_fixture("quoted.obj")).unwrap();
    assert_eq!(obj.material_libs[0].path, "quoted mats.mtl");
    assert_eq!(obj.material_libs[0].status, LibraryStatus::Loaded);
    assert_eq!(obj.materials[0].name, "an explicit name");
    // A name with spaces comes back as one name, quoted on the way out, after
    // the file's header comment.
    let text = obj.to_string();
    assert!(
        text.starts_with("# library names with spaces, quoted\nmtllib \"quoted mats.mtl\"\n"),
        "{text}"
    );
    let again = parse(&text).unwrap();
    assert_eq!(again.material_libs[0].path, "quoted mats.mtl");
}

/// A `usemtl` that no library answers still names the faces that asked for it.
#[test]
fn a_reference_with_no_definition_keeps_its_faces() {
    let obj = parse("v 0 0 0\nv 1 0 0\nv 0 1 0\nusemtl ghost\nf 1 2 3\n").unwrap();
    assert_eq!(obj.materials.len(), 1);
    assert_eq!(obj.materials[0].name, "ghost");
    assert!(!obj.materials[0].defined);
    assert_eq!(obj.faces_using(0).count(), 1);
    // Writing keeps the reference; reading re-synthesizes the same entry.
    assert_eq!(objserde::parse(&obj.to_string()).unwrap(), obj);
}

#[test]
fn resolving_after_a_text_parse_works() {
    let mut obj =
        parse("mtllib basic.mtl\nv 0 0 0\nv 1 0 0\nv 0 1 0\nusemtl metal\nf 1 2 3\n").unwrap();
    assert_eq!(obj.material_libs[0].status, LibraryStatus::Unresolved);
    assert!(!obj.materials[0].defined);

    obj.resolve_material_libs(&common::mtl_fixture(""));
    assert_eq!(obj.material_libs[0].status, LibraryStatus::Loaded);
    // The reference entry became the real definition, indices unchanged.
    assert!(obj.materials[0].defined);
    assert_eq!(obj.materials[0].shininess, Some(400.0));
    assert_eq!(obj.faces[0].material, Some(0));

    // Idempotent: resolving again does not duplicate or renumber.
    let before = obj.materials.clone();
    obj.resolve_material_libs(&common::mtl_fixture(""));
    assert_eq!(obj.materials, before);
}

#[test]
fn material_names_may_contain_spaces_on_both_sides() {
    let obj = parse("v 0 0 0\nv 1 0 0\nv 0 1 0\nusemtl brushed metal\nf 1 2 3\n").unwrap();
    assert_eq!(obj.materials[0].name, "brushed metal");
    assert!(obj.to_string().contains("usemtl brushed metal"));
    assert_eq!(objserde::parse(&obj.to_string()).unwrap(), obj);
}

#[test]
fn faces_are_written_under_the_material_that_owns_them() {
    let text = "mtllib mats.mtl\n\
                v 0 0 0\nv 1 0 0\nv 0 1 0\nv 1 1 0\n\
                usemtl a\nf 1 2 3\n\
                usemtl b\nf 2 3 4\n";
    let obj = parse(text).unwrap();
    let out = obj.to_string();
    assert_eq!(
        out,
        "mtllib mats.mtl\n\
         v 0 0 0\nv 1 0 0\nv 0 1 0\nv 1 1 0\n\
         usemtl a\nf 1 2 3\n\
         usemtl b\nf 2 3 4\n"
    );
    assert_eq!(objserde::parse(&out).unwrap(), obj);
}

/// One `usemtl` per change, not one per face: a thousand faces under one material
/// cost one line.
#[test]
fn material_lines_are_emitted_once_per_change() {
    let mut text = String::from("v 0 0 0\nv 1 0 0\nv 0 1 0\nusemtl only\n");
    for _ in 0..1000 {
        text.push_str("f 1 2 3\n");
    }
    let obj = parse(&text).unwrap();
    let out = obj.to_string();
    assert_eq!(out.matches("usemtl").count(), 1);
    assert_eq!(out.matches("f 1 2 3").count(), 1000);
}

/// Writing a file with no materials must be byte-identical to before material
/// support: six leading `mtllib`/`usemtl` lines appearing in old output would be
/// a silent change for every existing caller. The one difference since 0.3 is
/// the header comment, which is now kept.
#[test]
fn material_free_output_is_byte_identical_to_the_pre_material_crate() {
    let src = "# comment\nv 0 0 0 1 0 0\nv 1 0 0\nv 0 1 0\nvt 0.5 0.5\nvn 0 0 1\nf 1/1/1 2 3\n";
    let obj = parse(src).unwrap();
    assert_eq!(
        obj.to_string(),
        "# comment\nv 0 0 0 1 0 0\nv 1 0 0\nv 0 1 0\nvt 0.5 0.5\nvn 0 0 1\nf 1/1/1 2 3\n"
    );
}

/// A material assigned before any `mtllib` line still resolves on load, because
/// the reference table is keyed by name, not by file position.
#[test]
fn usemtl_before_mtllib_still_resolves() {
    let obj = objserde::parse_file(common::mtl_fixture("out_of_order.obj")).unwrap();
    assert_eq!(obj.material_libs[0].status, LibraryStatus::Loaded);
    assert!(obj.materials[0].defined);
    assert_eq!(obj.materials[0].diffuse, Some([0.25, 0.5, 0.75]));
}

/// `usemtl` with nothing after it is degenerate, and the model treats the empty
/// string as just another name rather than a material clear (OBJ has no unset).
/// Pinning it keeps the behaviour a decision rather than an accident.
#[test]
fn an_empty_material_name_is_a_name_not_a_clear() {
    let obj = parse("v 0 0 0\nv 1 0 0\nv 0 1 0\nusemtl \nf 1 2 3\n").unwrap();
    assert_eq!(obj.materials.len(), 1);
    assert_eq!(obj.materials[0].name, "");
    assert_eq!(obj.faces[0].material, Some(0));
    assert_eq!(objserde::parse(&obj.to_string()).unwrap(), obj);
}

/// The export workflow end to end: read a model with materials, write the `.obj`
/// and the `.mtl` it points at, then reopen the pair. What comes back should be
/// the shading and texturing that went in.
#[test]
fn export_and_reimport_keeps_shading_and_textures() {
    let original = objserde::parse_file(common::mtl_fixture("basic.obj")).unwrap();
    assert_eq!(original.material_libs[0].status, LibraryStatus::Loaded);

    // A unique directory per run, so parallel test threads cannot collide.
    let dir = std::env::temp_dir().join(format!(
        "objserde-export-{}-{}",
        std::process::id(),
        original.faces.len()
    ));
    std::fs::create_dir_all(&dir).unwrap();

    let obj_text = original.to_string();
    let mtl_text = objserde::write_material_lib(
        &original.materials,
        &objserde::MtlWriteOptions {
            header: vec!["# re-exported by objserde".into()],
        },
    );
    // Mode A writes the header comment, then the reference the way the
    // original spelled it.
    assert!(
        obj_text.starts_with("# two materials, one per group of faces\nmtllib basic.mtl\n"),
        "{obj_text}"
    );
    std::fs::write(dir.join("basic.mtl"), &mtl_text).unwrap();
    std::fs::write(dir.join("model.obj"), &obj_text).unwrap();

    let reread = objserde::parse_file(dir.join("model.obj")).unwrap();
    assert_eq!(reread.faces.len(), original.faces.len());
    assert_eq!(
        reread.material_libs[0].status,
        LibraryStatus::Loaded,
        "the exported library was found next to the exported OBJ"
    );

    // Same materials, same shading numbers, same texture references.
    assert_eq!(reread.materials.len(), original.materials.len());
    for (before, after) in original.materials.iter().zip(&reread.materials) {
        assert_eq!(before.name, after.name);
        assert_eq!(before.diffuse, after.diffuse, "{}: colour", before.name);
        assert_eq!(before.shininess, after.shininess);
        assert_eq!(before.illumination, after.illumination);
        let paths = |m: &objserde::Material| -> Vec<(objserde::MapKind, String)> {
            m.maps
                .iter()
                .map(|map| (map.kind, map.path.clone()))
                .collect()
        };
        assert_eq!(paths(before), paths(after), "{}: textures", before.name);
    }

    // And each face still draws under the material it started with.
    for (a, b) in original.faces.iter().zip(&reread.faces) {
        assert_eq!(
            original.material_of(a).map(|m| &m.name),
            reread.material_of(b).map(|m| &m.name)
        );
    }

    std::fs::remove_dir_all(&dir).ok();
}

/// A texture reference survives even when its file is nowhere near: objserde
/// stores the path and never opens the image.
#[test]
fn texture_paths_are_stored_not_resolved() {
    let obj = objserde::parse_file(common::mtl_fixture("basic.obj")).unwrap();
    let paint = obj.material_of(&obj.faces[0]).unwrap();
    let map = paint.diffuse_map().unwrap();
    assert_eq!(map.path, "textures/paint.png");
    assert!(
        !common::mtl_fixture("textures/paint.png").exists(),
        "the fixture deliberately has no image on disk"
    );
}

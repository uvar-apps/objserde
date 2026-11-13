//! Performance gate for the documented budget: a 1M-vertex / 2M-face OBJ parses
//! and binds in ≤ 1.2 s, release profile, single-threaded.
//!
//! The proxy is not in the repository (160 MB of generated text). Make it with
//! `python3 assets/perf/gen_million_sphere.py`, then run:
//!
//! ```sh
//! cargo test --release --test perf -- --ignored --nocapture
//! ```
//!
//! Without the file the test skips so a fresh checkout stays green.

use std::path::Path;
use std::time::{Duration, Instant};

const BUDGET: Duration = Duration::new(1, 200_000_000); // 1.2 s
const PROXY: &str = "/tmp/million_sphere.obj";

/// Parse + bind, the whole read path, timed.
#[test]
#[ignore = "needs the generated 160 MB proxy: assets/perf/gen_million_sphere.py"]
fn million_vertex_proxy_parses_and_binds_inside_budget() {
    let (bytes, read) = timed(|| std::fs::read(PROXY).expect("generate the proxy first"));
    let proxy_mb = bytes.len() as f64 / (1024.0 * 1024.0);

    let (obj, parse) = timed(|| objserde::parse_bytes(&bytes).expect("proxy parses"));
    let (mesh, bind) = timed(|| obj.bind());

    let total = parse + bind;
    eprintln!(
        "{PROXY}: {proxy_mb:.0} MB, read {read:.3?}, parse {parse:.3?}, bind {bind:.3?}, \
         parse+bind {total:.3?} (budget {BUDGET:?})"
    );
    eprintln!(
        "  {} vertices, {} triangles, {} normals",
        obj.vertices.len(),
        mesh.triangle_count(),
        obj.normals.len()
    );

    assert!(
        obj.vertices.len() > 1_000_000,
        "expected the 1M-vertex proxy, got {} vertices",
        obj.vertices.len()
    );
    assert!(
        total <= BUDGET,
        "parse + bind took {total:?}, budget is {BUDGET:?}"
    );
}

/// The whole path including the file read, reported but not gated: disk and page
/// cache state vary too much between machines for a budget to mean anything.
#[test]
#[ignore = "needs the generated 160 MB proxy: assets/perf/gen_million_sphere.py"]
fn million_vertex_proxy_reports_end_to_end_time() {
    if !Path::new(PROXY).exists() {
        eprintln!("skipping: {PROXY} not generated");
        return;
    }
    let started = Instant::now();
    let mesh = objserde::Obj::load(PROXY).expect("proxy loads");
    eprintln!(
        "Obj::load end to end: {:?} for {} vertices / {} triangles",
        started.elapsed(),
        mesh.vertex_count(),
        mesh.triangle_count()
    );
}

fn timed<T>(f: impl FnOnce() -> T) -> (T, Duration) {
    let started = Instant::now();
    let value = f();
    (value, started.elapsed())
}

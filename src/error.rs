//! Parse and load errors. Hand-rolled `Display`/`Error` — this crate takes no
//! dependency on `thiserror`.
//!
//! Every error names the smallest thing that identifies the problem: the
//! offending line and the offending text or value. Errors never carry the file
//! buffer, so a failed 165 MB parse does not allocate a 165 MB message.

use std::error::Error;
use std::fmt;
use std::path::PathBuf;

/// What can go wrong while reading an OBJ file.
#[derive(Debug)]
pub enum ObjError {
    /// The file parsed but carried no `v` line, so there is nothing to build.
    NoVertices,
    /// A numeric field could not be read. `got` is the offending token, or
    /// `"<missing field>"` when the line ended before the expected count.
    BadFloat {
        /// 1-based source line.
        line: usize,
        /// Text that failed to parse.
        got: String,
    },
    /// A face corner used a negative relative index, or `0` in the position
    /// slot. Relative indices are not resolved; they are reported.
    NegativeIndex {
        /// 1-based source line.
        line: usize,
        /// The index as written.
        value: i64,
    },
    /// A face index that no table can satisfy.
    IndexOutOfRange {
        /// 1-based source line.
        line: usize,
        /// The index as written (1-based).
        value: i64,
        /// Size of the table the index addressed.
        count: usize,
    },
    /// The file could not be read.
    Io {
        /// Path that failed.
        path: PathBuf,
        /// Underlying OS error.
        source: std::io::Error,
    },
}

impl fmt::Display for ObjError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ObjError::NoVertices => write!(f, "no vertices: the file has no `v` lines"),
            ObjError::BadFloat { line, got } => {
                write!(f, "line {line}: cannot read `{got}` as a number")
            }
            ObjError::NegativeIndex { line, value } => write!(
                f,
                "line {line}: face index {value}; relative (negative) indices are not supported, \
                 and position indices start at 1"
            ),
            ObjError::IndexOutOfRange { line, value, count } => write!(
                f,
                "line {line}: face index {value} is out of range (the file has {count})"
            ),
            ObjError::Io { path, source } => write!(f, "{}: {source}", path.display()),
        }
    }
}

impl Error for ObjError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            ObjError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

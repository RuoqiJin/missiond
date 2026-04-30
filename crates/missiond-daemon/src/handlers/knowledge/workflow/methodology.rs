use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use super::distill::paren_balanced_ignoring_strings;
use super::{COMPILER_VERSION, GENERATED_FLOWS_DIR, WORKFLOWS_DIR};

// ───────────────────────────────────────────────────────────────────────
// helpers — methodology compiler v0 (pure, covered by unit tests)
// ───────────────────────────────────────────────────────────────────────

mod extract;
mod io;
mod source;
mod types;
mod yaml;

#[allow(unused_imports)]
pub(in crate::handlers::knowledge::workflow) use self::extract::*;
#[allow(unused_imports)]
pub(in crate::handlers::knowledge::workflow) use self::io::*;
#[allow(unused_imports)]
pub(in crate::handlers::knowledge::workflow) use self::source::*;
#[allow(unused_imports)]
pub(in crate::handlers::knowledge::workflow) use self::types::*;
#[allow(unused_imports)]
pub(in crate::handlers::knowledge::workflow) use self::yaml::*;

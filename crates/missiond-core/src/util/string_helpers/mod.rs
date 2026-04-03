mod generated;
mod custom;

pub use generated::StringHelpers;
pub use custom::{StringHelpersImpl, safe_byte_truncate, safe_char_truncate};

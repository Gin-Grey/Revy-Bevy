//! Runtime support for Bevy Scene Notation (`.bsn`) assets.
//!
//! Files intentionally accept only the data-only subset shared with [`crate::bsn!`]. Rust
//! expressions, closures, macros, and scene functions are compile-time features and are rejected
//! by the runtime parser.

mod de;
mod loader;
mod syntax;

pub use loader::{BsnAssetLoader, BsnLoadError};
pub use syntax::{
    format_bsn, parse_bsn, BsnComponent, BsnComponentBody, BsnDocument, BsnEntity, BsnParseError,
    BsnSpan, BsnStructField, BsnValue,
};

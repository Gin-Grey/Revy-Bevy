#![cfg_attr(docsrs, feature(doc_cfg))]
#![expect(
    clippy::doc_markdown,
    reason = "Clippy lints for un-backticked identifiers within the cargo features list, which we don't want."
)]
//! Internal compatibility facade for Revy's Bevy 0.19-derived foundation.
//!
//! Revy applications should depend on `revy_engine`, an alias of the legacy
//! `arisna_engine` package that owns the public
//! runtime contract and re-exports the APIs needed by game projects.
#![doc = include_str!("../docs/cargo_features.md")]
#![no_std]

pub use bevy_internal::*;

// Wasm does not support dynamic linking.
#[cfg(all(feature = "dynamic_linking", not(target_family = "wasm")))]
#[expect(
    unused_imports,
    clippy::single_component_path_imports,
    reason = "This causes Bevy to be compiled as a dylib when using dynamic linking and therefore cannot be removed or changed without affecting dynamic linking."
)]
use bevy_dylib;

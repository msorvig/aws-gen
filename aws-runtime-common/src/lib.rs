//! Shared runtime for aws-gen: error types, SigV4 signing, XML/JSON parsing traits.
//!
//! This crate is not used directly — depend on `aws-runtime-async` or `aws-runtime-sync`
//! instead, which re-export everything from this crate.

pub mod aws;
pub mod base64;
pub use serde_json;

//! Sync AWS runtime for aws-gen.
//!
//! Provides a blocking [`Client`](aws::Client) backed by ureq + rustls.
//! No tokio dependency. Re-exports all shared types from `aws-runtime-common`.
//!
//! # Example
//!
//! ```rust,ignore
//! use aws_runtime_sync::aws::Client;
//!
//! fn main() {
//!     let client = Client::from_env();
//!     let resp = s3::list_buckets(&client, s3::ListBucketsRequest::default()).unwrap();
//! }
//! ```

pub mod aws {
    pub use aws_runtime_common::aws::*;
    pub mod client;
    pub use client::Client;
}
pub use aws_runtime_common::base64;
pub use aws_runtime_common::serde_json;

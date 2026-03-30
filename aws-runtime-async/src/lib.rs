//! Async AWS runtime for aws-gen.
//!
//! Provides an async [`Client`](aws::Client) backed by hyper + rustls.
//! Re-exports all shared types from `aws-runtime-common`.
//!
//! # Example
//!
//! ```rust,ignore
//! use aws_runtime_async::aws::Client;
//!
//! #[tokio::main(flavor = "current_thread")]
//! async fn main() {
//!     let client = Client::from_env();
//!     let resp = ec2::describe_instances(&client, ec2::DescribeInstancesRequest::default()).await.unwrap();
//! }
//! ```

pub mod aws {
    pub use aws_runtime_common::aws::*;
    pub mod client;
    pub use client::Client;
}
pub use aws_runtime_common::serde_json;

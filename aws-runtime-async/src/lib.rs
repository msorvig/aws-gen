pub mod aws {
    pub use aws_runtime_common::aws::*;
    pub mod client;
    pub use client::Client;
}
pub use aws_runtime_common::serde_json;

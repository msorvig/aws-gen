use aws_codegen::{GenerateSpec, Protocol};

fn main() {
    GenerateSpec {
        name:          "s3",
        protocol:      Protocol::RestXml,
        runtime_crate: "aws_runtime_sync",
        operations:    &["ListBuckets"],
        sync:          true,
    }.generate();
}

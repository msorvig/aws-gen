use aws_codegen::{GenerateSpec, Protocol};

fn main() {
    GenerateSpec {
        name:          "s3",
        protocol:      Protocol::RestXml,
        runtime_crate: "aws_api",
        operations:    &["ListBuckets", "ListObjectsV2", "HeadObject"],
    }.generate();
}

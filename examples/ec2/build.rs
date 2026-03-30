use aws_codegen::{GenerateSpec, Protocol};

fn main() {
    GenerateSpec {
        name:          "ec2",
        protocol:      Protocol::Query,
        runtime_crate: "aws_runtime_async",
        operations:    &["DescribeInstances"],
        sync:          false,
    }.generate();
}

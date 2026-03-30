use aws_codegen::{GenerateSpec, Protocol};

fn main() {
    GenerateSpec {
        name:          "ec2",
        protocol:      Protocol::Query,
        runtime_crate: "aws_runtime_async",
        operations:    &["DescribeInstances"],
        sync:          false,
    }.generate();

    GenerateSpec {
        name:          "ssm",
        protocol:      Protocol::Json,
        runtime_crate: "aws_runtime_async",
        operations:    &["GetParameters"],
        sync:          false,
    }.generate();
}

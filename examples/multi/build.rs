use aws_codegen::{GenerateSpec, Protocol};

fn main() {
    GenerateSpec {
        name:          "ec2",
        protocol:      Protocol::Query,
        runtime_crate: "aws_api",
        operations:    &["DescribeInstances"],
    }.generate();

    GenerateSpec {
        name:          "ssm",
        protocol:      Protocol::Json,
        runtime_crate: "aws_api",
        operations:    &["GetParameters"],
    }.generate();
}

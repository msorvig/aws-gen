# aws-gen

Minimal, selective AWS API codegen for Rust.

Generates typed Rust bindings for a hand-picked subset of AWS operations from
the published botocore JSON specs. No official AWS SDK dependency. SigV4 signing
is self-contained. Choose between async (hyper) and sync (ureq) runtimes.

## Structure

```
aws-gen/
├── codegen/                  # Build-time code generator [build-dependencies]
│   ├── specs/                # Bundled botocore service specs
│   └── src/
│       ├── lib.rs            # GenerateSpec — the public API
│       ├── model.rs          # Deserializes botocore service-2.json
│       ├── resolve.rs        # Transitive shape closure + topological sort
│       └── emit.rs           # Emits structs, FromXml/JSON impls, operation fns
│
├── aws-runtime-common/       # Shared runtime: signing, parsing, error types
├── aws-runtime-async/        # Async Client (hyper + rustls + tokio)
├── aws-runtime-sync/         # Sync Client (ureq + rustls, no tokio)
│
├── examples/
│   ├── ec2/                  # Async EC2 DescribeInstances
│   ├── s3/                   # Async S3 ListBuckets + ListObjectsV2
│   ├── s3-sync/              # Sync S3 ListBuckets (no tokio)
│   └── multi/                # Two services in one crate (EC2 + SSM)
│
└── fetch_specs.sh            # Downloads botocore specs into codegen/specs/
```

## Getting started

```sh
# 1. Fetch botocore service specs (one-time, ~6 MB total)
bash fetch_specs.sh

# 2. Build and run an example
cargo run -p s3-sync-example
```

Credentials are resolved automatically via `aws configure export-credentials`
(works with SSO, profiles, etc.) or from `AWS_ACCESS_KEY_ID` /
`AWS_SECRET_ACCESS_KEY` / `AWS_SESSION_TOKEN` env vars.

## Using in your own crate

### 1. Add dependencies

For async:
```toml
[dependencies]
aws-runtime-async = { path = "../aws-runtime-async" }
tokio = { version = "1", features = ["rt", "macros"] }

[build-dependencies]
aws-codegen = { path = "../codegen" }
```

For sync (no tokio):
```toml
[dependencies]
aws-runtime-sync = { path = "../aws-runtime-sync" }

[build-dependencies]
aws-codegen = { path = "../codegen" }
```

### 2. Write build.rs

```rust
use aws_codegen::{GenerateSpec, Protocol};

fn main() {
    GenerateSpec {
        name:          "s3",
        protocol:      Protocol::RestXml,
        runtime_crate: "aws_runtime_sync",  // or "aws_runtime_async"
        operations:    &["ListBuckets", "ListObjectsV2"],
        sync:          true,                // false for async
    }.generate();
}
```

The codegen reads the bundled botocore spec, walks the transitive closure of
shapes reachable from the requested operations, topologically sorts them, and
emits `$OUT_DIR/s3.rs` with only the types actually needed.

### 3. Include generated code

```rust
use aws_runtime_sync::aws::Client;

#[allow(dead_code, non_snake_case, unused_variables, clippy::all)]
mod s3 {
    include!(concat!(env!("OUT_DIR"), "/s3.rs"));
}

fn main() {
    let client = Client::from_env();
    let resp = s3::list_buckets(&client, s3::ListBucketsRequest::default()).unwrap();
    for bucket in resp.buckets.iter().flat_map(|b| &b.item) {
        println!("{}", bucket.name.as_deref().unwrap_or("?"));
    }
}
```

## Supported protocols

| Protocol | Services | Request | Response |
|----------|----------|---------|----------|
| `Query` | EC2, IAM, STS | URL-encoded POST body | XML |
| `Json` | SSM, DynamoDB | JSON POST body | JSON |
| `RestJson` | Lambda, API Gateway | JSON + URI templates | JSON |
| `RestXml` | S3, Route53 | URI/query/headers | XML |

## Adding operations

Add the operation name to the `operations` slice in your `build.rs`. The codegen
pulls in all required shapes automatically. If the operation doesn't exist in
the spec, `generate()` panics at build time with the list of available operations.

## Adding a new service

1. Add the spec to `fetch_specs.sh`:
   ```sh
   fetch lambda "2015-03-31"
   ```

2. Add a `GenerateSpec` block to your `build.rs` with the service name and
   protocol. The spec is loaded from the bundled `codegen/specs/` directory.

## Dependency footprint

| Runtime | Crate count | Release binary |
|---------|-------------|----------------|
| async (hyper) | ~105 | ~3.2 MB |
| sync (ureq) | ~61 | ~2.6 MB |

No serde derives in generated code. No quick-xml. No url/idna/ICU crates.
The generated code uses hand-rolled `FromXml` / `FromJsonValue` / `ToJsonValue`
impls emitted by the codegen.

## Known limitations

- **Pagination**: not wired up — operations return one page. Follow `NextToken`
  in a loop in caller code.
- **rest-xml request bodies**: S3 PUT/POST operations that require an XML request
  body (e.g. `CreateBucketConfiguration`) are not yet supported. Read operations
  (ListBuckets, ListObjectsV2, HeadObject) work.
- **Streaming responses**: GetObject returns the object body as a streaming
  response, which the current client reads fully into a String. Not suitable
  for large objects.
- **Timestamps**: XML protocols use ISO 8601 strings; JSON protocols use `f64`
  epoch seconds. No `chrono`/`time` integration.

# aws-gen

Minimal, selective AWS API codegen for Rust.

Generates typed Rust bindings for a hand-picked subset of AWS operations from
the published botocore JSON specs. No official AWS SDK runtime crates. SigV4
signing is self-contained in ~100 lines using `hmac` + `sha2`.

## Structure

```
aws-gen/
├── codegen/            # Pure library crate, used as [build-dependencies]
│   └── src/
│       ├── lib.rs      # GenerateSpec — the public API for build.rs callers
│       ├── model.rs    # Deserializes botocore service-2.json
│       ├── resolve.rs  # Transitive shape closure + topological sort
│       └── emit.rs     # Code emitter: structs, QueryEncode impls, async fns
│
├── spotty/             # The actual application crate
│   ├── specs/          # Checked-in botocore JSON specs (fetch with fetch_specs.sh)
│   ├── build.rs        # Drives codegen — lists operations per service
│   └── src/
│       ├── aws/
│       │   ├── client.rs       # Client struct + SigV4 signing + HTTP dispatch
│       │   ├── error.rs        # AwsError + XML/JSON error parsers
│       │   └── query_proto.rs  # QueryEncode trait + ToAwsStr + encode_form
│       ├── lib.rs              # include!()s generated modules
│       └── main.rs             # Usage examples
│
└── fetch_specs.sh      # One-time script to download botocore specs
```

## Getting started

```sh
# 1. Fetch the three botocore service specs (≈ 5 MB total)
bash fetch_specs.sh

# 2. Build — codegen runs automatically as part of cargo build
cargo build -p spotty

# 3. Run
AWS_DEFAULT_REGION=eu-west-1 \
AWS_ACCESS_KEY_ID=AKIA... \
AWS_SECRET_ACCESS_KEY=... \
cargo run -p spotty -- spot-prices
```

Available commands: `spot-prices`, `describe`, `ami-ids`, `iam-user [name]`, `instance-types`.

## How build.rs drives codegen

`spotty/build.rs` calls `GenerateSpec::generate()` for each service:

```rust
GenerateSpec {
    name:      "ec2",
    spec_file: "specs/ec2.json",
    protocol:  Protocol::Query,
    operations: &["RunInstances", "DescribeInstances", ...],
}.generate(&out_dir);
```

`generate()` does:
1. Parse `specs/ec2.json` (the full EC2 service model — ~15k shapes)
2. Walk the transitive closure of shapes reachable from the requested operations
3. Topologically sort them (dependencies before dependents)
4. Emit `$OUT_DIR/ec2.rs` containing only the shapes actually used

`spotty/src/lib.rs` then includes the generated file:

```rust
pub mod ec2 {
    include!(concat!(env!("OUT_DIR"), "/ec2.rs"));
}
```

## Wire protocols

### Query protocol (EC2, IAM)

Request: HTTP POST to `https://ec2.{region}.amazonaws.com/` with
`Content-Type: application/x-www-form-urlencoded`. Body contains
`Action=RunInstances&Version=2016-11-15&...`.

Nested structs flatten to dotted paths:
```
InstanceMarketOptions.MarketType=spot
InstanceMarketOptions.SpotOptions.MaxPrice=0.05
```

Lists use numeric suffixes:
```
SecurityGroupId.1=sg-abc123
SecurityGroupId.2=sg-def456
```

Response: XML. Generated via `quick-xml`'s serde support.

**IAM result wrapper**: IAM response bodies wrap the result in an extra XML
element (e.g., `<GetUserResult>...</GetUserResult>` inside
`<GetUserResponse>`). The codegen detects this via the `resultWrapper` field
in the spec and emits a private envelope struct to unwrap it. EC2 doesn't use
an intermediate wrapper — the result is directly inside the response root.

### JSON 1.1 protocol (SSM)

Request: HTTP POST with `Content-Type: application/x-amz-json-1.1` and
`X-Amz-Target: AmazonSSM.GetParameters`. Body is JSON.

Response: JSON, deserialized with `serde_json`.

## Adding operations

Edit `spotty/build.rs` — add the operation name to the relevant `operations` slice.
The codegen will automatically pull in all required shapes from the spec.

```rust
GenerateSpec {
    name:      "ec2",
    spec_file: "specs/ec2.json",
    protocol:  Protocol::Query,
    operations: &[
        "RunInstances",
        "TerminateInstances",
        "RequestSpotInstances",  // ← new
        ...
    ],
}.generate(&out_dir);
```

If the operation isn't found in the spec, `generate()` panics at build time
with the list of available operations.

## Adding a new service

1. Add the spec to `fetch_specs.sh`:
   ```sh
   fetch lambda "2015-03-31"
   ```

2. Add a `GenerateSpec` block to `spotty/build.rs`:
   ```rust
   GenerateSpec {
       name:      "lambda",
       spec_file: "specs/lambda.json",
       protocol:  Protocol::Json,    // rest-json, treated as json
       operations: &["InvokeFunction", "ListFunctions"],
   }.generate(&out);
   ```

3. Add the `include!` in `spotty/src/lib.rs`:
   ```rust
   pub mod lambda {
       include!(concat!(env!("OUT_DIR"), "/lambda.rs"));
   }
   ```

## Known rough edges

**`rest-json` protocol** (Lambda, S3, etc.): operations have URL path templates
like `/2015-03-31/functions/{FunctionName}/invocations`. The current `json`
protocol handler always POSTs to `/` and ignores URI templates and location
overrides. For `rest-json` services you'd need to add URI template expansion
to `emit_json_op` using the `http.requestUri` field and `location: "uri"`
member annotations. SSM uses the plain `json` protocol (always `/`), so it
works correctly.

**XML list deserialization edge cases**: EC2 uses `<item>` as the per-element
tag name by convention, but some operations use custom element names from
`member.locationName`. The list wrapper structs use `locationName` for the
serde `rename` attribute. Verify output for any new operations that have
unusual list shapes.

**Map types in query protocol**: query-protocol maps (e.g., tag filter maps,
attribute maps) are left as `HashMap<K, V>`. Serialization to query params
for maps isn't implemented — add `QueryEncode` impls per map type if needed.

**Pagination**: the botocore specs include `paginators-1.json` alongside
`service-2.json`. Pagination isn't wired up — operations return at most one
page. Implementing it requires following `NextToken` / `nextToken` in a loop;
straightforward to add as a higher-level wrapper over the generated functions.

**Timestamp types**: serialized as `String` (ISO 8601 / Unix epoch as AWS
returns them). Parse with `chrono` or `time` in the caller if needed.

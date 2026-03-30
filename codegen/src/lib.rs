pub mod model;
pub mod resolve;
pub mod emit;

use std::path::{Path, PathBuf};

/// Returns the path to a bundled botocore spec file (e.g. `spec("ec2")`).
/// The specs are shipped inside the codegen crate.
pub fn spec(service: &str) -> PathBuf {
    let codegen_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    codegen_dir.join("specs").join(format!("{service}.json"))
}

/// Which wire protocol the service uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// AWS query protocol (EC2, IAM, STS, CloudFormation).
    /// POST body is URL-encoded key=value params; response is XML.
    Query,
    /// AWS JSON 1.1 protocol (DynamoDB, SSM, etc.).
    /// POST body and response are JSON; X-Amz-Target header identifies the operation.
    Json,
    /// AWS REST-JSON protocol (Lambda, API Gateway, etc.).
    /// Method + URI path (with {Param} templates) + optional query/header params + JSON body.
    RestJson,
    /// AWS REST-XML protocol (S3, Route53, CloudFront, etc.).
    /// Method + URI path (with {Param} templates) + optional query/header params + XML response.
    RestXml,
}

/// Configuration for generating one service module.
pub struct GenerateSpec<'a> {
    /// Short lowercase name used as the output file name: "ec2", "ssm", "iam".
    pub name:       &'a str,
    /// Wire protocol.
    pub protocol:   Protocol,
    /// The subset of operations to generate (all transitive shapes are included).
    pub operations: &'a [&'a str],
    /// Crate path for the runtime in generated code.
    /// Use `"aws_runtime_async"` or `"aws_runtime_sync"`.
    pub runtime_crate: &'a str,
    /// If true, emit sync functions instead of async.
    pub sync: bool,
}

impl<'a> GenerateSpec<'a> {
    /// Generate Rust source and write it to `$OUT_DIR/{self.name}.rs`.
    /// `spec_file` is resolved relative to `$CARGO_MANIFEST_DIR`.
    /// Call this from `build.rs`.
    pub fn generate(&self) {
        let out_dir = std::env::var("OUT_DIR")
            .expect("OUT_DIR not set — generate() must be called from build.rs");

        // ── Load spec ──────────────────────────────────────────────────────────
        let spec_path = spec(self.name);
        let json = std::fs::read_to_string(&spec_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", spec_path.display()));

        let model: model::ServiceModel = serde_json::from_str(&json)
            .unwrap_or_else(|e| panic!("failed to parse {}: {e}", spec_path.display()));

        // Validate that every requested operation actually exists in the spec
        for &op in self.operations {
            if !model.operations.contains_key(op) {
                panic!(
                    "operation '{}' not found in {}. Available: {}",
                    op,
                    spec_path.display(),
                    model.operations.keys().cloned().collect::<Vec<_>>().join(", "),
                );
            }
        }

        // ── Resolve shapes ─────────────────────────────────────────────────────
        let reachable = resolve::compute(&model, self.operations);
        let topo      = resolve::topo_sort(&model, &reachable.all());

        // ── Emit code ─────────────────────────────────────────────────────────
        let ctx = emit::EmitCtx {
            model:        &model,
            reachable:    &reachable,
            topo_order:   &topo,
            operations:   self.operations,
            service_name: self.name,
            runtime_crate: self.runtime_crate,
            sync: self.sync,
        };
        let code = emit::emit(&ctx);

        // ── Write output ───────────────────────────────────────────────────────
        let out_path: PathBuf = [out_dir.as_str(), &format!("{}.rs", self.name)].iter().collect();
        std::fs::write(&out_path, code)
            .unwrap_or_else(|e| panic!("failed to write {}: {e}", out_path.display()));

        println!("cargo:rerun-if-changed={}", spec_path.display());
    }

    /// Generate Rust source from an explicit spec file path and return it as a String.
    /// Used by the codegen-cli dry-run tool.
    pub fn generate_from_spec(&self, spec_file: &str) -> String {
        let json = std::fs::read_to_string(spec_file)
            .unwrap_or_else(|e| panic!("failed to read {spec_file}: {e}"));
        let model: model::ServiceModel = serde_json::from_str(&json)
            .unwrap_or_else(|e| panic!("failed to parse {spec_file}: {e}"));
        for &op in self.operations {
            if !model.operations.contains_key(op) {
                panic!("operation '{}' not found in {}", op, spec_file);
            }
        }
        let reachable = resolve::compute(&model, self.operations);
        let topo      = resolve::topo_sort(&model, &reachable.all());
        let ctx = emit::EmitCtx {
            model:        &model,
            reachable:    &reachable,
            topo_order:   &topo,
            operations:   self.operations,
            service_name: self.name,
            runtime_crate: self.runtime_crate,
            sync: self.sync,
        };
        emit::emit(&ctx)
    }
}

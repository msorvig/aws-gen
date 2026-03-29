/// Codegen dry-run CLI.
///
/// Usage:
///   # Show all operation names in a spec:
///   cargo run -p aws-codegen --bin codegen-cli -- ops specs/ec2.json
///
///   # Show shapes reachable from specific operations:
///   cargo run -p aws-codegen --bin codegen-cli -- shapes specs/ec2.json RunInstances DescribeInstances
///
///   # Dry-run: print the generated Rust source for given operations:
///   cargo run -p aws-codegen --bin codegen-cli -- gen specs/ec2.json ec2 query RunInstances TerminateInstances
///
///   # Validate: parse the spec and report any unknown shape types:
///   cargo run -p aws-codegen --bin codegen-cli -- validate specs/ec2.json
use aws_codegen::{model::ServiceModel, resolve, GenerateSpec, Protocol};
use std::collections::HashSet;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: codegen-cli <command> <spec.json> [args...]");
        eprintln!("commands: ops, shapes, gen, validate");
        std::process::exit(1);
    }

    let cmd      = &args[0];
    let spec_path = args.get(1).map(String::as_str).unwrap_or("specs/ec2.json");

    let json = std::fs::read_to_string(spec_path)
        .unwrap_or_else(|e| { eprintln!("cannot read {spec_path}: {e}"); std::process::exit(1); });

    let model: ServiceModel = serde_json::from_str(&json)
        .unwrap_or_else(|e| { eprintln!("cannot parse {spec_path}: {e}"); std::process::exit(1); });

    match cmd.as_str() {
        "ops" => cmd_ops(&model),
        "shapes" => {
            let ops: Vec<&str> = args[2..].iter().map(String::as_str).collect();
            cmd_shapes(&model, &ops);
        }
        "gen" => {
            // gen <spec> <name> <protocol> <op1> [op2 ...]
            let name     = args.get(2).map(String::as_str).unwrap_or("service");
            let proto_s  = args.get(3).map(String::as_str).unwrap_or("query");
            let ops: Vec<&str> = args[4..].iter().map(String::as_str).collect();
            let protocol = match proto_s {
                "json"      => Protocol::Json,
                "rest-json" => Protocol::RestJson,
                "rest-xml"  => Protocol::RestXml,
                _           => Protocol::Query,
            };
            cmd_gen(spec_path, name, protocol, &ops);
        }
        "validate" => cmd_validate(&model),
        other => {
            eprintln!("unknown command: {other}");
            std::process::exit(1);
        }
    }
}

fn cmd_ops(model: &ServiceModel) {
    let mut ops: Vec<&str> = model.operations.keys().map(String::as_str).collect();
    ops.sort_unstable();
    println!("# {} operations in {} ({})", ops.len(), model.metadata.endpoint_prefix, model.metadata.protocol);
    for op in ops {
        let o = &model.operations[op];
        let input  = o.input.as_ref().map(|r| r.shape.as_str()).unwrap_or("—");
        let output = o.output.as_ref().map(|r| r.shape.as_str()).unwrap_or("—");
        println!("  {op:<50} {input} → {output}");
    }
}

fn cmd_shapes(model: &ServiceModel, ops: &[&str]) {
    // validate ops exist
    for &op in ops {
        if !model.operations.contains_key(op) {
            eprintln!("operation not found: {op}");
            std::process::exit(1);
        }
    }

    let reachable = resolve::compute(model, ops);
    let all = reachable.all();
    let topo = resolve::topo_sort(model, &all);

    println!("# {} shapes reachable from: {}", topo.len(), ops.join(", "));
    println!("{:<50} {:<12} {:<6} {:<6}", "Shape", "Type", "Input", "Output");
    println!("{}", "-".repeat(76));

    for name in &topo {
        let shape = match model.shapes.get(name) {
            Some(s) => s,
            None    => continue,
        };
        // Skip primitives unless enum
        if shape.is_primitive() { continue; }

        let in_i = if reachable.input.contains(name)  { "✓" } else { " " };
        let in_o = if reachable.output.contains(name) { "✓" } else { " " };
        println!("{:<50} {:<12} {:<6} {:<6}", name, shape.shape_type, in_i, in_o);

        // For structures, show member count and required members
        if shape.shape_type == "structure" && !shape.members.is_empty() {
            let req: Vec<_> = shape.required.iter().map(String::as_str).collect();
            for (mname, mref) in &shape.members {
                let is_req = req.contains(&mname.as_str());
                let loc    = mref.location.as_deref().unwrap_or("");
                let marker = if is_req { "*" } else { " " };
                let loc_s  = if loc.is_empty() { String::new() } else { format!(" [{loc}]") };
                println!("    {marker} {mname:<44} {}{loc_s}", mref.shape);
            }
        }
    }
}

fn cmd_gen(spec_path: &str, name: &str, protocol: Protocol, ops: &[&str]) {
    if ops.is_empty() {
        eprintln!("provide at least one operation name");
        std::process::exit(1);
    }

    let spec = GenerateSpec {
        name,
        protocol,
        operations: ops,
        runtime_crate: "aws_api",
    };

    let code = spec.generate_from_spec(spec_path);
    print!("{code}");
    eprintln!("\n# {} bytes generated for {} operations", code.len(), ops.len());
}

fn cmd_validate(model: &ServiceModel) {
    let mut issues = 0usize;
    let known_types: HashSet<&str> = [
        "structure","list","map","string","integer","long",
        "float","double","boolean","timestamp","blob",
    ].iter().copied().collect();

    for (name, shape) in &model.shapes {
        if !known_types.contains(shape.shape_type.as_str()) {
            eprintln!("UNKNOWN type: shape={name} type={}", shape.shape_type);
            issues += 1;
        }
        // Check all member references resolve
        for (mname, mref) in &shape.members {
            if !model.shapes.contains_key(&mref.shape) {
                eprintln!("MISSING: {name}.{mname} → {} (not in shapes)", mref.shape);
                issues += 1;
            }
        }
        // List
        if let Some(m) = &shape.member {
            if !model.shapes.contains_key(&m.shape) {
                eprintln!("MISSING list item: {name} → {}", m.shape);
                issues += 1;
            }
        }
    }
    // Check all operation inputs/outputs resolve
    for (op_name, op) in &model.operations {
        for side in [&op.input, &op.output].into_iter().flatten() {
            if !model.shapes.contains_key(&side.shape) {
                eprintln!("MISSING op shape: {op_name} → {}", side.shape);
                issues += 1;
            }
        }
    }

    let n_shapes = model.shapes.len();
    let n_ops    = model.operations.len();
    println!("Validated {n_ops} operations, {n_shapes} shapes — {issues} issues");
    if issues > 0 { std::process::exit(1); }
}

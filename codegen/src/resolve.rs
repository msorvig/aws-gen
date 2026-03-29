use std::collections::HashSet;
use crate::model::ServiceModel;

/// All shapes reachable from the given operation names, split into
/// input-context and output-context sets (a shape can be in both).
pub struct ReachableShapes {
    pub input:  HashSet<String>,
    pub output: HashSet<String>,
}

impl ReachableShapes {
    pub fn all(&self) -> HashSet<String> {
        self.input.union(&self.output).cloned().collect()
    }

    pub fn needs_emit(&self, name: &str) -> bool {
        self.all().contains(name)
    }
}

pub fn compute(model: &ServiceModel, operations: &[&str]) -> ReachableShapes {
    let mut input  = HashSet::new();
    let mut output = HashSet::new();

    for &op_name in operations {
        let Some(op) = model.operations.get(op_name) else { continue };
        if let Some(r) = &op.input  { walk(&r.shape, model, &mut input);  }
        if let Some(r) = &op.output { walk(&r.shape, model, &mut output); }
        // errors are always output-context (they're deserialized from responses)
        for err in &op.errors { walk(&err.shape, model, &mut output); }
    }

    ReachableShapes { input, output }
}

fn walk(name: &str, model: &ServiceModel, visited: &mut HashSet<String>) {
    if !visited.insert(name.to_string()) { return; }
    let Some(shape) = model.shapes.get(name) else { return };
    match shape.shape_type.as_str() {
        "structure" => {
            for member_ref in shape.members.values() {
                walk(&member_ref.shape, model, visited);
            }
        }
        "list" => {
            if let Some(m) = &shape.member { walk(&m.shape, model, visited); }
        }
        "map" => {
            if let Some(k) = &shape.key   { walk(&k.shape, model, visited); }
            if let Some(v) = &shape.value { walk(&v.shape, model, visited); }
        }
        _ => {}
    }
}

/// Topological sort of shapes so that dependencies are emitted before dependents.
/// Returns shape names in bottom-up order (leaves first).
pub fn topo_sort(model: &ServiceModel, names: &HashSet<String>) -> Vec<String> {
    let mut visited = HashSet::new();
    let mut order   = Vec::new();

    for name in names {
        topo_visit(name, model, names, &mut visited, &mut order);
    }

    order
}

fn topo_visit(
    name:    &str,
    model:   &ServiceModel,
    allowed: &HashSet<String>,
    visited: &mut HashSet<String>,
    order:   &mut Vec<String>,
) {
    if !visited.insert(name.to_string()) { return; }
    let Some(shape) = model.shapes.get(name) else { return };

    // Visit dependencies first
    match shape.shape_type.as_str() {
        "structure" => {
            for member_ref in shape.members.values() {
                if allowed.contains(&member_ref.shape) {
                    topo_visit(&member_ref.shape, model, allowed, visited, order);
                }
            }
        }
        "list" => {
            if let Some(m) = &shape.member {
                if allowed.contains(&m.shape) {
                    topo_visit(&m.shape, model, allowed, visited, order);
                }
            }
        }
        "map" => {
            for opt in [&shape.key, &shape.value].into_iter().flatten() {
                if allowed.contains(&opt.shape) {
                    topo_visit(&opt.shape, model, allowed, visited, order);
                }
            }
        }
        _ => {}
    }

    order.push(name.to_string());
}

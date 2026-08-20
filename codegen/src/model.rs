use indexmap::IndexMap;
use serde::Deserialize;

/// Root of a botocore service-2.json file.
#[derive(Debug, Deserialize)]
pub struct ServiceModel {
    pub metadata:   Metadata,
    pub operations: IndexMap<String, Operation>,
    pub shapes:     IndexMap<String, Shape>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    pub protocol:          String,           // "query" | "json" | "rest-json" | "rest-xml"
    pub endpoint_prefix:   String,           // "ec2", "ssm", "iam"
    pub api_version:       String,           // "2016-11-15"
    pub service_id:        Option<String>,
    pub signature_version: Option<String>,   // "v4"
    /// For "json" protocol: the value prefix used in X-Amz-Target header.
    /// e.g. "AmazonSSM" → header becomes "AmazonSSM.GetParameters"
    pub target_prefix:     Option<String>,
    pub json_version:      Option<String>,   // "1.1"
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    pub name:           String,
    pub http:           Option<Http>,
    pub input:          Option<ShapeRef>,
    pub output:         Option<ShapeRef>,
    #[serde(default)]
    pub errors:         Vec<ShapeRef>,
    /// Query protocol wraps the response body inside this element name.
    /// If absent the root element is the result directly.
    pub result_wrapper: Option<String>,
    /// Legacy flag: the operation requires a Content-MD5 header over the
    /// request body. Modern specs use `httpChecksum` instead.
    #[serde(rename = "httpChecksumRequired", default)]
    pub http_checksum_required: bool,
    /// Modern checksum requirements (e.g. S3 DeleteObjects).
    pub http_checksum: Option<HttpChecksum>,
    pub documentation:  Option<String>,
}

impl Operation {
    /// True when the request body must carry a Content-MD5 header.
    pub fn checksum_required(&self) -> bool {
        self.http_checksum_required
            || self
                .http_checksum
                .as_ref()
                .is_some_and(|c| c.request_checksum_required)
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HttpChecksum {
    #[serde(default)]
    pub request_checksum_required: bool,
    pub request_algorithm_member: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Http {
    pub method:       String,   // "POST" / "GET"
    pub request_uri:  String,   // "/" or "/2015-03-31/functions/{FunctionName}"
}

/// A reference to a named shape, used in structure members, list items, etc.
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ShapeRef {
    pub shape:          String,
    /// Wire name override.  For query protocol this is the query-param name.
    /// For JSON/REST this is the JSON key or URI segment name.
    pub location_name:  Option<String>,
    /// "uri" | "querystring" | "header" | "headers" — REST protocols only.
    pub location:       Option<String>,
    pub documentation:  Option<String>,
    pub streaming:      Option<bool>,
}

impl ShapeRef {
    /// The wire name: uses locationName if present, falls back to the Rust member name
    /// passed in (which is the botocore key from the parent members map).
    pub fn wire_name<'a>(&'a self, member_key: &'a str) -> &'a str {
        self.location_name.as_deref().unwrap_or(member_key)
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct Shape {
    #[serde(rename = "type")]
    pub shape_type: String,  // "structure" | "list" | "map" | "string" | "integer" | "long"
                             // | "float" | "double" | "boolean" | "timestamp" | "blob"

    // ── structure fields ──────────────────────────────────────────────────────
    #[serde(default)]
    pub members:  IndexMap<String, ShapeRef>,
    /// Members listed here are non-optional in the generated struct.
    #[serde(default)]
    pub required: Vec<String>,
    /// REST protocols: the member that IS the HTTP body (a blob for raw
    /// payloads like S3 Get/PutObject, or a structure serialized as the
    /// body like S3 DeleteObjects' Delete).
    pub payload:  Option<String>,

    // ── list fields ───────────────────────────────────────────────────────────
    /// The item shape reference (and its locationName = XML element name for items).
    pub member:   Option<ShapeRef>,
    /// Flattened lists serialize items directly (no wrapper element).
    #[serde(default)]
    pub flattened: bool,

    // ── map fields ────────────────────────────────────────────────────────────
    pub key:      Option<ShapeRef>,
    pub value:    Option<ShapeRef>,

    // ── string enum ───────────────────────────────────────────────────────────
    /// If present, this string shape is an enum.
    #[serde(rename = "enum")]
    pub enum_values: Option<Vec<String>>,

    pub documentation: Option<String>,
    pub sensitive:     Option<bool>,
    pub min:           Option<serde_json::Value>,
    pub max:           Option<serde_json::Value>,
}

impl Shape {
    /// True for primitive scalar shapes that we inline rather than emit as types.
    pub fn is_primitive(&self) -> bool {
        matches!(
            self.shape_type.as_str(),
            "string" | "integer" | "long" | "float" | "double" | "boolean" | "timestamp" | "blob"
        ) && self.enum_values.is_none()
    }
}

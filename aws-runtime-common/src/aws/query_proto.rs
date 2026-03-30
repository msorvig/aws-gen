/// Trait for encoding a value into AWS query-protocol key-value pairs.
/// `prefix` is the dotted path accumulated so far (e.g. "InstanceMarketOptions.SpotOptions").
pub trait QueryEncode {
    fn encode(&self, prefix: &str, out: &mut Vec<(String, String)>);
}

/// Trait for converting a scalar value to its AWS string representation.
pub trait ToAwsStr {
    fn to_aws_str(&self) -> String;
}

impl QueryEncode for String {
    fn encode(&self, prefix: &str, out: &mut Vec<(String, String)>) {
        out.push((prefix.to_string(), self.clone()));
    }
}

impl QueryEncode for bool {
    fn encode(&self, prefix: &str, out: &mut Vec<(String, String)>) {
        out.push((prefix.to_string(), self.to_aws_str()));
    }
}

impl QueryEncode for i32 {
    fn encode(&self, prefix: &str, out: &mut Vec<(String, String)>) {
        out.push((prefix.to_string(), self.to_aws_str()));
    }
}

impl QueryEncode for i64 {
    fn encode(&self, prefix: &str, out: &mut Vec<(String, String)>) {
        out.push((prefix.to_string(), self.to_aws_str()));
    }
}

impl QueryEncode for f64 {
    fn encode(&self, prefix: &str, out: &mut Vec<(String, String)>) {
        out.push((prefix.to_string(), self.to_aws_str()));
    }
}

impl ToAwsStr for String {
    fn to_aws_str(&self) -> String { self.clone() }
}

impl ToAwsStr for &str {
    fn to_aws_str(&self) -> String { self.to_string() }
}

impl ToAwsStr for i32 {
    fn to_aws_str(&self) -> String { self.to_string() }
}

impl ToAwsStr for i64 {
    fn to_aws_str(&self) -> String { self.to_string() }
}

impl ToAwsStr for f32 {
    fn to_aws_str(&self) -> String { self.to_string() }
}

impl ToAwsStr for f64 {
    fn to_aws_str(&self) -> String { self.to_string() }
}

impl ToAwsStr for bool {
    fn to_aws_str(&self) -> String { if *self { "true" } else { "false" }.to_string() }
}

/// Encode a list of key-value pairs into a URL-encoded form body.
pub fn encode_form(params: &[(String, String)]) -> String {
    params.iter()
        .map(|(k, v)| format!("{}={}", super::sign::percent_encode(k), super::sign::percent_encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

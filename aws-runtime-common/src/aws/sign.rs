use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use super::error::AwsError;

type HmacSha256 = Hmac<Sha256>;

/// AWS credentials resolved from environment or CLI.
pub struct Credentials {
    pub region: String,
    pub access_key: String,
    pub secret_key: String,
    pub session_token: Option<String>,
}

impl Credentials {
    /// Resolve credentials via `aws configure export-credentials`.
    /// Falls back to env vars if the CLI isn't available.
    pub fn from_env() -> Self {
        if let Some(creds) = Self::from_aws_cli() {
            return creds;
        }
        Self {
            region: Self::resolve_region(),
            access_key: std::env::var("AWS_ACCESS_KEY_ID")
                .expect("AWS_ACCESS_KEY_ID must be set (or install aws cli for SSO)"),
            secret_key: std::env::var("AWS_SECRET_ACCESS_KEY")
                .expect("AWS_SECRET_ACCESS_KEY must be set"),
            session_token: std::env::var("AWS_SESSION_TOKEN").ok(),
        }
    }

    fn from_aws_cli() -> Option<Self> {
        let output = std::process::Command::new("aws")
            .args(["configure", "export-credentials"])
            .output()
            .ok()?;
        if !output.status.success() { return None; }

        let v: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
        Some(Self {
            region: Self::resolve_region(),
            access_key: v["AccessKeyId"].as_str()?.to_string(),
            secret_key: v["SecretAccessKey"].as_str()?.to_string(),
            session_token: v["SessionToken"].as_str().map(String::from),
        })
    }

    pub fn resolve_region() -> String {
        std::env::var("AWS_DEFAULT_REGION")
            .or_else(|_| std::env::var("AWS_REGION"))
            .or_else(|e| {
                std::process::Command::new("aws")
                    .args(["configure", "get", "region"])
                    .output().ok()
                    .and_then(|out| String::from_utf8(out.stdout).ok())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .ok_or(e)
            })
            .unwrap_or_else(|_| "us-east-1".into())
    }
}

// ── SigV4 ──────────────────────────────────────────────────────────────

pub fn sign_v4(
    creds: &Credentials,
    service: &str,
    date_stamp: &str,
    amz_date: &str,
    signed_headers: &str,
    canonical_request: &str,
) -> String {
    let scope = format!("{date_stamp}/{}/{service}/aws4_request", creds.region);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        hex_sha256(canonical_request.as_bytes()),
    );

    let k_date    = hmac_sha256(format!("AWS4{}", creds.secret_key).as_bytes(), date_stamp.as_bytes());
    let k_region  = hmac_sha256(&k_date, creds.region.as_bytes());
    let k_service = hmac_sha256(&k_region, service.as_bytes());
    let k_signing = hmac_sha256(&k_service, b"aws4_request");
    let signature = hex_encode(hmac_sha256(&k_signing, string_to_sign.as_bytes()));

    format!(
        "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
        creds.access_key,
    )
}

// ── Helpers ────────────────────────────────────────────────────────────

pub fn hex_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex_encode(hasher.finalize())
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

pub fn now_stamps() -> (String, String) {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let (y, m, d, hh, mm, ss) = unix_to_ymdhms(secs);
    let date_stamp = format!("{y:04}{m:02}{d:02}");
    let amz_date   = format!("{y:04}{m:02}{d:02}T{hh:02}{mm:02}{ss:02}Z");
    (date_stamp, amz_date)
}

fn unix_to_ymdhms(secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let s = secs % 86400;
    let hh = s / 3600;
    let mm = (s % 3600) / 60;
    let ss = s % 60;

    let mut days = secs / 86400;
    let mut y = 1970u64;
    loop {
        let dy = if is_leap(y) { 366 } else { 365 };
        if days < dy { break; }
        days -= dy;
        y += 1;
    }
    let leap = is_leap(y);
    let mdays: [u64; 12] = [31, if leap {29} else {28}, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut m = 0u64;
    for md in mdays {
        if days < md { break; }
        days -= md;
        m += 1;
    }
    (y, m + 1, days + 1, hh, mm, ss)
}

fn is_leap(y: u64) -> bool {
    y.is_multiple_of(4) && (!y.is_multiple_of(100) || y.is_multiple_of(400))
}

/// AWS-compatible percent encoding (RFC 3986 unreserved chars pass through).
pub fn percent_encode(s: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push(HEX[(b >> 4) as usize] as char);
                out.push(HEX[(b & 0xf) as usize] as char);
            }
        }
    }
    out
}

pub fn hex_encode(data: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let data = data.as_ref();
    let mut s = String::with_capacity(data.len() * 2);
    for &b in data {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0xf) as usize] as char);
    }
    s
}

// ── Error parsers ──────────────────────────────────────────────────────

pub fn parse_xml_error(body: &str, status: u16) -> AwsError {
    let code = extract_xml_tag(body, "Code").unwrap_or_else(|| format!("HTTP {status}"));
    let message = extract_xml_tag(body, "Message").unwrap_or_else(|| body[..body.len().min(200)].to_string());
    AwsError::AwsService { code, message }
}

fn extract_xml_tag(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    Some(xml[start..end].to_string())
}

pub fn parse_json_error(body: &str, status: u16) -> AwsError {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        let code = v.get("__type")
            .or_else(|| v.get("code"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .rsplit('#').next()
            .unwrap_or("")
            .to_string();
        let message = v.get("message")
            .or_else(|| v.get("Message"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        AwsError::AwsService { code, message }
    } else {
        AwsError::AwsService {
            code: format!("HTTP {status}"),
            message: body[..body.len().min(200)].to_string(),
        }
    }
}

// ── Query string helpers ───────────────────────────────────────────────

pub fn build_query_string(query: &[(String, String)]) -> String {
    if query.is_empty() { return String::new(); }
    let s = query.iter()
        .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    format!("?{s}")
}

pub fn canonical_query_string(query: &[(String, String)]) -> String {
    if query.is_empty() { return String::new(); }
    let mut sorted = query.to_vec();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    sorted.iter()
        .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

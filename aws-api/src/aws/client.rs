use bytes::Bytes;
use hmac::{Hmac, Mac};
use http_body_util::{BodyExt, Full};
use hyper::Request;
use hyper_rustls::HttpsConnectorBuilder;
use hyper_util::client::legacy::Client as HyperClient;
use hyper_util::rt::TokioExecutor;
use sha2::{Digest, Sha256};

use super::error::AwsError;
use super::query_proto;

type HmacSha256 = Hmac<Sha256>;
type HttpClient = HyperClient<hyper_rustls::HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>, Full<Bytes>>;

pub struct Client {
    pub region: String,
    pub access_key: String,
    pub secret_key: String,
    pub session_token: Option<String>,
    http: HttpClient,
}

fn make_http_client() -> HttpClient {
    let https = HttpsConnectorBuilder::new()
        .with_webpki_roots()
        .https_only()
        .enable_http1()
        .build();
    HyperClient::builder(TokioExecutor::new()).build(https)
}

impl Client {
    /// Resolve credentials via `aws configure export-credentials`.
    /// Falls back to env vars if the CLI isn't available.
    pub fn from_env() -> Self {
        if let Some(client) = Self::from_aws_cli() {
            return client;
        }
        Self {
            region: Self::resolve_region(),
            access_key: std::env::var("AWS_ACCESS_KEY_ID")
                .expect("AWS_ACCESS_KEY_ID must be set (or install aws cli for SSO)"),
            secret_key: std::env::var("AWS_SECRET_ACCESS_KEY")
                .expect("AWS_SECRET_ACCESS_KEY must be set"),
            session_token: std::env::var("AWS_SESSION_TOKEN").ok(),
            http: make_http_client(),
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
            http: make_http_client(),
        })
    }

    fn resolve_region() -> String {
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

    // ── Low-level HTTP dispatch ─────────────────────────────────────────

    async fn send(&self, req: Request<Full<Bytes>>) -> Result<(u16, String), AwsError> {
        let resp = self.http.request(req).await
            .map_err(|e| AwsError::Http(e.to_string()))?;
        let status = resp.status().as_u16();
        let body = resp.into_body().collect().await
            .map_err(|e| AwsError::Http(e.to_string()))?
            .to_bytes();
        let text = String::from_utf8_lossy(&body).into_owned();
        Ok((status, text))
    }

    fn build_request(
        &self,
        method: &str,
        url: &str,
        headers: &[(&str, &str)],
        body: Vec<u8>,
    ) -> Request<Full<Bytes>> {
        let mut builder = Request::builder()
            .method(method)
            .uri(url);
        for &(k, v) in headers {
            builder = builder.header(k, v);
        }
        builder.body(Full::new(Bytes::from(body))).unwrap()
    }

    // ── Query protocol (EC2, IAM) ──────────────────────────────────────

    pub async fn query_request<T: super::xml::FromXml>(
        &self,
        service: &str,
        version: &str,
        action: &str,
        mut params: Vec<(String, String)>,
    ) -> Result<T, AwsError> {
        params.push(("Action".into(), action.into()));
        params.push(("Version".into(), version.into()));
        let body = query_proto::encode_form(&params);

        let host = format!("{service}.{}.amazonaws.com", self.region);
        let url = format!("https://{host}/");

        let (date_stamp, amz_date) = now_stamps();
        let content_type = "application/x-www-form-urlencoded; charset=utf-8";
        let payload_hash = hex_sha256(body.as_bytes());

        let (headers_to_sign, canonical) = if let Some(token) = &self.session_token {
            ("content-type;host;x-amz-date;x-amz-security-token".to_string(),
             format!("POST\n/\n\ncontent-type:{content_type}\nhost:{host}\nx-amz-date:{amz_date}\nx-amz-security-token:{token}\n\ncontent-type;host;x-amz-date;x-amz-security-token\n{payload_hash}"))
        } else {
            ("content-type;host;x-amz-date".to_string(),
             format!("POST\n/\n\ncontent-type:{content_type}\nhost:{host}\nx-amz-date:{amz_date}\n\ncontent-type;host;x-amz-date\n{payload_hash}"))
        };
        let auth = self.sign_v4(service, &date_stamp, &amz_date, &headers_to_sign, &canonical);

        let mut hdrs: Vec<(&str, &str)> = vec![
            ("Content-Type", content_type),
            ("Host", &host),
            ("X-Amz-Date", &amz_date),
            ("Authorization", &auth),
        ];
        if let Some(token) = &self.session_token {
            hdrs.push(("X-Amz-Security-Token", token));
        }

        let req = self.build_request("POST", &url, &hdrs, body.into_bytes());
        let (status, text) = self.send(req).await?;

        if status >= 300 {
            return Err(parse_xml_error(&text, status));
        }
        let root = super::xml::XmlNode::parse(&text)
            .map_err(|e| AwsError::XmlParse(format!("{e}\n--- body ---\n{}", &text[..text.len().min(500)])))?;
        T::from_xml(&root)
            .map_err(|e| AwsError::XmlParse(format!("{e}\n--- body ---\n{}", &text[..text.len().min(500)])))
    }

    /// Like query_request but returns the parsed XML root node directly.
    /// Used by operations that need to unwrap a resultWrapper element.
    pub async fn query_request_raw(
        &self,
        service: &str,
        version: &str,
        action: &str,
        mut params: Vec<(String, String)>,
    ) -> Result<super::xml::XmlNode, AwsError> {
        params.push(("Action".into(), action.into()));
        params.push(("Version".into(), version.into()));
        let body = query_proto::encode_form(&params);

        let host = format!("{service}.{}.amazonaws.com", self.region);
        let url = format!("https://{host}/");

        let (date_stamp, amz_date) = now_stamps();
        let content_type = "application/x-www-form-urlencoded; charset=utf-8";
        let payload_hash = hex_sha256(body.as_bytes());

        let (headers_to_sign, canonical) = if let Some(token) = &self.session_token {
            ("content-type;host;x-amz-date;x-amz-security-token".to_string(),
             format!("POST\n/\n\ncontent-type:{content_type}\nhost:{host}\nx-amz-date:{amz_date}\nx-amz-security-token:{token}\n\ncontent-type;host;x-amz-date;x-amz-security-token\n{payload_hash}"))
        } else {
            ("content-type;host;x-amz-date".to_string(),
             format!("POST\n/\n\ncontent-type:{content_type}\nhost:{host}\nx-amz-date:{amz_date}\n\ncontent-type;host;x-amz-date\n{payload_hash}"))
        };
        let auth = self.sign_v4(service, &date_stamp, &amz_date, &headers_to_sign, &canonical);

        let mut hdrs: Vec<(&str, &str)> = vec![
            ("Content-Type", content_type),
            ("Host", &host),
            ("X-Amz-Date", &amz_date),
            ("Authorization", &auth),
        ];
        if let Some(token) = &self.session_token {
            hdrs.push(("X-Amz-Security-Token", token));
        }

        let req = self.build_request("POST", &url, &hdrs, body.into_bytes());
        let (status, text) = self.send(req).await?;

        if status >= 300 {
            return Err(parse_xml_error(&text, status));
        }
        super::xml::XmlNode::parse(&text)
            .map_err(|e| AwsError::XmlParse(format!("{e}\n--- body ---\n{}", &text[..text.len().min(500)])))
    }

    pub async fn query_request_void(
        &self,
        service: &str,
        version: &str,
        action: &str,
        mut params: Vec<(String, String)>,
    ) -> Result<(), AwsError> {
        params.push(("Action".into(), action.into()));
        params.push(("Version".into(), version.into()));
        let body = query_proto::encode_form(&params);

        let host = format!("{service}.{}.amazonaws.com", self.region);
        let url = format!("https://{host}/");

        let (date_stamp, amz_date) = now_stamps();
        let content_type = "application/x-www-form-urlencoded; charset=utf-8";
        let payload_hash = hex_sha256(body.as_bytes());

        let (headers_to_sign, canonical) = if let Some(token) = &self.session_token {
            ("content-type;host;x-amz-date;x-amz-security-token".to_string(),
             format!("POST\n/\n\ncontent-type:{content_type}\nhost:{host}\nx-amz-date:{amz_date}\nx-amz-security-token:{token}\n\ncontent-type;host;x-amz-date;x-amz-security-token\n{payload_hash}"))
        } else {
            ("content-type;host;x-amz-date".to_string(),
             format!("POST\n/\n\ncontent-type:{content_type}\nhost:{host}\nx-amz-date:{amz_date}\n\ncontent-type;host;x-amz-date\n{payload_hash}"))
        };
        let auth = self.sign_v4(service, &date_stamp, &amz_date, &headers_to_sign, &canonical);

        let mut hdrs: Vec<(&str, &str)> = vec![
            ("Content-Type", content_type),
            ("Host", &host),
            ("X-Amz-Date", &amz_date),
            ("Authorization", &auth),
        ];
        if let Some(token) = &self.session_token {
            hdrs.push(("X-Amz-Security-Token", token));
        }

        let req = self.build_request("POST", &url, &hdrs, body.into_bytes());
        let (status, text) = self.send(req).await?;

        if status >= 300 {
            return Err(parse_xml_error(&text, status));
        }
        Ok(())
    }

    // ── JSON 1.1 protocol (SSM) ────────────────────────────────────────

    pub async fn json_request<T: super::json::FromJsonValue>(
        &self,
        service: &str,
        target: &str,
        input: &impl super::json::ToJsonValue,
    ) -> Result<T, AwsError> {
        let body = serde_json::to_string(&input.to_json())
            .map_err(|e| AwsError::JsonParse(e.to_string()))?;

        let host = format!("{service}.{}.amazonaws.com", self.region);
        let url = format!("https://{host}/");
        let content_type = "application/x-amz-json-1.1";

        let (date_stamp, amz_date) = now_stamps();
        let payload_hash = hex_sha256(body.as_bytes());

        let (headers_to_sign, canonical) = if let Some(token) = &self.session_token {
            ("content-type;host;x-amz-date;x-amz-security-token;x-amz-target",
             format!("POST\n/\n\ncontent-type:{content_type}\nhost:{host}\nx-amz-date:{amz_date}\nx-amz-security-token:{token}\nx-amz-target:{target}\n\ncontent-type;host;x-amz-date;x-amz-security-token;x-amz-target\n{payload_hash}"))
        } else {
            ("content-type;host;x-amz-date;x-amz-target",
             format!("POST\n/\n\ncontent-type:{content_type}\nhost:{host}\nx-amz-date:{amz_date}\nx-amz-target:{target}\n\ncontent-type;host;x-amz-date;x-amz-target\n{payload_hash}"))
        };
        let auth = self.sign_v4(service, &date_stamp, &amz_date, headers_to_sign, &canonical);

        let mut hdrs: Vec<(&str, &str)> = vec![
            ("Content-Type", content_type),
            ("Host", &host),
            ("X-Amz-Date", &amz_date),
            ("X-Amz-Target", target),
            ("Authorization", &auth),
        ];
        if let Some(token) = &self.session_token {
            hdrs.push(("X-Amz-Security-Token", token));
        }

        let req = self.build_request("POST", &url, &hdrs, body.into_bytes());
        let (status, text) = self.send(req).await?;

        if status >= 300 {
            return Err(parse_json_error(&text, status));
        }
        let value: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| AwsError::JsonParse(format!("{e}\n--- body ---\n{}", &text[..text.len().min(500)])))?;
        Ok(T::from_json(&value))
    }

    // ── REST-JSON protocol (Lambda etc.) ───────────────────────────────

    pub async fn rest_json_request<T: super::json::FromJsonValue>(
        &self,
        service: &str,
        method: &str,
        uri: &str,
        query: &[(String, String)],
        extra_headers: &[(String, String)],
        input: &impl super::json::ToJsonValue,
    ) -> Result<T, AwsError> {
        let text = self.rest_json_raw(service, method, uri, query, extra_headers, input).await?;
        let value: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| AwsError::JsonParse(format!("{e}\n--- body ---\n{}", &text[..text.len().min(500)])))?;
        Ok(T::from_json(&value))
    }

    pub async fn rest_json_request_void(
        &self,
        service: &str,
        method: &str,
        uri: &str,
        query: &[(String, String)],
        extra_headers: &[(String, String)],
        input: &impl super::json::ToJsonValue,
    ) -> Result<(), AwsError> {
        self.rest_json_raw(service, method, uri, query, extra_headers, input).await?;
        Ok(())
    }

    async fn rest_json_raw(
        &self,
        service: &str,
        method: &str,
        uri: &str,
        query: &[(String, String)],
        extra_headers: &[(String, String)],
        input: &impl super::json::ToJsonValue,
    ) -> Result<String, AwsError> {
        let body = serde_json::to_string(&input.to_json())
            .map_err(|e| AwsError::JsonParse(e.to_string()))?;

        let (url, canonical_qs, host) = self.rest_url(service, uri, query);
        let content_type = "application/json";

        let (date_stamp, amz_date) = now_stamps();
        let payload_hash = hex_sha256(body.as_bytes());

        let (headers_to_sign, canonical) = if let Some(token) = &self.session_token {
            ("content-type;host;x-amz-date;x-amz-security-token",
             format!("{method}\n{uri}\n{canonical_qs}\ncontent-type:{content_type}\nhost:{host}\nx-amz-date:{amz_date}\nx-amz-security-token:{token}\n\ncontent-type;host;x-amz-date;x-amz-security-token\n{payload_hash}"))
        } else {
            ("content-type;host;x-amz-date",
             format!("{method}\n{uri}\n{canonical_qs}\ncontent-type:{content_type}\nhost:{host}\nx-amz-date:{amz_date}\n\ncontent-type;host;x-amz-date\n{payload_hash}"))
        };
        let auth = self.sign_v4(service, &date_stamp, &amz_date, headers_to_sign, &canonical);

        let mut hdrs: Vec<(&str, &str)> = vec![
            ("Content-Type", content_type),
            ("Host", &host),
            ("X-Amz-Date", &amz_date),
            ("Authorization", &auth),
        ];
        if let Some(token) = &self.session_token {
            hdrs.push(("X-Amz-Security-Token", token));
        }
        let extra_owned: Vec<(String, String)> = extra_headers.to_vec();
        for (k, v) in &extra_owned {
            hdrs.push((k, v));
        }

        let req = self.build_request(method, &url, &hdrs, body.into_bytes());
        let (status, text) = self.send(req).await?;

        if status >= 300 {
            return Err(parse_json_error(&text, status));
        }
        Ok(text)
    }

    // ── REST-XML protocol (S3, Route53, etc.) ────────────────────────

    pub async fn rest_xml_request<T: super::xml::FromXml>(
        &self,
        service: &str,
        method: &str,
        uri: &str,
        query: &[(String, String)],
        extra_headers: &[(String, String)],
    ) -> Result<T, AwsError> {
        let text = self.rest_xml_raw(service, method, uri, query, extra_headers).await?;
        let root = super::xml::XmlNode::parse(&text)
            .map_err(|e| AwsError::XmlParse(format!("{e}\n--- body ---\n{}", &text[..text.len().min(500)])))?;
        T::from_xml(&root)
            .map_err(|e| AwsError::XmlParse(format!("{e}\n--- body ---\n{}", &text[..text.len().min(500)])))
    }

    pub async fn rest_xml_request_void(
        &self,
        service: &str,
        method: &str,
        uri: &str,
        query: &[(String, String)],
        extra_headers: &[(String, String)],
    ) -> Result<(), AwsError> {
        self.rest_xml_raw(service, method, uri, query, extra_headers).await?;
        Ok(())
    }

    async fn rest_xml_raw(
        &self,
        service: &str,
        method: &str,
        uri: &str,
        query: &[(String, String)],
        extra_headers: &[(String, String)],
    ) -> Result<String, AwsError> {
        let (url, canonical_qs, host) = self.rest_url(service, uri, query);

        let (date_stamp, amz_date) = now_stamps();
        let payload_hash = hex_sha256(b"");

        let (headers_to_sign, canonical) = if let Some(token) = &self.session_token {
            ("host;x-amz-content-sha256;x-amz-date;x-amz-security-token",
             format!("{method}\n{uri}\n{canonical_qs}\nhost:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\nx-amz-security-token:{token}\n\nhost;x-amz-content-sha256;x-amz-date;x-amz-security-token\n{payload_hash}"))
        } else {
            ("host;x-amz-content-sha256;x-amz-date",
             format!("{method}\n{uri}\n{canonical_qs}\nhost:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\n\nhost;x-amz-content-sha256;x-amz-date\n{payload_hash}"))
        };
        let auth = self.sign_v4(service, &date_stamp, &amz_date, headers_to_sign, &canonical);

        let mut hdrs: Vec<(&str, &str)> = vec![
            ("Host", &host),
            ("X-Amz-Date", &amz_date),
            ("X-Amz-Content-Sha256", &payload_hash),
            ("Authorization", &auth),
        ];
        if let Some(token) = &self.session_token {
            hdrs.push(("X-Amz-Security-Token", token));
        }
        let extra_owned: Vec<(String, String)> = extra_headers.to_vec();
        for (k, v) in &extra_owned {
            hdrs.push((k, v));
        }

        let req = self.build_request(method, &url, &hdrs, vec![]);
        let (status, text) = self.send(req).await?;

        if status >= 300 {
            return Err(parse_xml_error(&text, status));
        }
        Ok(text)
    }

    // ── URL building ────────────────────────────────────────────────────

    fn rest_url(&self, service: &str, uri: &str, query: &[(String, String)]) -> (String, String, String) {
        let host = format!("{service}.{}.amazonaws.com", self.region);
        let qs = if query.is_empty() {
            String::new()
        } else {
            let s = query.iter()
                .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
                .collect::<Vec<_>>()
                .join("&");
            format!("?{s}")
        };
        let url = format!("https://{host}{uri}{qs}");
        let canonical_qs = if query.is_empty() { String::new() } else {
            let mut sorted = query.to_vec();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            sorted.iter()
                .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
                .collect::<Vec<_>>()
                .join("&")
        };
        (url, canonical_qs, host)
    }

    // ── SigV4 ──────────────────────────────────────────────────────────

    fn sign_v4(
        &self,
        service: &str,
        date_stamp: &str,
        amz_date: &str,
        signed_headers: &str,
        canonical_request: &str,
    ) -> String {
        let scope = format!("{date_stamp}/{}/{service}/aws4_request", self.region);
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
            hex_sha256(canonical_request.as_bytes()),
        );

        let k_date    = hmac_sha256(format!("AWS4{}", self.secret_key).as_bytes(), date_stamp.as_bytes());
        let k_region  = hmac_sha256(&k_date, self.region.as_bytes());
        let k_service = hmac_sha256(&k_region, service.as_bytes());
        let k_signing = hmac_sha256(&k_service, b"aws4_request");
        let signature = hex::encode(hmac_sha256(&k_signing, string_to_sign.as_bytes()));

        format!(
            "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
            self.access_key,
        )
    }
}

// ── Helpers ────────────────────────────────────────────────────────────

fn hex_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn now_stamps() -> (String, String) {
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
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push(hex::HEX_CHARS[(b >> 4) as usize] as char);
                out.push(hex::HEX_CHARS[(b & 0xf) as usize] as char);
            }
        }
    }
    out
}

fn parse_xml_error(body: &str, status: u16) -> AwsError {
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

fn parse_json_error(body: &str, status: u16) -> AwsError {
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

mod hex {
    pub const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";
    pub fn encode(data: impl AsRef<[u8]>) -> String {
        let data = data.as_ref();
        let mut s = String::with_capacity(data.len() * 2);
        for &b in data {
            s.push(HEX_CHARS[(b >> 4) as usize] as char);
            s.push(HEX_CHARS[(b & 0xf) as usize] as char);
        }
        s
    }
}

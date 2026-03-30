use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::Request;
use hyper_rustls::HttpsConnectorBuilder;
use hyper_util::client::legacy::Client as HyperClient;
use hyper_util::rt::TokioExecutor;

use aws_runtime_common::aws::error::AwsError;
use aws_runtime_common::aws::query_proto;
use aws_runtime_common::aws::sign::{self, Credentials};

type HttpClient = HyperClient<hyper_rustls::HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>, Full<Bytes>>;

/// Async AWS HTTP client with SigV4 signing.
///
/// Resolves credentials from the `aws` CLI or environment variables.
/// Uses hyper + rustls for HTTPS.
pub struct Client {
    /// AWS credentials and region.
    pub creds: Credentials,
    http: HttpClient,
}

/// AWS-compatible percent encoding (re-exported for generated code).
pub fn percent_encode(s: &str) -> String {
    sign::percent_encode(s)
}

impl Client {
    /// Create a client, resolving credentials from `aws configure export-credentials`
    /// or falling back to `AWS_ACCESS_KEY_ID`/`AWS_SECRET_ACCESS_KEY` env vars.
    pub fn from_env() -> Self {
        let https = HttpsConnectorBuilder::new()
            .with_webpki_roots()
            .https_only()
            .enable_http1()
            .build();
        Self {
            creds: Credentials::from_env(),
            http: HyperClient::builder(TokioExecutor::new()).build(https),
        }
    }

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

    fn build_request(&self, method: &str, url: &str, headers: &[(&str, &str)], body: Vec<u8>) -> Request<Full<Bytes>> {
        let mut builder = Request::builder().method(method).uri(url);
        for &(k, v) in headers { builder = builder.header(k, v); }
        builder.body(Full::new(Bytes::from(body))).unwrap()
    }

    fn sign(&self, service: &str, date_stamp: &str, amz_date: &str, signed_headers: &str, canonical: &str) -> String {
        sign::sign_v4(&self.creds, service, date_stamp, amz_date, signed_headers, canonical)
    }

    fn host(&self, service: &str) -> String {
        format!("{service}.{}.amazonaws.com", self.creds.region)
    }

    // ── Query protocol (EC2, IAM) ──────────────────────────────────────

    pub async fn query_request<T: aws_runtime_common::aws::xml::FromXml>(
        &self, service: &str, version: &str, action: &str, params: Vec<(String, String)>,
    ) -> Result<T, AwsError> {
        let (status, text) = self.query_raw(service, version, action, params).await?;
        if status >= 300 { return Err(sign::parse_xml_error(&text, status)); }
        let root = aws_runtime_common::aws::xml::XmlNode::parse(&text)
            .map_err(|e| AwsError::XmlParse(format!("{e}\n--- body ---\n{}", &text[..text.len().min(500)])))?;
        T::from_xml(&root)
            .map_err(|e| AwsError::XmlParse(format!("{e}\n--- body ---\n{}", &text[..text.len().min(500)])))
    }

    pub async fn query_request_raw(
        &self, service: &str, version: &str, action: &str, params: Vec<(String, String)>,
    ) -> Result<aws_runtime_common::aws::xml::XmlNode, AwsError> {
        let (status, text) = self.query_raw(service, version, action, params).await?;
        if status >= 300 { return Err(sign::parse_xml_error(&text, status)); }
        aws_runtime_common::aws::xml::XmlNode::parse(&text)
            .map_err(|e| AwsError::XmlParse(format!("{e}\n--- body ---\n{}", &text[..text.len().min(500)])))
    }

    pub async fn query_request_void(
        &self, service: &str, version: &str, action: &str, params: Vec<(String, String)>,
    ) -> Result<(), AwsError> {
        let (status, text) = self.query_raw(service, version, action, params).await?;
        if status >= 300 { return Err(sign::parse_xml_error(&text, status)); }
        Ok(())
    }

    async fn query_raw(&self, service: &str, version: &str, action: &str, mut params: Vec<(String, String)>) -> Result<(u16, String), AwsError> {
        params.push(("Action".into(), action.into()));
        params.push(("Version".into(), version.into()));
        let body = query_proto::encode_form(&params);
        let host = self.host(service);
        let url = format!("https://{host}/");
        let (date_stamp, amz_date) = sign::now_stamps();
        let content_type = "application/x-www-form-urlencoded; charset=utf-8";
        let payload_hash = sign::hex_sha256(body.as_bytes());

        let (hdr_str, canonical) = self.build_canonical_post("/", "", content_type, &host, &amz_date, &payload_hash, None);
        let auth = self.sign(service, &date_stamp, &amz_date, &hdr_str, &canonical);

        let mut hdrs = vec![
            ("Content-Type", content_type),
            ("Host", host.as_str()),
            ("X-Amz-Date", amz_date.as_str()),
            ("Authorization", auth.as_str()),
        ];
        let token_ref;
        if let Some(token) = &self.creds.session_token {
            token_ref = token.clone();
            hdrs.push(("X-Amz-Security-Token", &token_ref));
        }
        let req = self.build_request("POST", &url, &hdrs, body.into_bytes());
        self.send(req).await
    }

    // ── JSON 1.1 protocol (SSM) ────────────────────────────────────────

    pub async fn json_request<T: aws_runtime_common::aws::json::FromJsonValue>(
        &self, service: &str, target: &str, input: &impl aws_runtime_common::aws::json::ToJsonValue,
    ) -> Result<T, AwsError> {
        let body = serde_json::to_string(&input.to_json()).map_err(|e| AwsError::JsonParse(e.to_string()))?;
        let host = self.host(service);
        let url = format!("https://{host}/");
        let content_type = "application/x-amz-json-1.1";
        let (date_stamp, amz_date) = sign::now_stamps();
        let payload_hash = sign::hex_sha256(body.as_bytes());

        let (hdr_str, canonical) = self.build_canonical_post("/", "", content_type, &host, &amz_date, &payload_hash, Some(target));
        let auth = self.sign(service, &date_stamp, &amz_date, &hdr_str, &canonical);

        let mut hdrs = vec![
            ("Content-Type", content_type),
            ("Host", host.as_str()),
            ("X-Amz-Date", amz_date.as_str()),
            ("X-Amz-Target", target),
            ("Authorization", auth.as_str()),
        ];
        let token_ref;
        if let Some(token) = &self.creds.session_token {
            token_ref = token.clone();
            hdrs.push(("X-Amz-Security-Token", &token_ref));
        }
        let req = self.build_request("POST", &url, &hdrs, body.into_bytes());
        let (status, text) = self.send(req).await?;
        if status >= 300 { return Err(sign::parse_json_error(&text, status)); }
        let value: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| AwsError::JsonParse(format!("{e}\n--- body ---\n{}", &text[..text.len().min(500)])))?;
        Ok(T::from_json(&value))
    }

    // ── REST-JSON protocol ─────────────────────────────────────────────

    pub async fn rest_json_request<T: aws_runtime_common::aws::json::FromJsonValue>(
        &self, service: &str, method: &str, uri: &str, query: &[(String, String)],
        extra_headers: &[(String, String)], input: &impl aws_runtime_common::aws::json::ToJsonValue,
    ) -> Result<T, AwsError> {
        let text = self.rest_json_raw(service, method, uri, query, extra_headers, input).await?;
        let value: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| AwsError::JsonParse(format!("{e}\n--- body ---\n{}", &text[..text.len().min(500)])))?;
        Ok(T::from_json(&value))
    }

    pub async fn rest_json_request_void(
        &self, service: &str, method: &str, uri: &str, query: &[(String, String)],
        extra_headers: &[(String, String)], input: &impl aws_runtime_common::aws::json::ToJsonValue,
    ) -> Result<(), AwsError> {
        self.rest_json_raw(service, method, uri, query, extra_headers, input).await?;
        Ok(())
    }

    async fn rest_json_raw(
        &self, service: &str, method: &str, uri: &str, query: &[(String, String)],
        extra_headers: &[(String, String)], input: &impl aws_runtime_common::aws::json::ToJsonValue,
    ) -> Result<String, AwsError> {
        let body = serde_json::to_string(&input.to_json()).map_err(|e| AwsError::JsonParse(e.to_string()))?;
        let host = self.host(service);
        let qs = sign::build_query_string(query);
        let url = format!("https://{host}{uri}{qs}");
        let canonical_qs = sign::canonical_query_string(query);
        let content_type = "application/json";
        let (date_stamp, amz_date) = sign::now_stamps();
        let payload_hash = sign::hex_sha256(body.as_bytes());

        let (hdr_str, canonical) = self.build_canonical_rest(method, uri, &canonical_qs, content_type, &host, &amz_date, &payload_hash);
        let auth = self.sign(service, &date_stamp, &amz_date, &hdr_str, &canonical);

        let mut hdrs = vec![
            ("Content-Type", content_type),
            ("Host", host.as_str()),
            ("X-Amz-Date", amz_date.as_str()),
            ("Authorization", auth.as_str()),
        ];
        let token_ref;
        if let Some(token) = &self.creds.session_token {
            token_ref = token.clone();
            hdrs.push(("X-Amz-Security-Token", &token_ref));
        }
        for (k, v) in extra_headers { hdrs.push((k, v)); }
        let req = self.build_request(method, &url, &hdrs, body.into_bytes());
        let (status, text) = self.send(req).await?;
        if status >= 300 { return Err(sign::parse_json_error(&text, status)); }
        Ok(text)
    }

    // ── REST-XML protocol (S3) ─────────────────────────────────────────

    pub async fn rest_xml_request<T: aws_runtime_common::aws::xml::FromXml>(
        &self, service: &str, method: &str, uri: &str, query: &[(String, String)],
        extra_headers: &[(String, String)],
    ) -> Result<T, AwsError> {
        let text = self.rest_xml_raw(service, method, uri, query, extra_headers).await?;
        let root = aws_runtime_common::aws::xml::XmlNode::parse(&text)
            .map_err(|e| AwsError::XmlParse(format!("{e}\n--- body ---\n{}", &text[..text.len().min(500)])))?;
        T::from_xml(&root)
            .map_err(|e| AwsError::XmlParse(format!("{e}\n--- body ---\n{}", &text[..text.len().min(500)])))
    }

    pub async fn rest_xml_request_void(
        &self, service: &str, method: &str, uri: &str, query: &[(String, String)],
        extra_headers: &[(String, String)],
    ) -> Result<(), AwsError> {
        self.rest_xml_raw(service, method, uri, query, extra_headers).await?;
        Ok(())
    }

    async fn rest_xml_raw(
        &self, service: &str, method: &str, uri: &str, query: &[(String, String)],
        extra_headers: &[(String, String)],
    ) -> Result<String, AwsError> {
        let host = self.host(service);
        let qs = sign::build_query_string(query);
        let url = format!("https://{host}{uri}{qs}");
        let canonical_qs = sign::canonical_query_string(query);
        let (date_stamp, amz_date) = sign::now_stamps();
        let payload_hash = sign::hex_sha256(b"");

        let (hdr_str, canonical) = self.build_canonical_s3(method, uri, &canonical_qs, &host, &amz_date, &payload_hash);
        let auth = self.sign(service, &date_stamp, &amz_date, &hdr_str, &canonical);

        let mut hdrs = vec![
            ("Host", host.as_str()),
            ("X-Amz-Date", amz_date.as_str()),
            ("X-Amz-Content-Sha256", payload_hash.as_str()),
            ("Authorization", auth.as_str()),
        ];
        let token_ref;
        if let Some(token) = &self.creds.session_token {
            token_ref = token.clone();
            hdrs.push(("X-Amz-Security-Token", &token_ref));
        }
        for (k, v) in extra_headers { hdrs.push((k, v)); }
        let req = self.build_request(method, &url, &hdrs, vec![]);
        let (status, text) = self.send(req).await?;
        if status >= 300 { return Err(sign::parse_xml_error(&text, status)); }
        Ok(text)
    }

    // ── Canonical request builders ─────────────────────────────────────

    fn build_canonical_post(&self, uri: &str, qs: &str, content_type: &str, host: &str, amz_date: &str, payload_hash: &str, target: Option<&str>) -> (String, String) {
        if let Some(token) = &self.creds.session_token {
            if let Some(tgt) = target {
                ("content-type;host;x-amz-date;x-amz-security-token;x-amz-target".into(),
                 format!("POST\n{uri}\n{qs}\ncontent-type:{content_type}\nhost:{host}\nx-amz-date:{amz_date}\nx-amz-security-token:{token}\nx-amz-target:{tgt}\n\ncontent-type;host;x-amz-date;x-amz-security-token;x-amz-target\n{payload_hash}"))
            } else {
                ("content-type;host;x-amz-date;x-amz-security-token".into(),
                 format!("POST\n{uri}\n{qs}\ncontent-type:{content_type}\nhost:{host}\nx-amz-date:{amz_date}\nx-amz-security-token:{token}\n\ncontent-type;host;x-amz-date;x-amz-security-token\n{payload_hash}"))
            }
        } else if let Some(tgt) = target {
            ("content-type;host;x-amz-date;x-amz-target".into(),
             format!("POST\n{uri}\n{qs}\ncontent-type:{content_type}\nhost:{host}\nx-amz-date:{amz_date}\nx-amz-target:{tgt}\n\ncontent-type;host;x-amz-date;x-amz-target\n{payload_hash}"))
        } else {
            ("content-type;host;x-amz-date".into(),
             format!("POST\n{uri}\n{qs}\ncontent-type:{content_type}\nhost:{host}\nx-amz-date:{amz_date}\n\ncontent-type;host;x-amz-date\n{payload_hash}"))
        }
    }

    fn build_canonical_rest(&self, method: &str, uri: &str, qs: &str, content_type: &str, host: &str, amz_date: &str, payload_hash: &str) -> (String, String) {
        if let Some(token) = &self.creds.session_token {
            ("content-type;host;x-amz-date;x-amz-security-token".into(),
             format!("{method}\n{uri}\n{qs}\ncontent-type:{content_type}\nhost:{host}\nx-amz-date:{amz_date}\nx-amz-security-token:{token}\n\ncontent-type;host;x-amz-date;x-amz-security-token\n{payload_hash}"))
        } else {
            ("content-type;host;x-amz-date".into(),
             format!("{method}\n{uri}\n{qs}\ncontent-type:{content_type}\nhost:{host}\nx-amz-date:{amz_date}\n\ncontent-type;host;x-amz-date\n{payload_hash}"))
        }
    }

    fn build_canonical_s3(&self, method: &str, uri: &str, qs: &str, host: &str, amz_date: &str, payload_hash: &str) -> (String, String) {
        if let Some(token) = &self.creds.session_token {
            ("host;x-amz-content-sha256;x-amz-date;x-amz-security-token".into(),
             format!("{method}\n{uri}\n{qs}\nhost:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\nx-amz-security-token:{token}\n\nhost;x-amz-content-sha256;x-amz-date;x-amz-security-token\n{payload_hash}"))
        } else {
            ("host;x-amz-content-sha256;x-amz-date".into(),
             format!("{method}\n{uri}\n{qs}\nhost:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\n\nhost;x-amz-content-sha256;x-amz-date\n{payload_hash}"))
        }
    }
}

use aws_runtime_common::serde_json;

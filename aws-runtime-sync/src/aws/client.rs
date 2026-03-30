use aws_runtime_common::aws::error::AwsError;
use aws_runtime_common::aws::query_proto;
use aws_runtime_common::aws::sign::{self, Credentials};

pub struct Client {
    pub creds: Credentials,
    agent: ureq::Agent,
}

/// AWS-compatible percent encoding (re-exported for generated code).
pub fn percent_encode(s: &str) -> String {
    sign::percent_encode(s)
}

impl Client {
    pub fn from_env() -> Self {
        Self {
            creds: Credentials::from_env(),
            agent: ureq::Agent::new_with_defaults(),
        }
    }

    fn send(&self, method: &str, url: &str, headers: &[(&str, &str)], body: &[u8]) -> Result<(u16, String), AwsError> {
        let mut builder = http::Request::builder().method(method).uri(url);
        for &(k, v) in headers {
            builder = builder.header(k, v);
        }
        let req = builder.body(body.to_vec()).map_err(|e| AwsError::Http(e.to_string()))?;
        let resp = self.agent.run(req).map_err(|e| AwsError::Http(e.to_string()))?;
        let status = resp.status().as_u16();
        let text = resp.into_body().read_to_string()
            .map_err(|e| AwsError::Http(e.to_string()))?;
        Ok((status, text))
    }

    fn host(&self, service: &str) -> String {
        format!("{service}.{}.amazonaws.com", self.creds.region)
    }

    fn sign(&self, service: &str, date_stamp: &str, amz_date: &str, signed_headers: &str, canonical: &str) -> String {
        sign::sign_v4(&self.creds, service, date_stamp, amz_date, signed_headers, canonical)
    }

    // ── Query protocol (EC2, IAM) ──────────────────────────────────────

    pub fn query_request<T: aws_runtime_common::aws::xml::FromXml>(
        &self, service: &str, version: &str, action: &str, params: Vec<(String, String)>,
    ) -> Result<T, AwsError> {
        let (status, text) = self.query_raw(service, version, action, params)?;
        if status >= 300 { return Err(sign::parse_xml_error(&text, status)); }
        let root = aws_runtime_common::aws::xml::XmlNode::parse(&text)
            .map_err(|e| AwsError::XmlParse(format!("{e}\n--- body ---\n{}", &text[..text.len().min(500)])))?;
        T::from_xml(&root)
            .map_err(|e| AwsError::XmlParse(format!("{e}\n--- body ---\n{}", &text[..text.len().min(500)])))
    }

    pub fn query_request_raw(
        &self, service: &str, version: &str, action: &str, params: Vec<(String, String)>,
    ) -> Result<aws_runtime_common::aws::xml::XmlNode, AwsError> {
        let (status, text) = self.query_raw(service, version, action, params)?;
        if status >= 300 { return Err(sign::parse_xml_error(&text, status)); }
        aws_runtime_common::aws::xml::XmlNode::parse(&text)
            .map_err(|e| AwsError::XmlParse(format!("{e}\n--- body ---\n{}", &text[..text.len().min(500)])))
    }

    pub fn query_request_void(
        &self, service: &str, version: &str, action: &str, params: Vec<(String, String)>,
    ) -> Result<(), AwsError> {
        let (status, text) = self.query_raw(service, version, action, params)?;
        if status >= 300 { return Err(sign::parse_xml_error(&text, status)); }
        Ok(())
    }

    fn query_raw(&self, service: &str, version: &str, action: &str, mut params: Vec<(String, String)>) -> Result<(u16, String), AwsError> {
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

        let mut hdrs: Vec<(&str, &str)> = vec![
            ("Content-Type", content_type),
            ("Host", &host),
            ("X-Amz-Date", &amz_date),
            ("Authorization", &auth),
        ];
        let token_ref;
        if let Some(token) = &self.creds.session_token {
            token_ref = token.clone();
            hdrs.push(("X-Amz-Security-Token", &token_ref));
        }
        self.send("POST", &url, &hdrs, body.as_bytes())
    }

    // ── JSON 1.1 protocol (SSM) ────────────────────────────────────────

    pub fn json_request<T: aws_runtime_common::aws::json::FromJsonValue>(
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

        let mut hdrs: Vec<(&str, &str)> = vec![
            ("Content-Type", content_type),
            ("Host", &host),
            ("X-Amz-Date", &amz_date),
            ("X-Amz-Target", target),
            ("Authorization", &auth),
        ];
        let token_ref;
        if let Some(token) = &self.creds.session_token {
            token_ref = token.clone();
            hdrs.push(("X-Amz-Security-Token", &token_ref));
        }
        let (status, text) = self.send("POST", &url, &hdrs, body.as_bytes())?;
        if status >= 300 { return Err(sign::parse_json_error(&text, status)); }
        let value: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| AwsError::JsonParse(format!("{e}\n--- body ---\n{}", &text[..text.len().min(500)])))?;
        Ok(T::from_json(&value))
    }

    // ── REST-JSON protocol ─────────────────────────────────────────────

    pub fn rest_json_request<T: aws_runtime_common::aws::json::FromJsonValue>(
        &self, service: &str, method: &str, uri: &str, query: &[(String, String)],
        extra_headers: &[(String, String)], input: &impl aws_runtime_common::aws::json::ToJsonValue,
    ) -> Result<T, AwsError> {
        let text = self.rest_json_raw(service, method, uri, query, extra_headers, input)?;
        let value: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| AwsError::JsonParse(format!("{e}\n--- body ---\n{}", &text[..text.len().min(500)])))?;
        Ok(T::from_json(&value))
    }

    pub fn rest_json_request_void(
        &self, service: &str, method: &str, uri: &str, query: &[(String, String)],
        extra_headers: &[(String, String)], input: &impl aws_runtime_common::aws::json::ToJsonValue,
    ) -> Result<(), AwsError> {
        self.rest_json_raw(service, method, uri, query, extra_headers, input)?;
        Ok(())
    }

    fn rest_json_raw(
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

        let mut hdrs: Vec<(&str, &str)> = vec![
            ("Content-Type", content_type),
            ("Host", &host),
            ("X-Amz-Date", &amz_date),
            ("Authorization", &auth),
        ];
        let token_ref;
        if let Some(token) = &self.creds.session_token {
            token_ref = token.clone();
            hdrs.push(("X-Amz-Security-Token", &token_ref));
        }
        for (k, v) in extra_headers { hdrs.push((k, v)); }
        let (status, text) = self.send(method, &url, &hdrs, body.as_bytes())?;
        if status >= 300 { return Err(sign::parse_json_error(&text, status)); }
        Ok(text)
    }

    // ── REST-XML protocol (S3) ─────────────────────────────────────────

    pub fn rest_xml_request<T: aws_runtime_common::aws::xml::FromXml>(
        &self, service: &str, method: &str, uri: &str, query: &[(String, String)],
        extra_headers: &[(String, String)],
    ) -> Result<T, AwsError> {
        let text = self.rest_xml_raw(service, method, uri, query, extra_headers)?;
        let root = aws_runtime_common::aws::xml::XmlNode::parse(&text)
            .map_err(|e| AwsError::XmlParse(format!("{e}\n--- body ---\n{}", &text[..text.len().min(500)])))?;
        T::from_xml(&root)
            .map_err(|e| AwsError::XmlParse(format!("{e}\n--- body ---\n{}", &text[..text.len().min(500)])))
    }

    pub fn rest_xml_request_void(
        &self, service: &str, method: &str, uri: &str, query: &[(String, String)],
        extra_headers: &[(String, String)],
    ) -> Result<(), AwsError> {
        self.rest_xml_raw(service, method, uri, query, extra_headers)?;
        Ok(())
    }

    fn rest_xml_raw(
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

        let mut hdrs: Vec<(&str, &str)> = vec![
            ("Host", &host),
            ("X-Amz-Date", &amz_date),
            ("X-Amz-Content-Sha256", &payload_hash),
            ("Authorization", &auth),
        ];
        let token_ref;
        if let Some(token) = &self.creds.session_token {
            token_ref = token.clone();
            hdrs.push(("X-Amz-Security-Token", &token_ref));
        }
        for (k, v) in extra_headers { hdrs.push((k, v)); }
        let (status, text) = self.send(method, &url, &hdrs, &[])?;
        if status >= 300 { return Err(sign::parse_xml_error(&text, status)); }
        Ok(text)
    }

    // ── Canonical request builders (same logic as async) ───────────────

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

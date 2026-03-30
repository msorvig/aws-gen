/// Error type for AWS API operations.
#[derive(Debug)]
pub enum AwsError {
    /// HTTP transport error (connection failure, timeout, etc.).
    Http(String),
    /// Failed to parse an XML response body.
    XmlParse(String),
    /// Failed to parse a JSON response body.
    JsonParse(String),
    /// AWS returned a service-level error with a code and message.
    AwsService { code: String, message: String },
}

impl std::fmt::Display for AwsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AwsError::Http(e) => write!(f, "HTTP error: {e}"),
            AwsError::XmlParse(e) => write!(f, "XML parse error: {e}"),
            AwsError::JsonParse(e) => write!(f, "JSON parse error: {e}"),
            AwsError::AwsService { code, message } => write!(f, "AWS {code}: {message}"),
        }
    }
}

impl std::error::Error for AwsError {}

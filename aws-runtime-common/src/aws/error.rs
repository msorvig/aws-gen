#[derive(Debug)]
pub enum AwsError {
    Http(String),
    XmlParse(String),
    JsonParse(String),
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

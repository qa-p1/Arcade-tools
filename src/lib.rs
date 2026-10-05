pub mod link;
pub mod manager;
pub mod paths;
pub mod platform;
pub mod release;
pub mod source;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, serde::Serialize)]
pub struct Error {
    pub code: String,
    pub message: String,
}

impl Error {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::new("io", e.to_string())
    }
}
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::new("manifest", e.to_string())
    }
}
impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        Self::new("network", e.to_string())
    }
}
impl From<arcade_link::LinkError> for Error {
    fn from(e: arcade_link::LinkError) -> Self {
        Self::new(e.code.as_str(), e.to_string())
    }
}

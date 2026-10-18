pub mod apps;
pub mod archive;
pub mod backup;
pub mod catalog;
pub mod dashboard;
pub mod diagnostics;
pub mod diagnostics_archive;
pub mod docs;
pub mod engines;
pub mod health;
pub mod installation;
pub mod link;
pub mod manager;
pub mod markdown;
pub mod paths;
pub mod platform;
pub mod portable_backup;
pub mod release;
pub mod removal;
pub mod self_update;
pub mod shortcut_control;
pub mod source;
pub mod updates;
pub mod windows_login;

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
        // The library's own text ("error sending request for url (…)") means
        // nothing to a user; keep it in the log and say what happened.
        let mut detail = e.to_string();
        let mut source = std::error::Error::source(&e);
        while let Some(cause) = source {
            detail = format!("{detail}: {cause}");
            source = cause.source();
        }
        eprintln!("Network: {detail}");
        let message = if e.is_timeout() {
            "The connection timed out. Check your internet connection and try again."
        } else if e.is_connect() {
            "Couldn't connect. Check your internet connection and try again."
        } else if e.is_body() || e.is_decode() {
            "The download was interrupted. Try again."
        } else {
            "A network error stopped this. Check your internet connection and try again."
        };
        Self::new("network", message)
    }
}
impl From<arcade_link::LinkError> for Error {
    fn from(e: arcade_link::LinkError) -> Self {
        Self::new(e.code.as_str(), e.to_string())
    }
}

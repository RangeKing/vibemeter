use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("database operation failed")]
    Database(#[from] rusqlite::Error),
    #[error("file operation failed")]
    Io(#[from] std::io::Error),
    #[error("data could not be decoded")]
    Json(#[from] serde_json::Error),
    #[error("network request failed")]
    Network(#[from] reqwest::Error),
    #[error("export was blocked by Share Guard")]
    PrivacyBlocked,
    #[error("unsupported export format")]
    UnsupportedExport,
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("provider data is unavailable: {0}")]
    ProviderUnavailable(String),
    #[error("rendering failed: {0}")]
    Render(String),
}

impl serde::Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;

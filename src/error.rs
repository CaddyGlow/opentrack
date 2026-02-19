use std::error::Error as StdError;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("provider not found: {0}")]
    ProviderNotFound(String),

    #[error("provider '{provider}' cannot auto-detect parcel ID '{id}'")]
    AutoDetectFailed { provider: String, id: String },

    #[error("ambiguous parcel ID '{0}': matched multiple providers")]
    AutoDetectAmbiguous(String),

    #[error("HTTP error: {0}")]
    Http(#[from] wreq::Error),

    #[error("JSON parse error: {0}")]
    Parse(#[from] serde_json::Error),

    #[error("TOML parse error: {0}")]
    TomlParse(#[from] toml::de::Error),

    #[error("TOML serialize error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("missing required config field: {0}")]
    MissingConfig(&'static str),

    #[error("provider returned error: {message} (code {code})")]
    ProviderError { code: u32, message: String },

    #[error("cache error: {0}")]
    Cache(String),

    #[error("notification error ({notifier}): {source}")]
    Notification {
        notifier: String,
        source: Box<dyn StdError + Send + Sync>,
    },

    #[error("unsupported command: {0}")]
    Unsupported(&'static str),
}

pub type Result<T> = std::result::Result<T, Error>;

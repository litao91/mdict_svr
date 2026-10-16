//! Error types shared across the crate.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("malformed mdict file: {0}")]
    Mdx(String),

    #[error("malformed qmd container: {0}")]
    Qmd(String),

    #[error("decompression failed: {0}")]
    Decompress(String),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;

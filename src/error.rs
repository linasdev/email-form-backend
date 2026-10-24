use std::fmt;

use actix_web::{ResponseError, http::StatusCode};

#[derive(Debug)]
pub enum EmailFormError {
    Config(config::ConfigError),
    Io(std::io::Error),
}

impl fmt::Display for EmailFormError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(err) => write!(f, "configuration error: {err}"),
            Self::Io(err) => write!(f, "io error: {err}"),
        }
    }
}

impl std::error::Error for EmailFormError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Config(err) => Some(err),
            Self::Io(err) => Some(err),
        }
    }
}

impl ResponseError for EmailFormError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::Config(_) | Self::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl From<config::ConfigError> for EmailFormError {
    fn from(err: config::ConfigError) -> Self {
        Self::Config(err)
    }
}

impl From<std::io::Error> for EmailFormError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

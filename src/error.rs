use std::fmt;

use actix_web::{ResponseError, http::StatusCode};

#[derive(Debug)]
pub enum EmailFormError {
    Config(config::ConfigError),
    Io(std::io::Error),
    Validation(String),
    Recaptcha(String),
    Http(reqwest::Error),
}

impl fmt::Display for EmailFormError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(err) => write!(f, "configuration error: {err}"),
            Self::Io(err) => write!(f, "io error: {err}"),
            Self::Validation(message) => write!(f, "{message}"),
            Self::Recaptcha(message) => write!(f, "recaptcha verification failed: {message}"),
            Self::Http(err) => write!(f, "http error: {err}"),
        }
    }
}

impl std::error::Error for EmailFormError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Config(err) => Some(err),
            Self::Io(err) => Some(err),
            Self::Validation(_) | Self::Recaptcha(_) => None,
            Self::Http(err) => Some(err),
        }
    }
}

impl ResponseError for EmailFormError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::Config(_) | Self::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Validation(_) | Self::Recaptcha(_) => StatusCode::BAD_REQUEST,
            Self::Http(_) => StatusCode::BAD_GATEWAY,
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

impl From<reqwest::Error> for EmailFormError {
    fn from(err: reqwest::Error) -> Self {
        Self::Http(err)
    }
}

impl From<validator::ValidationErrors> for EmailFormError {
    fn from(err: validator::ValidationErrors) -> Self {
        let mut fields: Vec<_> = err
            .field_errors()
            .keys()
            .map(|field| field.to_string())
            .collect();
        fields.sort();

        Self::Validation(format!("invalid fields: {}", fields.join(", ")))
    }
}

#[cfg(test)]
mod tests {
    use actix_web::{ResponseError, http::StatusCode};
    use googletest::prelude::*;
    use validator::Validate;

    use super::*;

    #[derive(Validate)]
    struct Sample {
        #[validate(length(min = 1))]
        b: String,
        #[validate(length(min = 1))]
        a: String,
    }

    #[actix_web::test]
    async fn validation_error_is_bad_request() {
        let error = EmailFormError::Validation("bad".to_string());

        assert_that!(error.status_code(), eq(StatusCode::BAD_REQUEST));
        assert_that!(error.to_string(), eq("bad"));
    }

    #[actix_web::test]
    async fn config_error_is_internal_server_error() {
        let error = EmailFormError::from(config::ConfigError::Message("oops".to_string()));

        assert_that!(error.status_code(), eq(StatusCode::INTERNAL_SERVER_ERROR));
        assert_that!(error.to_string(), eq("configuration error: oops"));
    }

    #[actix_web::test]
    async fn io_error_is_internal_server_error() {
        let error = EmailFormError::from(std::io::Error::other("broken"));

        assert_that!(error.status_code(), eq(StatusCode::INTERNAL_SERVER_ERROR));
        assert_that!(error.to_string(), eq("io error: broken"));
    }

    #[actix_web::test]
    async fn validation_errors_list_sorted_field_names() {
        let sample = Sample {
            b: String::new(),
            a: String::new(),
        };

        let error = EmailFormError::from(sample.validate().unwrap_err());

        assert_that!(error.to_string(), eq("invalid fields: a, b"));
    }

    #[actix_web::test]
    async fn recaptcha_error_is_bad_request() {
        let error = EmailFormError::Recaptcha("score too low".to_string());

        assert_that!(error.status_code(), eq(StatusCode::BAD_REQUEST));
        assert_that!(
            error.to_string(),
            eq("recaptcha verification failed: score too low")
        );
    }

    #[actix_web::test]
    async fn http_error_is_bad_gateway() {
        let source = reqwest::Client::new()
            .get("http://127.0.0.1:1")
            .send()
            .await
            .unwrap_err();
        let error = EmailFormError::from(source);

        assert_that!(error.status_code(), eq(StatusCode::BAD_GATEWAY));
    }
}

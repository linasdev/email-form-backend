use std::fmt;

use actix_web::http::header::ContentType;
use actix_web::{HttpResponse, ResponseError, http::StatusCode};

#[derive(Debug)]
pub enum EmailFormError {
    Config(config::ConfigError),
    Io(std::io::Error),
    Validation(String),
    Recaptcha(String),
    Http(reqwest::Error),
    Message(lettre::error::Error),
    Smtp(Box<dyn std::error::Error + Send + Sync>),
}

impl fmt::Display for EmailFormError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(err) => write!(f, "configuration error: {err}"),
            Self::Io(err) => write!(f, "io error: {err}"),
            Self::Validation(message) => write!(f, "{message}"),
            Self::Recaptcha(message) => write!(f, "recaptcha verification failed: {message}"),
            Self::Http(err) => write!(f, "http error: {err}"),
            Self::Message(err) => write!(f, "message error: {err}"),
            Self::Smtp(err) => write!(f, "smtp error: {err}"),
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
            Self::Message(err) => Some(err),
            Self::Smtp(err) => Some(err.as_ref()),
        }
    }
}

impl ResponseError for EmailFormError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::Config(_) | Self::Io(_) | Self::Message(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Validation(_) | Self::Recaptcha(_) => StatusCode::BAD_REQUEST,
            Self::Http(_) | Self::Smtp(_) => StatusCode::BAD_GATEWAY,
        }
    }

    fn error_response(&self) -> HttpResponse {
        let status = self.status_code();
        let message = match self {
            Self::Validation(_) => {
                log::debug!("rejected submission: {self}");
                self.to_string()
            }
            Self::Recaptcha(_) => {
                log::warn!("rejected submission: {self}");
                self.to_string()
            }
            _ => {
                log::error!("request failed: {self}");
                status.canonical_reason().unwrap_or("Error").to_string()
            }
        };

        HttpResponse::build(status)
            .insert_header(ContentType::plaintext())
            .body(message)
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

impl From<lettre::error::Error> for EmailFormError {
    fn from(err: lettre::error::Error) -> Self {
        Self::Message(err)
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

    #[actix_web::test]
    async fn smtp_error_is_bad_gateway() {
        let error = EmailFormError::Smtp(Box::new(std::io::Error::other("down")));

        assert_that!(error.status_code(), eq(StatusCode::BAD_GATEWAY));
        assert_that!(error.to_string(), eq("smtp error: down"));
    }

    #[actix_web::test]
    async fn message_error_is_internal_server_error() {
        let error = EmailFormError::from(lettre::error::Error::MissingFrom);

        assert_that!(error.status_code(), eq(StatusCode::INTERNAL_SERVER_ERROR));
    }

    async fn body_of(error: &EmailFormError) -> String {
        let body = actix_web::body::to_bytes(error.error_response().into_body())
            .await
            .unwrap();

        String::from_utf8(body.to_vec()).unwrap()
    }

    #[actix_web::test]
    async fn client_errors_expose_their_message() {
        let validation = EmailFormError::Validation("invalid fields: email".to_string());
        let recaptcha = EmailFormError::Recaptcha("score too low".to_string());

        assert_that!(body_of(&validation).await, eq("invalid fields: email"));
        assert_that!(
            body_of(&recaptcha).await,
            eq("recaptcha verification failed: score too low")
        );
    }

    #[actix_web::test]
    async fn server_errors_hide_their_details() {
        let config = EmailFormError::from(config::ConfigError::Message("secret path".to_string()));
        let io = EmailFormError::from(std::io::Error::other("disk details"));
        let smtp = EmailFormError::Smtp(Box::new(std::io::Error::other("smtp.internal:25")));

        assert_that!(body_of(&config).await, eq("Internal Server Error"));
        assert_that!(body_of(&io).await, eq("Internal Server Error"));
        assert_that!(body_of(&smtp).await, eq("Bad Gateway"));
    }
}

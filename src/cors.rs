use actix_cors::Cors;
use actix_web::http::header::CONTENT_TYPE;
use actix_web::middleware::Condition;

use crate::error::EmailFormError;
use crate::settings::CorsSettings;

fn validate_origin(origin: &str) -> Result<(), EmailFormError> {
    let has_scheme = origin.starts_with("http://") || origin.starts_with("https://");
    let has_host = origin
        .split_once("://")
        .is_some_and(|(_, host)| !host.is_empty() && !host.contains('/'));

    if origin == "*" || (has_scheme && has_host) {
        Ok(())
    } else {
        Err(config::ConfigError::Message(format!(
            "invalid cors origin `{origin}`: expected `*` or a scheme and host such as `https://example.com`"
        ))
        .into())
    }
}

#[derive(Debug, Clone)]
pub struct AllowedOrigins(Vec<String>);

impl AllowedOrigins {
    pub fn parse(settings: &CorsSettings) -> Result<Self, EmailFormError> {
        if settings.allowed_origins.is_empty() {
            return Err(config::ConfigError::Message(
                "cors.allowed_origins must not be empty; remove the cors section to disable cors"
                    .to_string(),
            )
            .into());
        }

        for origin in &settings.allowed_origins {
            validate_origin(origin)?;
        }

        Ok(Self(settings.allowed_origins.clone()))
    }

    fn cors(&self) -> Cors {
        let cors = if self.0.iter().any(|origin| origin == "*") {
            Cors::default().allow_any_origin().send_wildcard()
        } else {
            self.0
                .iter()
                .fold(Cors::default(), |cors, origin| cors.allowed_origin(origin))
        };

        cors.allowed_methods(["POST"])
            .allowed_header(CONTENT_TYPE)
            .max_age(3600)
    }
}

pub fn middleware(origins: Option<&AllowedOrigins>) -> Condition<Cors> {
    Condition::new(
        origins.is_some(),
        origins.map_or_else(Cors::default, AllowedOrigins::cors),
    )
}

#[cfg(test)]
mod tests {
    use actix_web::http::{Method, StatusCode, header};
    use actix_web::{App, HttpResponse, test, web};
    use googletest::prelude::*;

    use super::*;

    fn settings(origins: &[&str]) -> CorsSettings {
        CorsSettings {
            allowed_origins: origins.iter().map(ToString::to_string).collect(),
        }
    }

    struct Reply {
        status: StatusCode,
        headers: header::HeaderMap,
    }

    impl Reply {
        fn header(&self, name: header::HeaderName) -> Option<String> {
            self.headers
                .get(name)
                .map(|value| value.to_str().unwrap().to_string())
        }
    }

    async fn call(origins: Option<&[&str]>, request: test::TestRequest) -> Reply {
        let origins = origins.map(|origins| AllowedOrigins::parse(&settings(origins)).unwrap());
        let app = test::init_service(
            App::new()
                .wrap(middleware(origins.as_ref()))
                .route("/submit", web::post().to(HttpResponse::Ok)),
        )
        .await;
        let response = test::call_service(&app, request.to_request()).await;

        Reply {
            status: response.status(),
            headers: response.headers().clone(),
        }
    }

    #[actix_web::test]
    async fn is_disabled_without_cors_settings() {
        let request = test::TestRequest::post()
            .uri("/submit")
            .insert_header((header::ORIGIN, "https://example.com"));

        let response = call(None, request).await;

        assert_that!(response.status, eq(StatusCode::OK));
        assert_that!(response.header(header::ACCESS_CONTROL_ALLOW_ORIGIN), none());
    }

    #[actix_web::test]
    async fn answers_preflight_for_allowed_origin() {
        let request = test::TestRequest::default()
            .method(Method::OPTIONS)
            .uri("/submit")
            .insert_header((header::ORIGIN, "https://example.com"))
            .insert_header((header::ACCESS_CONTROL_REQUEST_METHOD, "POST"))
            .insert_header((header::ACCESS_CONTROL_REQUEST_HEADERS, "content-type"));

        let response = call(Some(&["https://example.com"]), request).await;

        assert_that!(response.status, eq(StatusCode::OK));
        assert_that!(
            response.header(header::ACCESS_CONTROL_ALLOW_ORIGIN),
            some(eq("https://example.com"))
        );
        assert_that!(
            response.header(header::ACCESS_CONTROL_ALLOW_METHODS),
            some(eq("POST"))
        );
        assert_that!(
            response.header(header::ACCESS_CONTROL_MAX_AGE),
            some(eq("3600"))
        );
    }

    #[actix_web::test]
    async fn adds_headers_to_actual_request_from_allowed_origin() {
        let request = test::TestRequest::post()
            .uri("/submit")
            .insert_header((header::ORIGIN, "https://example.com"));

        let response = call(Some(&["https://example.com", "https://other.com"]), request).await;

        assert_that!(response.status, eq(StatusCode::OK));
        assert_that!(
            response.header(header::ACCESS_CONTROL_ALLOW_ORIGIN),
            some(eq("https://example.com"))
        );
    }

    #[actix_web::test]
    async fn rejects_preflight_from_origin_that_is_not_allowed() {
        let request = test::TestRequest::default()
            .method(Method::OPTIONS)
            .uri("/submit")
            .insert_header((header::ORIGIN, "https://evil.com"))
            .insert_header((header::ACCESS_CONTROL_REQUEST_METHOD, "POST"));

        let response = call(Some(&["https://example.com"]), request).await;

        assert_that!(response.status, eq(StatusCode::BAD_REQUEST));
        assert_that!(response.header(header::ACCESS_CONTROL_ALLOW_ORIGIN), none());
    }

    #[actix_web::test]
    async fn omits_headers_for_actual_request_from_origin_that_is_not_allowed() {
        let request = test::TestRequest::post()
            .uri("/submit")
            .insert_header((header::ORIGIN, "https://evil.com"));

        let response = call(Some(&["https://example.com"]), request).await;

        assert_that!(response.header(header::ACCESS_CONTROL_ALLOW_ORIGIN), none());
    }

    #[actix_web::test]
    async fn allows_any_origin_with_wildcard() {
        let request = test::TestRequest::post()
            .uri("/submit")
            .insert_header((header::ORIGIN, "https://anything.com"));

        let response = call(Some(&["*"]), request).await;

        assert_that!(response.status, eq(StatusCode::OK));
        assert_that!(
            response.header(header::ACCESS_CONTROL_ALLOW_ORIGIN),
            some(eq("*"))
        );
    }

    #[actix_web::test]
    async fn rejects_malformed_origins_in_config() {
        for origin in [
            "example.com",
            "https://example.com/",
            "https://example.com/path",
            "ftp://example.com",
            "https://",
            "",
        ] {
            let result = AllowedOrigins::parse(&settings(&[origin]));

            assert_that!(result, err(matches_pattern!(EmailFormError::Config(_))));
        }
    }

    #[actix_web::test]
    async fn accepts_origins_with_port() {
        let result =
            AllowedOrigins::parse(&settings(&["http://localhost:3000", "https://example.com"]));

        assert_that!(result, ok(anything()));
    }

    #[actix_web::test]
    async fn rejects_empty_origin_list() {
        let result = AllowedOrigins::parse(&settings(&[]));

        assert_that!(result, err(matches_pattern!(EmailFormError::Config(_))));
    }
}

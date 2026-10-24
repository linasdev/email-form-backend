use actix_web::{HttpResponse, post, web};
use serde::Deserialize;
use validator::Validate;

use crate::error::EmailFormError;
use crate::settings::Settings;

#[derive(Debug, Deserialize, Validate)]
pub struct FormSubmission {
    #[validate(length(min = 1, max = 100))]
    pub name: String,
    #[validate(email, length(max = 256))]
    pub email: String,
    #[validate(length(min = 1))]
    pub message: String,
    #[validate(length(min = 1, max = 4096))]
    pub recaptcha_token: String,
}

#[post("/submit")]
pub async fn submit(
    settings: web::Data<Settings>,
    form: web::Json<FormSubmission>,
) -> Result<HttpResponse, EmailFormError> {
    form.validate()?;

    if form.message.chars().count() > settings.max_message_length {
        return Err(EmailFormError::Validation(format!(
            "message must be at most {} characters",
            settings.max_message_length
        )));
    }

    Ok(HttpResponse::Ok().finish())
}

#[cfg(test)]
mod tests {
    use actix_web::{App, http::StatusCode, test};
    use googletest::prelude::*;
    use serde_json::{Value, json};

    use super::*;

    fn settings(max_message_length: usize) -> Settings {
        Settings {
            host: "127.0.0.1".to_string(),
            port: 8080,
            max_message_length,
        }
    }

    fn valid_payload() -> Value {
        json!({
            "name": "Linas",
            "email": "linas@example.com",
            "message": "Hello!",
            "recaptcha_token": "token",
        })
    }

    async fn post(settings: Settings, payload: Value) -> (StatusCode, String) {
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(settings))
                .service(submit),
        )
        .await;

        let request = test::TestRequest::post()
            .uri("/submit")
            .set_json(payload)
            .to_request();
        let response = test::call_service(&app, request).await;
        let status = response.status();
        let body = test::read_body(response).await;

        (status, String::from_utf8(body.to_vec()).unwrap())
    }

    #[actix_web::test]
    async fn accepts_valid_submission() {
        let (status, _) = post(settings(5000), valid_payload()).await;

        assert_that!(status, eq(StatusCode::OK));
    }

    #[actix_web::test]
    async fn rejects_missing_field() {
        let mut payload = valid_payload();
        payload.as_object_mut().unwrap().remove("email");

        let (status, body) = post(settings(5000), payload).await;

        assert_that!(status, eq(StatusCode::BAD_REQUEST));
        assert_that!(body, contains_substring("missing field `email`"));
    }

    #[actix_web::test]
    async fn rejects_empty_fields() {
        for field in ["name", "message", "recaptcha_token"] {
            let mut payload = valid_payload();
            payload[field] = json!("");

            let (status, body) = post(settings(5000), payload).await;

            assert_that!(status, eq(StatusCode::BAD_REQUEST));
            assert_that!(body, eq(&format!("invalid fields: {field}")));
        }
    }

    #[actix_web::test]
    async fn rejects_invalid_email() {
        let mut payload = valid_payload();
        payload["email"] = json!("not-an-email");

        let (status, body) = post(settings(5000), payload).await;

        assert_that!(status, eq(StatusCode::BAD_REQUEST));
        assert_that!(body, eq("invalid fields: email"));
    }

    #[actix_web::test]
    async fn reports_all_invalid_fields_sorted() {
        let mut payload = valid_payload();
        payload["name"] = json!("");
        payload["email"] = json!("nope");

        let (status, body) = post(settings(5000), payload).await;

        assert_that!(status, eq(StatusCode::BAD_REQUEST));
        assert_that!(body, eq("invalid fields: email, name"));
    }

    #[actix_web::test]
    async fn enforces_fixed_length_limits() {
        let cases = [
            ("name", "a".repeat(101)),
            ("email", format!("{}@example.com", "a".repeat(250))),
            ("recaptcha_token", "a".repeat(4097)),
        ];

        for (field, value) in cases {
            let mut payload = valid_payload();
            payload[field] = json!(value);

            let (status, body) = post(settings(5000), payload).await;

            assert_that!(status, eq(StatusCode::BAD_REQUEST));
            assert_that!(body, eq(&format!("invalid fields: {field}")));
        }
    }

    #[actix_web::test]
    async fn accepts_fields_at_length_limits() {
        let mut payload = valid_payload();
        payload["name"] = json!("a".repeat(100));
        payload["recaptcha_token"] = json!("a".repeat(4096));

        let (status, _) = post(settings(5000), payload).await;

        assert_that!(status, eq(StatusCode::OK));
    }

    #[actix_web::test]
    async fn accepts_message_at_configured_limit() {
        let mut payload = valid_payload();
        payload["message"] = json!("a".repeat(10));

        let (status, _) = post(settings(10), payload).await;

        assert_that!(status, eq(StatusCode::OK));
    }

    #[actix_web::test]
    async fn rejects_message_over_configured_limit() {
        let mut payload = valid_payload();
        payload["message"] = json!("a".repeat(11));

        let (status, body) = post(settings(10), payload).await;

        assert_that!(status, eq(StatusCode::BAD_REQUEST));
        assert_that!(body, eq("message must be at most 10 characters"));
    }

    #[actix_web::test]
    async fn counts_message_length_in_characters() {
        let mut payload = valid_payload();
        payload["message"] = json!("ą".repeat(10));

        let (status, _) = post(settings(10), payload).await;

        assert_that!(status, eq(StatusCode::OK));
    }
}

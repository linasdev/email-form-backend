use actix_web::{HttpResponse, post, web};
use serde::Deserialize;
use validator::Validate;

use crate::error::EmailFormError;
use crate::recaptcha;
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
    client: web::Data<reqwest::Client>,
    form: web::Json<FormSubmission>,
) -> Result<HttpResponse, EmailFormError> {
    form.validate()?;

    if form.message.chars().count() > settings.max_message_length {
        return Err(EmailFormError::Validation(format!(
            "message must be at most {} characters",
            settings.max_message_length
        )));
    }

    recaptcha::verify(&client, &settings, &form.recaptcha_token).await?;

    Ok(HttpResponse::Ok().finish())
}

#[cfg(test)]
mod tests {
    use actix_web::{App, http::StatusCode, test};
    use googletest::prelude::*;
    use serde_json::{Value, json};

    use super::*;
    use crate::recaptcha::test_support::*;

    fn settings(max_message_length: usize, recaptcha_verify_url: String) -> Settings {
        Settings {
            host: "127.0.0.1".to_string(),
            port: 8080,
            max_message_length,
            recaptcha_secret: SECRET.to_string(),
            recaptcha_verify_url,
            recaptcha_min_score: None,
            recaptcha_expected_action: None,
            recaptcha_expected_hostname: None,
        }
    }

    fn valid_payload() -> Value {
        json!({
            "name": "Linas",
            "email": "linas@example.com",
            "message": "Hello!",
            "recaptcha_token": VALID_TOKEN,
        })
    }

    async fn post(max_message_length: usize, payload: Value) -> (StatusCode, String) {
        let url = spawn_mock(json!({ "success": true }));

        post_to(settings(max_message_length, url), payload).await
    }

    async fn post_to(settings: Settings, payload: Value) -> (StatusCode, String) {
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(settings))
                .app_data(web::Data::new(reqwest::Client::new()))
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
        let (status, _) = post(5000, valid_payload()).await;

        assert_that!(status, eq(StatusCode::OK));
    }

    #[actix_web::test]
    async fn rejects_missing_field() {
        let mut payload = valid_payload();
        payload.as_object_mut().unwrap().remove("email");

        let (status, body) = post(5000, payload).await;

        assert_that!(status, eq(StatusCode::BAD_REQUEST));
        assert_that!(body, contains_substring("missing field `email`"));
    }

    #[actix_web::test]
    async fn rejects_empty_fields() {
        for field in ["name", "message", "recaptcha_token"] {
            let mut payload = valid_payload();
            payload[field] = json!("");

            let (status, body) = post(5000, payload).await;

            assert_that!(status, eq(StatusCode::BAD_REQUEST));
            assert_that!(body, eq(&format!("invalid fields: {field}")));
        }
    }

    #[actix_web::test]
    async fn rejects_invalid_email() {
        let mut payload = valid_payload();
        payload["email"] = json!("not-an-email");

        let (status, body) = post(5000, payload).await;

        assert_that!(status, eq(StatusCode::BAD_REQUEST));
        assert_that!(body, eq("invalid fields: email"));
    }

    #[actix_web::test]
    async fn reports_all_invalid_fields_sorted() {
        let mut payload = valid_payload();
        payload["name"] = json!("");
        payload["email"] = json!("nope");

        let (status, body) = post(5000, payload).await;

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

            let (status, body) = post(5000, payload).await;

            assert_that!(status, eq(StatusCode::BAD_REQUEST));
            assert_that!(body, eq(&format!("invalid fields: {field}")));
        }
    }

    #[actix_web::test]
    async fn accepts_fields_at_length_limits() {
        let mut payload = valid_payload();
        payload["name"] = json!("a".repeat(100));
        payload["recaptcha_token"] = json!("a".repeat(4096));

        let (status, _) = post(5000, payload).await;

        assert_that!(status, eq(StatusCode::OK));
    }

    #[actix_web::test]
    async fn accepts_message_at_configured_limit() {
        let mut payload = valid_payload();
        payload["message"] = json!("a".repeat(10));

        let (status, _) = post(10, payload).await;

        assert_that!(status, eq(StatusCode::OK));
    }

    #[actix_web::test]
    async fn rejects_message_over_configured_limit() {
        let mut payload = valid_payload();
        payload["message"] = json!("a".repeat(11));

        let (status, body) = post(10, payload).await;

        assert_that!(status, eq(StatusCode::BAD_REQUEST));
        assert_that!(body, eq("message must be at most 10 characters"));
    }

    #[actix_web::test]
    async fn counts_message_length_in_characters() {
        let mut payload = valid_payload();
        payload["message"] = json!("ą".repeat(10));

        let (status, _) = post(10, payload).await;

        assert_that!(status, eq(StatusCode::OK));
    }

    #[actix_web::test]
    async fn rejects_token_failing_recaptcha_verification() {
        let mut payload = valid_payload();
        payload["recaptcha_token"] = json!(INVALID_TOKEN);

        let (status, body) = post(5000, payload).await;

        assert_that!(status, eq(StatusCode::BAD_REQUEST));
        assert_that!(
            body,
            eq("recaptcha verification failed: invalid-input-response")
        );
    }

    #[actix_web::test]
    async fn returns_bad_gateway_when_recaptcha_is_unreachable() {
        let settings = settings(5000, "http://127.0.0.1:1/siteverify".to_string());

        let (status, _) = post_to(settings, valid_payload()).await;

        assert_that!(status, eq(StatusCode::BAD_GATEWAY));
    }
}

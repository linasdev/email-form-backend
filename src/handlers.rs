use actix_web::{HttpResponse, post, web};
use serde::Deserialize;
use validator::Validate;

use crate::error::EmailFormError;
use crate::mailer::Mailer;
use crate::recaptcha;
use crate::settings::{FormSettings, RecaptchaSettings};

#[derive(Debug, Deserialize, Validate)]
pub struct FormSubmission {
    #[validate(length(min = 1, max = 100))]
    pub name: String,
    #[validate(email, length(max = 256))]
    pub email: String,
    #[validate(length(min = 1))]
    pub message: String,
    #[validate(length(min = 1, max = 4096))]
    pub recaptcha_token: Option<String>,
}

#[post("/submit")]
pub async fn submit(
    form_settings: web::Data<FormSettings>,
    recaptcha_settings: Option<web::Data<RecaptchaSettings>>,
    client: web::Data<reqwest::Client>,
    mailer: web::Data<Mailer>,
    form: web::Json<FormSubmission>,
) -> Result<HttpResponse, EmailFormError> {
    form.validate()?;

    if form.message.chars().count() > form_settings.max_message_length {
        return Err(EmailFormError::Validation(format!(
            "message must be at most {} characters",
            form_settings.max_message_length
        )));
    }

    if let Some(recaptcha_settings) = &recaptcha_settings {
        let token = form
            .recaptcha_token
            .as_deref()
            .ok_or_else(|| EmailFormError::Validation("recaptcha_token is required".to_string()))?;

        recaptcha::verify(&client, recaptcha_settings, token).await?;
    }

    mailer.send(&form).await?;
    log::info!("submission delivered");

    Ok(HttpResponse::Ok().finish())
}

#[cfg(test)]
mod tests {
    use actix_web::{App, http::StatusCode, test};
    use googletest::prelude::*;
    use lettre::transport::stub::AsyncStubTransport;
    use serde_json::{Value, json};

    use super::*;
    use crate::recaptcha::test_support::*;
    use crate::settings::Settings;

    fn settings(max_message_length: usize, recaptcha_verify_url: String) -> Settings {
        Settings {
            form: FormSettings { max_message_length },
            recaptcha: Some(RecaptchaSettings {
                verify_url: recaptcha_verify_url,
                ..RecaptchaSettings::test_default()
            }),
            ..Settings::test_default()
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

        post_to(
            settings(max_message_length, url),
            AsyncStubTransport::new_ok(),
            payload,
        )
        .await
    }

    async fn post_to(
        settings: Settings,
        stub: AsyncStubTransport,
        payload: Value,
    ) -> (StatusCode, String) {
        let mailer = Mailer::with_stub(&settings.smtp, stub).unwrap();
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(settings.form))
                .configure(|cfg| {
                    if let Some(recaptcha) = settings.recaptcha {
                        cfg.app_data(web::Data::new(recaptcha));
                    }
                })
                .app_data(web::Data::new(reqwest::Client::new()))
                .app_data(web::Data::new(mailer))
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

        let (status, _) = post_to(settings, AsyncStubTransport::new_ok(), valid_payload()).await;

        assert_that!(status, eq(StatusCode::BAD_GATEWAY));
    }

    #[actix_web::test]
    async fn sends_email_for_valid_submission() {
        let url = spawn_mock(json!({ "success": true }));
        let stub = AsyncStubTransport::new_ok();

        let (status, _) = post_to(settings(5000, url), stub.clone(), valid_payload()).await;
        let messages = stub.messages().await;

        assert_that!(status, eq(StatusCode::OK));
        assert_that!(messages, len(eq(1)));

        let (envelope, raw) = &messages[0];

        assert_that!(
            envelope.from().map(ToString::to_string),
            some(eq("form@example.com"))
        );
        assert_that!(
            envelope
                .to()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            elements_are![eq("owner@example.com")]
        );
        assert_that!(raw, contains_substring("Subject: New message from Linas"));
        assert_that!(
            raw,
            contains_substring("Reply-To: Linas <linas@example.com>")
        );
        assert_that!(
            raw,
            contains_substring("Name: Linas\r\nEmail: linas@example.com")
        );
        assert_that!(raw, contains_substring("Hello!"));
    }

    #[actix_web::test]
    async fn does_not_send_email_for_invalid_submission() {
        let url = spawn_mock(json!({ "success": true }));
        let stub = AsyncStubTransport::new_ok();
        let mut payload = valid_payload();
        payload["email"] = json!("nope");

        let (status, _) = post_to(settings(5000, url), stub.clone(), payload).await;

        assert_that!(status, eq(StatusCode::BAD_REQUEST));
        assert_that!(stub.messages().await, is_empty());
    }

    #[actix_web::test]
    async fn does_not_send_email_when_recaptcha_fails() {
        let url = spawn_mock(json!({ "success": true }));
        let stub = AsyncStubTransport::new_ok();
        let mut payload = valid_payload();
        payload["recaptcha_token"] = json!(INVALID_TOKEN);

        let (status, _) = post_to(settings(5000, url), stub.clone(), payload).await;

        assert_that!(status, eq(StatusCode::BAD_REQUEST));
        assert_that!(stub.messages().await, is_empty());
    }

    #[actix_web::test]
    async fn returns_bad_gateway_when_smtp_fails() {
        let url = spawn_mock(json!({ "success": true }));
        let stub = AsyncStubTransport::new_error();

        let (status, body) = post_to(settings(5000, url), stub, valid_payload()).await;

        assert_that!(status, eq(StatusCode::BAD_GATEWAY));
        assert_that!(body, eq("Bad Gateway"));
    }

    #[actix_web::test]
    async fn requires_token_when_recaptcha_is_enabled() {
        let mut payload = valid_payload();
        payload.as_object_mut().unwrap().remove("recaptcha_token");

        let (status, body) = post(5000, payload).await;

        assert_that!(status, eq(StatusCode::BAD_REQUEST));
        assert_that!(body, eq("recaptcha_token is required"));
    }

    #[actix_web::test]
    async fn skips_verification_when_recaptcha_is_disabled() {
        let stub = AsyncStubTransport::new_ok();
        let settings = Settings {
            recaptcha: None,
            ..settings(5000, "http://127.0.0.1:1/siteverify".to_string())
        };
        let mut payload = valid_payload();
        payload.as_object_mut().unwrap().remove("recaptcha_token");

        let (status, _) = post_to(settings, stub.clone(), payload).await;

        assert_that!(status, eq(StatusCode::OK));
        assert_that!(stub.messages().await, len(eq(1)));
    }

    #[actix_web::test]
    async fn ignores_token_when_recaptcha_is_disabled() {
        let settings = Settings {
            recaptcha: None,
            ..settings(5000, "http://127.0.0.1:1/siteverify".to_string())
        };
        let mut payload = valid_payload();
        payload["recaptcha_token"] = json!(INVALID_TOKEN);

        let (status, _) = post_to(settings, AsyncStubTransport::new_ok(), payload).await;

        assert_that!(status, eq(StatusCode::OK));
    }

    #[actix_web::test]
    async fn still_validates_other_fields_when_recaptcha_is_disabled() {
        let settings = Settings {
            recaptcha: None,
            ..settings(5000, String::new())
        };
        let mut payload = valid_payload();
        payload["email"] = json!("nope");

        let (status, _) = post_to(settings, AsyncStubTransport::new_ok(), payload).await;

        assert_that!(status, eq(StatusCode::BAD_REQUEST));
    }
}

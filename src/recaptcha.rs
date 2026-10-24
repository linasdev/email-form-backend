use serde::Deserialize;

use crate::error::EmailFormError;
use crate::settings::RecaptchaSettings;

#[derive(Debug, Deserialize)]
struct VerifyResponse {
    success: bool,
    score: Option<f32>,
    action: Option<String>,
    hostname: Option<String>,
    #[serde(default, rename = "error-codes")]
    error_codes: Vec<String>,
}

pub async fn verify(
    client: &reqwest::Client,
    settings: &RecaptchaSettings,
    token: &str,
) -> Result<(), EmailFormError> {
    let response: VerifyResponse = client
        .post(&settings.verify_url)
        .form(&[("secret", settings.secret.as_str()), ("response", token)])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    log::debug!(
        "recaptcha response: success={}, score={:?}, action={:?}, hostname={:?}",
        response.success,
        response.score,
        response.action,
        response.hostname
    );

    if !response.success {
        return Err(EmailFormError::Recaptcha(response.error_codes.join(", ")));
    }

    if let Some(min_score) = settings.min_score {
        let score = response.score.unwrap_or(0.0);

        if score < min_score {
            return Err(EmailFormError::Recaptcha("score too low".to_string()));
        }
    }

    if let Some(expected) = &settings.expected_action
        && response.action.as_ref() != Some(expected)
    {
        return Err(EmailFormError::Recaptcha("unexpected action".to_string()));
    }

    if let Some(expected) = &settings.expected_hostname
        && response.hostname.as_ref() != Some(expected)
    {
        return Err(EmailFormError::Recaptcha("unexpected hostname".to_string()));
    }

    Ok(())
}

#[cfg(test)]
pub mod test_support {
    use std::collections::HashMap;

    use actix_web::{App, HttpResponse, HttpServer, web};
    use serde_json::{Value, json};

    pub const SECRET: &str = "test-secret";
    pub const VALID_TOKEN: &str = "valid-token";
    pub const INVALID_TOKEN: &str = "bad-token";

    async fn siteverify(
        form: web::Form<HashMap<String, String>>,
        result: web::Data<Value>,
    ) -> HttpResponse {
        let credentials_match = form.get("secret").map(String::as_str) == Some(SECRET)
            && form.get("response").map(String::as_str) != Some(INVALID_TOKEN);

        if credentials_match {
            HttpResponse::Ok().json(result.get_ref())
        } else {
            HttpResponse::Ok().json(json!({
                "success": false,
                "error-codes": ["invalid-input-response"],
            }))
        }
    }

    pub fn spawn_mock(result: Value) -> String {
        let result = web::Data::new(result);
        let server = HttpServer::new(move || {
            App::new()
                .app_data(result.clone())
                .route("/siteverify", web::post().to(siteverify))
        })
        .workers(1)
        .bind(("127.0.0.1", 0))
        .unwrap();
        let port = server.addrs()[0].port();

        actix_web::rt::spawn(server.run());

        format!("http://127.0.0.1:{port}/siteverify")
    }
}

#[cfg(test)]
mod tests {
    use googletest::prelude::*;
    use serde_json::json;

    use super::test_support::*;
    use super::*;

    fn settings(url: String, min_score: Option<f32>) -> RecaptchaSettings {
        RecaptchaSettings {
            verify_url: url,
            min_score,
            ..RecaptchaSettings::test_default()
        }
    }

    #[actix_web::test]
    async fn accepts_successful_verification() {
        let url = spawn_mock(json!({ "success": true }));

        let result = verify(&reqwest::Client::new(), &settings(url, None), VALID_TOKEN).await;

        assert_that!(result, ok(anything()));
    }

    #[actix_web::test]
    async fn rejects_unsuccessful_verification_with_error_codes() {
        let url = spawn_mock(json!({ "success": true }));

        let result = verify(&reqwest::Client::new(), &settings(url, None), INVALID_TOKEN).await;

        assert_that!(
            result.unwrap_err().to_string(),
            eq("recaptcha verification failed: invalid-input-response")
        );
    }

    #[actix_web::test]
    async fn rejects_score_below_minimum() {
        let url = spawn_mock(json!({ "success": true, "score": 0.3 }));

        let result = verify(
            &reqwest::Client::new(),
            &settings(url, Some(0.5)),
            VALID_TOKEN,
        )
        .await;

        assert_that!(
            result.unwrap_err().to_string(),
            eq("recaptcha verification failed: score too low")
        );
    }

    #[actix_web::test]
    async fn accepts_score_at_minimum() {
        let url = spawn_mock(json!({ "success": true, "score": 0.5 }));

        let result = verify(
            &reqwest::Client::new(),
            &settings(url, Some(0.5)),
            VALID_TOKEN,
        )
        .await;

        assert_that!(result, ok(anything()));
    }

    #[actix_web::test]
    async fn rejects_missing_score_when_minimum_is_set() {
        let url = spawn_mock(json!({ "success": true }));

        let result = verify(
            &reqwest::Client::new(),
            &settings(url, Some(0.5)),
            VALID_TOKEN,
        )
        .await;

        assert_that!(result, err(anything()));
    }

    #[actix_web::test]
    async fn ignores_score_when_no_minimum_is_set() {
        let url = spawn_mock(json!({ "success": true, "score": 0.0 }));

        let result = verify(&reqwest::Client::new(), &settings(url, None), VALID_TOKEN).await;

        assert_that!(result, ok(anything()));
    }

    #[actix_web::test]
    async fn fails_when_verification_service_is_unreachable() {
        let url = "http://127.0.0.1:1/siteverify".to_string();

        let result = verify(&reqwest::Client::new(), &settings(url, None), VALID_TOKEN).await;

        assert_that!(result, err(matches_pattern!(EmailFormError::Http(_))));
    }

    #[actix_web::test]
    async fn accepts_matching_action_and_hostname() {
        let url = spawn_mock(json!({
            "success": true,
            "action": "contact",
            "hostname": "example.com",
        }));
        let mut settings = settings(url, None);
        settings.expected_action = Some("contact".to_string());
        settings.expected_hostname = Some("example.com".to_string());

        let result = verify(&reqwest::Client::new(), &settings, VALID_TOKEN).await;

        assert_that!(result, ok(anything()));
    }

    #[actix_web::test]
    async fn rejects_mismatched_action() {
        let url = spawn_mock(json!({ "success": true, "action": "login" }));
        let mut settings = settings(url, None);
        settings.expected_action = Some("contact".to_string());

        let result = verify(&reqwest::Client::new(), &settings, VALID_TOKEN).await;

        assert_that!(
            result.unwrap_err().to_string(),
            eq("recaptcha verification failed: unexpected action")
        );
    }

    #[actix_web::test]
    async fn rejects_missing_action_when_expected() {
        let url = spawn_mock(json!({ "success": true }));
        let mut settings = settings(url, None);
        settings.expected_action = Some("contact".to_string());

        let result = verify(&reqwest::Client::new(), &settings, VALID_TOKEN).await;

        assert_that!(result, err(anything()));
    }

    #[actix_web::test]
    async fn rejects_mismatched_hostname() {
        let url = spawn_mock(json!({ "success": true, "hostname": "evil.com" }));
        let mut settings = settings(url, None);
        settings.expected_hostname = Some("example.com".to_string());

        let result = verify(&reqwest::Client::new(), &settings, VALID_TOKEN).await;

        assert_that!(
            result.unwrap_err().to_string(),
            eq("recaptcha verification failed: unexpected hostname")
        );
    }

    #[actix_web::test]
    async fn rejects_missing_hostname_when_expected() {
        let url = spawn_mock(json!({ "success": true }));
        let mut settings = settings(url, None);
        settings.expected_hostname = Some("example.com".to_string());

        let result = verify(&reqwest::Client::new(), &settings, VALID_TOKEN).await;

        assert_that!(result, err(anything()));
    }

    #[actix_web::test]
    async fn ignores_action_and_hostname_when_not_expected() {
        let url = spawn_mock(json!({
            "success": true,
            "action": "anything",
            "hostname": "anywhere.com",
        }));

        let result = verify(&reqwest::Client::new(), &settings(url, None), VALID_TOKEN).await;

        assert_that!(result, ok(anything()));
    }
}

use config::{Config, Environment, File};
use serde::Deserialize;

use crate::error::EmailFormError;

#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
    pub host: String,
    pub port: u16,
    pub max_message_length: usize,
    pub recaptcha_secret: String,
    pub recaptcha_verify_url: String,
    pub recaptcha_min_score: Option<f32>,
    pub recaptcha_expected_action: Option<String>,
    pub recaptcha_expected_hostname: Option<String>,
}

impl Settings {
    pub fn load() -> Result<Self, EmailFormError> {
        let settings = Config::builder()
            .set_default("host", "127.0.0.1")?
            .set_default("port", 8080)?
            .set_default("max_message_length", 4096)?
            .set_default(
                "recaptcha_verify_url",
                "https://www.google.com/recaptcha/api/siteverify",
            )?
            .add_source(File::with_name("config").required(false))
            .add_source(Environment::with_prefix("EMAIL_FORM"))
            .build()?
            .try_deserialize()?;

        Ok(settings)
    }
}

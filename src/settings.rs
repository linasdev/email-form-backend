use config::{Config, Environment, File};
use serde::Deserialize;

use crate::error::EmailFormError;

#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
    pub server: ServerSettings,
    pub form: FormSettings,
    pub recaptcha: Option<RecaptchaSettings>,
    pub smtp: SmtpSettings,
    pub cors: Option<CorsSettings>,
    pub rate_limit: Option<RateLimitSettings>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerSettings {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CorsSettings {
    pub allowed_origins: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitSettings {
    #[serde(default = "default_burst_size")]
    pub burst_size: u32,
    #[serde(default = "default_replenish_seconds")]
    pub replenish_seconds: u64,
    #[serde(default)]
    pub trust_proxy_headers: bool,
}

impl Default for RateLimitSettings {
    fn default() -> Self {
        Self {
            burst_size: default_burst_size(),
            replenish_seconds: default_replenish_seconds(),
            trust_proxy_headers: false,
        }
    }
}

fn default_burst_size() -> u32 {
    5
}

fn default_replenish_seconds() -> u64 {
    60
}

#[derive(Debug, Clone, Deserialize)]
pub struct FormSettings {
    pub max_message_length: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RecaptchaSettings {
    pub secret: String,
    #[serde(default = "default_verify_url")]
    pub verify_url: String,
    pub min_score: Option<f32>,
    pub expected_action: Option<String>,
    pub expected_hostname: Option<String>,
}

fn default_verify_url() -> String {
    "https://www.google.com/recaptcha/api/siteverify".to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SmtpSecurity {
    Starttls,
    Tls,
    None,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SmtpSettings {
    pub host: String,
    pub port: Option<u16>,
    pub security: SmtpSecurity,
    pub username: Option<String>,
    pub password: Option<String>,
    pub from: String,
    pub to: String,
}

impl Settings {
    pub fn load() -> Result<Self, EmailFormError> {
        let settings = Config::builder()
            .set_default("server.host", "127.0.0.1")?
            .set_default("server.port", 8080)?
            .set_default("form.max_message_length", 4096)?
            .set_default("smtp.security", "starttls")?
            .add_source(File::with_name("config").required(false))
            .add_source(
                Environment::with_prefix("EMAIL_FORM")
                    .prefix_separator("_")
                    .separator("__")
                    .list_separator(",")
                    .with_list_parse_key("cors.allowed_origins")
                    .try_parsing(true),
            )
            .build()?
            .try_deserialize()?;

        Ok(settings)
    }
}

#[cfg(test)]
impl FormSettings {
    pub fn test_default() -> Self {
        Self {
            max_message_length: 4096,
        }
    }
}

#[cfg(test)]
impl RecaptchaSettings {
    pub fn test_default() -> Self {
        Self {
            secret: crate::recaptcha::test_support::SECRET.to_string(),
            verify_url: String::new(),
            min_score: None,
            expected_action: None,
            expected_hostname: None,
        }
    }
}

#[cfg(test)]
impl SmtpSettings {
    pub fn test_default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: None,
            security: SmtpSecurity::None,
            username: None,
            password: None,
            from: "Contact Form <form@example.com>".to_string(),
            to: "owner@example.com".to_string(),
        }
    }
}

#[cfg(test)]
impl Settings {
    pub fn test_default() -> Self {
        Self {
            server: ServerSettings {
                host: "127.0.0.1".to_string(),
                port: 8080,
            },
            form: FormSettings::test_default(),
            recaptcha: Some(RecaptchaSettings::test_default()),
            smtp: SmtpSettings::test_default(),
            cors: None,
            rate_limit: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use config::FileFormat;
    use googletest::prelude::*;

    use super::*;

    #[actix_web::test]
    async fn deserializes_nested_sections() {
        let toml = r#"
            [server]
            host = "0.0.0.0"
            port = 9000

            [form]
            max_message_length = 100

            [recaptcha]
            secret = "s"
            verify_url = "http://localhost/verify"
            min_score = 0.5
            expected_action = "contact"

            [smtp]
            host = "smtp.example.com"
            port = 465
            security = "tls"
            username = "user"
            password = "pass"
            from = "form@example.com"
            to = "owner@example.com"
        "#;

        let settings: Settings = Config::builder()
            .add_source(File::from_str(toml, FileFormat::Toml))
            .build()
            .unwrap()
            .try_deserialize()
            .unwrap();

        assert_that!(settings.server.host, eq("0.0.0.0"));
        assert_that!(settings.server.port, eq(9000));
        assert_that!(settings.form.max_message_length, eq(100));
        let recaptcha = settings.recaptcha.unwrap();
        assert_that!(recaptcha.min_score, some(eq(0.5)));
        assert_that!(recaptcha.expected_action, some(eq("contact")));
        assert_that!(recaptcha.expected_hostname, none());
        assert_that!(settings.smtp.security, eq(SmtpSecurity::Tls));
        assert_that!(settings.smtp.port, some(eq(465)));
        assert_that!(settings.smtp.username, some(eq("user")));
    }

    #[actix_web::test]
    async fn rejects_unknown_smtp_security() {
        let toml = r#"
            [server]
            host = "h"
            port = 1
            [form]
            max_message_length = 1
            [recaptcha]
            secret = "s"
            verify_url = "u"
            [smtp]
            host = "h"
            security = "ssl"
            from = "a@b.co"
            to = "c@d.co"
        "#;

        let result = Config::builder()
            .add_source(File::from_str(toml, FileFormat::Toml))
            .build()
            .unwrap()
            .try_deserialize::<Settings>();

        assert_that!(result, err(anything()));
    }

    fn minimal_toml(recaptcha: &str) -> String {
        format!(
            r#"
            [server]
            host = "h"
            port = 1
            [form]
            max_message_length = 1
            [smtp]
            host = "h"
            security = "none"
            from = "a@b.co"
            to = "c@d.co"
            {recaptcha}
            "#
        )
    }

    fn parse(toml: &str) -> std::result::Result<Settings, config::ConfigError> {
        Config::builder()
            .add_source(File::from_str(toml, FileFormat::Toml))
            .build()
            .unwrap()
            .try_deserialize()
    }

    #[actix_web::test]
    async fn recaptcha_is_disabled_without_its_section() {
        let settings = parse(&minimal_toml("")).unwrap();

        assert_that!(settings.recaptcha, none());
    }

    #[actix_web::test]
    async fn recaptcha_section_requires_a_secret() {
        let result = parse(&minimal_toml("[recaptcha]\nmin_score = 0.5"));

        assert_that!(result, err(anything()));
    }

    #[actix_web::test]
    async fn recaptcha_verify_url_defaults_to_google() {
        let settings = parse(&minimal_toml("[recaptcha]\nsecret = \"s\"")).unwrap();

        assert_that!(
            settings.recaptcha.unwrap().verify_url,
            eq("https://www.google.com/recaptcha/api/siteverify")
        );
    }

    #[actix_web::test]
    async fn cors_is_disabled_without_its_section() {
        let settings = parse(&minimal_toml("")).unwrap();

        assert_that!(settings.cors, none());
    }

    #[actix_web::test]
    async fn cors_section_requires_allowed_origins() {
        let result = parse(&minimal_toml("[cors]"));

        assert_that!(result, err(anything()));
    }

    #[actix_web::test]
    async fn cors_section_lists_allowed_origins() {
        let toml = minimal_toml("[cors]\nallowed_origins = [\"https://a.com\", \"https://b.com\"]");

        let settings = parse(&toml).unwrap();

        assert_that!(
            settings.cors.unwrap().allowed_origins,
            elements_are![eq("https://a.com"), eq("https://b.com")]
        );
    }

    #[actix_web::test]
    async fn rate_limit_is_disabled_without_its_section() {
        let settings = parse(&minimal_toml("")).unwrap();

        assert_that!(settings.rate_limit, none());
    }

    #[actix_web::test]
    async fn rate_limit_section_has_defaults() {
        let settings = parse(&minimal_toml("[rate_limit]")).unwrap();
        let rate_limit = settings.rate_limit.unwrap();

        assert_that!(rate_limit.burst_size, eq(5));
        assert_that!(rate_limit.replenish_seconds, eq(60));
        assert_that!(rate_limit.trust_proxy_headers, eq(false));
    }

    #[actix_web::test]
    async fn rate_limit_section_overrides_defaults() {
        let toml = minimal_toml(
            "[rate_limit]\nburst_size = 2\nreplenish_seconds = 10\ntrust_proxy_headers = true",
        );

        let rate_limit = parse(&toml).unwrap().rate_limit.unwrap();

        assert_that!(rate_limit.burst_size, eq(2));
        assert_that!(rate_limit.replenish_seconds, eq(10));
        assert_that!(rate_limit.trust_proxy_headers, eq(true));
    }

    #[actix_web::test]
    async fn shipped_config_file_is_valid() {
        let settings = parse(include_str!("../config.toml")).unwrap();

        assert_that!(settings.server.port, eq(8080));
        assert_that!(settings.smtp.security, eq(SmtpSecurity::Starttls));
        assert_that!(settings.recaptcha, none());
        assert_that!(settings.cors, none());
        assert_that!(settings.rate_limit, none());
    }

    #[actix_web::test]
    async fn shipped_config_file_is_valid_with_every_section_enabled() {
        let uncommented: String = include_str!("../config.toml")
            .lines()
            .map(|line| {
                let optional = line
                    .strip_prefix("# ")
                    .filter(|rest| rest.starts_with('[') || rest.split_once(" = ").is_some());

                optional.unwrap_or(line)
            })
            .collect::<Vec<_>>()
            .join("\n");

        let settings = parse(&uncommented).unwrap();

        assert_that!(settings.smtp.port, some(eq(587)));
        assert_that!(settings.smtp.username, some(eq("user")));
        assert_that!(settings.recaptcha.unwrap().min_score, some(eq(0.5)));
        assert_that!(settings.cors.unwrap().allowed_origins, len(eq(1)));
        assert_that!(settings.rate_limit.unwrap().burst_size, eq(5));
    }
}

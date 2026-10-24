use config::{Config, Environment, File};
use serde::Deserialize;

use crate::error::EmailFormError;

#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
    pub host: String,
    pub port: u16,
}

impl Settings {
    pub fn load() -> Result<Self, EmailFormError> {
        let settings = Config::builder()
            .set_default("host", "127.0.0.1")?
            .set_default("port", 8080)?
            .add_source(File::with_name("config").required(false))
            .add_source(Environment::with_prefix("EMAIL_FORM"))
            .build()?
            .try_deserialize()?;

        Ok(settings)
    }
}

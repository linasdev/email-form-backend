use lettre::message::{Mailbox, header::ContentType};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Address, AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use crate::error::EmailFormError;
use crate::handlers::FormSubmission;
use crate::settings::{SmtpSecurity, SmtpSettings};

#[derive(Debug)]
enum Transport {
    Smtp(AsyncSmtpTransport<Tokio1Executor>),
    #[cfg(test)]
    Stub(lettre::transport::stub::AsyncStubTransport),
}

#[derive(Debug)]
pub struct Mailer {
    transport: Transport,
    from: Mailbox,
    to: Mailbox,
}

fn config_error(message: String) -> EmailFormError {
    config::ConfigError::Message(message).into()
}

fn parse_mailbox(name: &str, value: &str) -> Result<Mailbox, EmailFormError> {
    value
        .parse()
        .map_err(|err| config_error(format!("invalid {name} `{value}`: {err}")))
}

fn sanitize(value: &str) -> String {
    value.chars().filter(|c| !c.is_control()).collect()
}

impl Mailer {
    pub fn new(settings: &SmtpSettings) -> Result<Self, EmailFormError> {
        let builder = match settings.security {
            SmtpSecurity::Starttls => {
                AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&settings.host)
            }
            SmtpSecurity::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(&settings.host),
            SmtpSecurity::None => Ok(AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(
                &settings.host,
            )),
        }
        .map_err(|err| EmailFormError::Smtp(Box::new(err)))?;

        let builder = match settings.port {
            Some(port) => builder.port(port),
            None => builder,
        };

        let builder = match (&settings.username, &settings.password) {
            (Some(username), Some(password)) => {
                builder.credentials(Credentials::new(username.clone(), password.clone()))
            }
            (None, None) => builder,
            _ => {
                return Err(config_error(
                    "smtp_username and smtp_password must be set together".to_string(),
                ));
            }
        };

        log::info!(
            "smtp configured: host={}, security={:?}, auth={}",
            settings.host,
            settings.security,
            settings.username.is_some()
        );

        Ok(Self {
            transport: Transport::Smtp(builder.build()),
            from: parse_mailbox("smtp_from", &settings.from)?,
            to: parse_mailbox("smtp_to", &settings.to)?,
        })
    }

    #[cfg(test)]
    pub fn with_stub(
        settings: &SmtpSettings,
        stub: lettre::transport::stub::AsyncStubTransport,
    ) -> Result<Self, EmailFormError> {
        Ok(Self {
            transport: Transport::Stub(stub),
            from: parse_mailbox("smtp_from", &settings.from)?,
            to: parse_mailbox("smtp_to", &settings.to)?,
        })
    }

    pub async fn send(&self, submission: &FormSubmission) -> Result<(), EmailFormError> {
        let name = sanitize(&submission.name);
        let address: Address = submission
            .email
            .parse()
            .map_err(|_| EmailFormError::Validation("email is invalid".to_string()))?;

        let message = Message::builder()
            .from(self.from.clone())
            .to(self.to.clone())
            .reply_to(Mailbox::new(Some(name.clone()), address))
            .subject(format!("New message from {name}"))
            .header(ContentType::TEXT_PLAIN)
            .body(format!(
                "Name: {name}\nEmail: {}\n\n{}",
                submission.email, submission.message
            ))?;

        match &self.transport {
            Transport::Smtp(transport) => transport
                .send(message)
                .await
                .map(|_| ())
                .map_err(|err| EmailFormError::Smtp(Box::new(err))),
            #[cfg(test)]
            Transport::Stub(transport) => transport
                .send(message)
                .await
                .map_err(|err| EmailFormError::Smtp(Box::new(err))),
        }
    }
}

#[cfg(test)]
mod tests {
    use googletest::prelude::*;
    use lettre::transport::stub::AsyncStubTransport;

    use super::*;

    fn submission(name: &str, email: &str) -> FormSubmission {
        FormSubmission {
            name: name.to_string(),
            email: email.to_string(),
            message: "Hello!".to_string(),
            recaptcha_token: None,
        }
    }

    #[actix_web::test]
    async fn strips_control_characters_from_name() {
        let stub = AsyncStubTransport::new_ok();
        let mailer = Mailer::with_stub(&SmtpSettings::test_default(), stub.clone()).unwrap();

        mailer
            .send(&submission(
                "Evil\r\nBcc: spam@example.com",
                "a@example.com",
            ))
            .await
            .unwrap();
        let messages = stub.messages().await;

        assert_that!(messages[0].1, not(contains_substring("\r\nBcc:")));
        assert_that!(
            messages[0].1,
            contains_substring("Subject: New message from EvilBcc: spam@example.com")
        );
    }

    #[actix_web::test]
    async fn rejects_unparseable_submitter_email() {
        let mailer =
            Mailer::with_stub(&SmtpSettings::test_default(), AsyncStubTransport::new_ok()).unwrap();

        let result = mailer.send(&submission("Linas", "not an email")).await;

        assert_that!(
            result,
            err(matches_pattern!(EmailFormError::Validation(eq(
                "email is invalid"
            ))))
        );
    }

    #[actix_web::test]
    async fn rejects_invalid_from_address() {
        let settings = SmtpSettings {
            from: "nope".to_string(),
            ..SmtpSettings::test_default()
        };

        let result = Mailer::new(&settings);

        assert_that!(result, err(matches_pattern!(EmailFormError::Config(_))));
    }

    #[actix_web::test]
    async fn rejects_invalid_to_address() {
        let settings = SmtpSettings {
            to: "nope".to_string(),
            ..SmtpSettings::test_default()
        };

        let result = Mailer::new(&settings);

        assert_that!(result, err(matches_pattern!(EmailFormError::Config(_))));
    }

    #[actix_web::test]
    async fn rejects_username_without_password() {
        let settings = SmtpSettings {
            username: Some("user".to_string()),
            ..SmtpSettings::test_default()
        };

        let result = Mailer::new(&settings);

        assert_that!(result, err(matches_pattern!(EmailFormError::Config(_))));
    }

    #[actix_web::test]
    async fn builds_for_every_security_mode() {
        for security in [
            SmtpSecurity::Starttls,
            SmtpSecurity::Tls,
            SmtpSecurity::None,
        ] {
            let settings = SmtpSettings {
                security,
                port: Some(2525),
                username: Some("user".to_string()),
                password: Some("pass".to_string()),
                ..SmtpSettings::test_default()
            };

            assert_that!(Mailer::new(&settings), ok(anything()));
        }
    }
}

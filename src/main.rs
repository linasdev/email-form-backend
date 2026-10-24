use actix_web::middleware::Logger;
use actix_web::{App, HttpServer, web};

mod error;
mod handlers;
mod mailer;
mod recaptcha;
mod settings;

use error::EmailFormError;
use mailer::Mailer;
use settings::Settings;

#[actix_web::main]
async fn main() -> Result<(), EmailFormError> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let settings = Settings::load()?;
    let form_settings = web::Data::new(settings.form.clone());
    let recaptcha_settings = settings.recaptcha.clone().map(web::Data::new);

    if recaptcha_settings.is_none() {
        log::warn!("recaptcha is not configured; submissions will not be verified");
    }

    let mailer = web::Data::new(Mailer::new(&settings.smtp)?);
    let client = web::Data::new(reqwest::Client::new());
    let bind_address = (settings.server.host.clone(), settings.server.port);

    log::info!("listening on {}:{}", bind_address.0, bind_address.1);

    HttpServer::new(move || {
        App::new()
            .wrap(Logger::default())
            .app_data(form_settings.clone())
            .configure(|cfg| {
                if let Some(recaptcha_settings) = &recaptcha_settings {
                    cfg.app_data(recaptcha_settings.clone());
                }
            })
            .app_data(client.clone())
            .app_data(mailer.clone())
            .service(handlers::submit)
    })
    .bind(bind_address)?
    .run()
    .await?;

    Ok(())
}

use actix_web::{App, HttpServer, web};

mod error;
mod handlers;
mod recaptcha;
mod settings;

use error::EmailFormError;
use settings::Settings;

#[actix_web::main]
async fn main() -> Result<(), EmailFormError> {
    let settings = Settings::load()?;
    let client = web::Data::new(reqwest::Client::new());
    let bind_address = (settings.host.clone(), settings.port);

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(settings.clone()))
            .app_data(client.clone())
            .service(handlers::submit)
    })
    .bind(bind_address)?
    .run()
    .await?;

    Ok(())
}

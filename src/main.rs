use actix_web::{App, HttpServer, web};

mod error;
mod settings;

use error::EmailFormError;
use settings::Settings;

#[actix_web::main]
async fn main() -> Result<(), EmailFormError> {
    let settings = Settings::load()?;
    let bind_address = (settings.host.clone(), settings.port);

    HttpServer::new(move || App::new().app_data(web::Data::new(settings.clone())))
        .bind(bind_address)?
        .run()
        .await?;

    Ok(())
}

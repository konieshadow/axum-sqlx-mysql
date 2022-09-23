use anyhow::Context;
use axum::{AddExtensionLayer, Router};
use sqlx::MySqlPool;
use std::sync::Arc;
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;

use crate::config::Config;

mod error;
mod extractor;
mod users;
mod profiles;
mod articles;
mod types;

pub use error::{Error, ResultExt};

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Clone)]
struct ApiContext {
    config: Arc<Config>,
    db: MySqlPool,
}

pub async fn serve(config: Config, db: MySqlPool) -> anyhow::Result<()> {
    let app = api_router().layer(
        ServiceBuilder::new()
            .layer(AddExtensionLayer::new(ApiContext {
                config: Arc::new(config),
                db,
            }))
            .layer(TraceLayer::new_for_http()),
    );

    axum::Server::bind(&"0.0.0.0:8080".parse()?)
        .serve(app.into_make_service())
        .await
        .context("error running HTTP server")
}

fn api_router() -> Router {
    users::router()
        .merge(profiles::router())
        .merge(articles::router())
}

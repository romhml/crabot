use std::net::SocketAddr;

use axum::{extract::MatchedPath, http::Request, Router};

use router::index::index_router;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tower_livereload::LiveReloadLayer;

use tracing::info_span;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use dotenv::dotenv;

mod model;
mod router;
mod template;

// TODO:
// - Update favicon.
// - Update documentation.
// - Implement support for real world models (e.g. with candle).
fn create_app() -> Router {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                // axum logs rejections from built-in extractors with the `axum::rejection`
                // target, at `TRACE` level. `axum::rejection=trace` enables showing those events
                "example_tracing_aka_logging=debug,tower_http=debug,axum::rejection=trace".into()
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let trace_layer = TraceLayer::new_for_http().make_span_with(|request: &Request<_>| {
        // Log the matched route's path (with placeholders not filled in).
        // Use request.uri() or OriginalUri if you want the real path.
        let matched_path = request
            .extensions()
            .get::<MatchedPath>()
            .map(MatchedPath::as_str);

        info_span!(
            "http_request",
            method = ?request.method(),
            matched_path,
            some_other_field = tracing::field::Empty,
        )
    });

    let assets_path = std::env::current_dir().unwrap();
    // build our application with a route

    Router::new()
        .merge(index_router())
        .nest_service(
            "/assets",
            ServeDir::new(format!("{}/assets", assets_path.to_str().unwrap())),
        )
        .fallback_service(ServeDir::new(format!(
            "{}/public",
            assets_path.to_str().unwrap()
        )))
        .layer(trace_layer)
}

#[tokio::main]
async fn main() {
    dotenv().ok();
    // TODO: Disable live reload on production
    let app = create_app().layer(LiveReloadLayer::new());

    // Run our app with hyper, listening globally on port 3000
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    tracing::debug!("Listening on {}", listener.local_addr().unwrap());

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
}

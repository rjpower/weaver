use std::path::PathBuf;

use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

use crate::db::Db;

fn static_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("static")
        .join("dist")
}

pub async fn serve(db: Db, listener: TcpListener, cancel: CancellationToken) -> anyhow::Result<()> {
    let app = crate::web::router(db)
        .layer(CorsLayer::permissive())
        .fallback_service(ServeDir::new(static_dir()));

    axum::serve(listener, app)
        .with_graceful_shutdown(cancel.cancelled_owned())
        .await?;

    Ok(())
}

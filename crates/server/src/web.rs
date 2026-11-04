//! Serves the built web UI (`web/dist`), embedded into the binary in release
//! builds. Unknown paths fall back to `index.html` for client-side routing.

use axum::{
    http::{StatusCode, Uri, header},
    response::{Html, IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../web/dist"]
#[allow_missing = true]
struct Assets;

pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    if let Some(file) = Assets::get(path).filter(|_| !path.is_empty()) {
        let mime = if path.ends_with(".webmanifest") {
            "application/manifest+json".to_owned()
        } else {
            mime_guess::from_path(path).first_or_octet_stream().to_string()
        };
        // Vite fingerprints everything under assets/, so it can be cached forever.
        let cache = if path.starts_with("assets/") {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        };
        return (
            [
                (header::CONTENT_TYPE, mime),
                (header::CACHE_CONTROL, cache.to_owned()),
            ],
            file.data,
        )
            .into_response();
    }

    match Assets::get("index.html") {
        Some(index) => (
            [(header::CACHE_CONTROL, "no-cache")],
            Html(index.data),
        )
            .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            "Web UI not built. Run `pnpm build` in web/, or use the Vite dev server.",
        )
            .into_response(),
    }
}

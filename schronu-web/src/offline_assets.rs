//! Describe the same public files that Dioxus serves, including WASM-bindgen's nested imports.
use base64::{engine::general_purpose::STANDARD, Engine};
use dioxus::fullstack::axum::{http::StatusCode, Json};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
pub(crate) struct Asset {
    path: String,
    integrity: String,
}

pub(crate) async fn manifest() -> Result<Json<Vec<Asset>>, StatusCode> {
    tokio::task::spawn_blocking(|| {
        let public = match std::env::var_os("DIOXUS_PUBLIC_PATH") {
            Some(path) => PathBuf::from(path),
            None => std::env::current_exe()?.with_file_name("public"),
        };
        let mut assets = Vec::new();
        collect(&public, &public, &mut assets)?;
        Ok::<_, io::Error>(Json(assets))
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)
}

fn collect(root: &Path, dir: &Path, assets: &mut Vec<Asset>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let path = entry.path();
        if kind.is_dir() {
            collect(root, &path, assets)?;
        } else if kind.is_file()
            && matches!(
                path.extension().and_then(|ext| ext.to_str()),
                Some("wasm" | "js" | "mjs" | "css")
            )
        {
            let name = path
                .strip_prefix(root)
                .ok()
                .and_then(Path::to_str)
                .ok_or_else(|| io::Error::other("invalid public asset path"))?;
            let mut hash = Sha256::new();
            io::copy(&mut fs::File::open(&path)?, &mut hash)?;
            assets.push(Asset {
                path: format!("/{name}"),
                integrity: format!("sha256-{}", STANDARD.encode(hash.finalize())),
            });
        }
    }
    Ok(())
}

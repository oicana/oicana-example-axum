use std::{path::Path, sync::Arc};

use axum::{
    Json,
    extract::{Multipart, State},
    http::StatusCode,
    response::IntoResponse,
};
use dashmap::DashMap;
use serde::Serialize;
use tracing::{error, info, warn};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

const DEFAULT_BLOB_UUID: Uuid = Uuid::nil();

pub type BlobStorage = Arc<DashMap<Uuid, Vec<u8>>>;

pub fn router() -> (OpenApiRouter, BlobStorage) {
    let storage = Arc::new(initialize_blob_storage());

    let router = OpenApiRouter::new()
        .routes(routes!(upload_blob))
        .with_state(storage.clone());

    (router, storage)
}

fn initialize_blob_storage() -> DashMap<Uuid, Vec<u8>> {
    let storage = DashMap::new();

    let path = Path::new("blobs").join(DEFAULT_BLOB_UUID.to_string());
    match std::fs::read(&path) {
        Ok(data) => {
            info!(
                "Loaded default blob (Oicana logo) with UUID {}",
                DEFAULT_BLOB_UUID
            );
            storage.insert(DEFAULT_BLOB_UUID, data);
        }
        Err(e) => {
            error!("Failed to load default blob from {}: {}", path.display(), e);
        }
    }

    storage
}

pub fn get_blob(storage: &DashMap<Uuid, Vec<u8>>, id: Uuid) -> Option<Vec<u8>> {
    if let Some(entry) = storage.get(&id) {
        return Some(entry.value().clone());
    }

    let path = Path::new("blobs").join(id.to_string());
    match std::fs::read(&path) {
        Ok(data) => {
            info!("Loaded blob {} from disk and cached it", id);
            storage.insert(id, data.clone());
            Some(data)
        }
        Err(e) => {
            warn!("Failed to read blob {} from {}: {}", id, path.display(), e);
            None
        }
    }
}

#[derive(Serialize, ToSchema)]
struct UploadResponse {
    /// The UUID assigned to the uploaded blob
    #[schema(example = "550e8400-e29b-41d4-a716-446655440000")]
    id: Uuid,
}

#[derive(ToSchema)]
#[schema(title = "FileUpload")]
#[allow(dead_code)]
struct FileUploadSchema {
    /// The file to upload
    #[schema(value_type = String, format = Binary)]
    file: Vec<u8>,
}

#[utoipa::path(
    method(post),
    tag = super::BLOB_TAG,
    path = "/blobs",
    request_body(content = FileUploadSchema, content_type = "multipart/form-data"),
    description = "Upload a blob (image, file, etc.) to use as template input. Returns a UUID to reference the blob in compilation requests.",
    responses(
        (status = OK, description = "Blob uploaded successfully", body = UploadResponse, content_type = "application/json"),
        (status = BAD_REQUEST, description = "Invalid file upload"),
        (status = INTERNAL_SERVER_ERROR, description = "Failed to save file to disk")
    )
)]
async fn upload_blob(
    State(storage): State<Arc<DashMap<Uuid, Vec<u8>>>>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let mut file_data: Option<Vec<u8>> = None;

    while let Some(field) = multipart.next_field().await.unwrap_or(None) {
        let field_name = field.name().unwrap_or("");

        if field_name == "file" {
            match field.bytes().await {
                Ok(bytes) => {
                    file_data = Some(bytes.to_vec());
                    break;
                }
                Err(e) => {
                    error!("Failed to read file field: {}", e);
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({"error": "Failed to read file"})),
                    )
                        .into_response();
                }
            }
        }
    }

    let data = match file_data {
        Some(data) => data,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "No file field provided"})),
            )
                .into_response();
        }
    };

    let id = Uuid::new_v4();
    let path = Path::new("blobs").join(id.to_string());

    if let Err(e) = std::fs::write(&path, &data) {
        error!("Failed to write blob {} to {}: {}", id, path.display(), e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to save file"})),
        )
            .into_response();
    }

    storage.insert(id, data);
    info!("Stored blob {} to disk and cache", id);

    (StatusCode::OK, Json(UploadResponse { id })).into_response()
}

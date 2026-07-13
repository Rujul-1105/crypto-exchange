//! Crate-wide error type for the API layer.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::auth::Role;
use ledger::LedgerError;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error(transparent)]
    Auth(#[from] crate::auth::AuthError),
    #[error(transparent)]
    Ledger(#[from] LedgerError),
    #[error("symbol not registered: {0}")]
    UnknownSymbol(String),
    #[error("invalid symbol: {0}")]
    InvalidSymbol(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        let body = serde_json::json!({
            "error": self.to_string(),
        });
        (status, Json(body)).into_response()
    }
}

impl ApiError {
    pub fn status(&self) -> StatusCode {
        match self {
            ApiError::Auth(e) => StatusCode::from_u16(e.http_status())
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            ApiError::Ledger(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ApiError::UnknownSymbol(_) => StatusCode::NOT_FOUND,
            ApiError::InvalidSymbol(_) => StatusCode::BAD_REQUEST,
        }
    }
}
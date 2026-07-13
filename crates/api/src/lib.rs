//! HTTP API surface for the exchange.
//!
//! ## Phase 4 — Auth & RBAC
//!
//! Auth primitives (Argon2id password hashing, JWT HS256), role enum,
//! user store, and auth-aware handlers. Every admin-only handler calls
//! `AuthContext::require_role(Role::Admin)` as its first action.
//!
//! ## Phase 5 — REST API & Persistence Integration
//!
//! The `http` module owns the axum router, `AuthContext` extractor,
//! JSON response types, and the binary entry point.
//! `service` provides the `OrderService` that orchestrates
//! ledger-lock → actor-submit → ledger-settle → WAL-append.
//! `wal` provides the JSONL WAL with file persistence and replay.
//! `idempotency` and `ratelimit` are in-memory helpers.

pub mod auth;
pub mod error;
pub mod handlers;
pub mod http;
pub mod idempotency;
pub mod ratelimit;
pub mod service;
pub mod wal;

pub use auth::{
    hash_password, issue_token, verify_password, verify_token, AuthContext, AuthError, InMemoryUserStore,
    PasswordHash, Role, TokenClaims, User, UserStore,
};
pub use error::ApiError;
pub use service::{OrderService, ServiceError, SubmitResult};
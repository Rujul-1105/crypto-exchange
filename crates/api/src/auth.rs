//! Authentication primitives: password hashing (Argon2id), JWT
//! sign/verify (HS256), role enum, in-memory user store.
//!
//! ## Phase 4 scope
//!
//! Pure auth flow. No HTTP wiring — Phase 5 wraps these as axum
//! handlers. The boundary checking happens at `AuthContext::require_role`,
//! which every protected handler calls as its first action.
//!
//! ## Layers
//!
//! - `Role` enum + simple linear hierarchy (Admin > MarketMaker > User).
//! - `PasswordHash` wraps the PHC string from `argon2`. Equality /
//!   display are debug-only (never log the hash).
//! - `User` is a domain type carrying the role and hash; the in-memory
//!   store is the only Phase 4 backend.
//! - `AuthContext` is what an authenticated request carries. In Phase 5
//!   it's an axum extractor; in tests it's constructed from a token.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use argon2::password_hash::{PasswordHash as Argon2PasswordHash, SaltString};
use argon2::{Argon2, PasswordHasher, PasswordVerifier};
use rand_core::OsRng;
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use ledger::UserId;
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("invalid credentials")]
    InvalidCredentials,
    #[error("user not found")]
    UnknownUser,
    #[error("duplicate username")]
    DuplicateUsername,
    #[error("password too short (minimum 8 characters)")]
    PasswordTooShort,
    #[error("empty username")]
    EmptyUsername,
    #[error("invalid token: {0}")]
    InvalidToken(String),
    #[error("token expired")]
    TokenExpired,
    #[error("role {have:?} cannot access resource requiring {need:?}")]
    Forbidden { have: Role, need: Role },
    #[error("unknown order: {0:?}")]
    UnknownOrder(common::OrderId),
    #[error("internal: {0}")]
    Internal(String),
}

impl AuthError {
    /// HTTP-shaped status. Phase 5 will use this to map to a response.
    pub fn http_status(&self) -> u16 {
        match self {
            AuthError::InvalidCredentials | AuthError::UnknownUser => 401,
            AuthError::InvalidToken(_) | AuthError::TokenExpired => 401,
            AuthError::UnknownOrder(_) => 404,
            AuthError::DuplicateUsername => 409,
            AuthError::PasswordTooShort | AuthError::EmptyUsername => 400,
            AuthError::Forbidden { .. } => 403,
            AuthError::Internal(_) => 500,
        }
    }
}

/// Roles form a linear hierarchy for access checks: an admin satisfies
/// any `require_role(R)` for `R` ∈ {User, MarketMaker, Admin}; a
/// market-maker satisfies User and MarketMaker; a user only satisfies User.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Role {
    User,
    MarketMaker,
    Admin,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::User => "user",
            Role::MarketMaker => "market_maker",
            Role::Admin => "admin",
        }
    }

    pub fn parse(s: &str) -> Result<Self, AuthError> {
        match s {
            "user" => Ok(Role::User),
            "market_maker" => Ok(Role::MarketMaker),
            "admin" => Ok(Role::Admin),
            other => Err(AuthError::Internal(format!("unknown role string: {}", other))),
        }
    }

    /// Linear hierarchy check. `self.can_access(R)` returns true iff
    /// `self`'s rank is >= `R`'s rank.
    pub fn can_access(self, required: Role) -> bool {
        let self_rank = match self {
            Role::User => 0,
            Role::MarketMaker => 1,
            Role::Admin => 2,
        };
        let req_rank = match required {
            Role::User => 0,
            Role::MarketMaker => 1,
            Role::Admin => 2,
        };
        self_rank >= req_rank
    }
}

/// Argon2id PHC string. Hidden debug output (don't print hashes in logs).
#[derive(Clone, PartialEq, Eq)]
pub struct PasswordHash(String);

impl std::fmt::Debug for PasswordHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PasswordHash(<redacted>)")
    }
}

/// Hash a password with Argon2id + a fresh random salt. Returns a PHC
/// string suitable for storage.
pub fn hash_password(plain: &str) -> Result<PasswordHash, AuthError> {
    if plain.len() < 8 {
        return Err(AuthError::PasswordTooShort);
    }
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(plain.as_bytes(), &salt)
        .map_err(|e| AuthError::Internal(format!("argon2: {}", e)))?
        .to_string();
    Ok(PasswordHash(hash))
}

/// Verify a password against a stored hash. Constant-time-by-design on
/// the underlying Argon2 compare.
pub fn verify_password(plain: &str, hash: &PasswordHash) -> bool {
    let parsed = match Argon2PasswordHash::new(&hash.0) {
        Ok(p) => p,
        Err(_) => return false,
    };
    Argon2::default()
        .verify_password(plain.as_bytes(), &parsed)
        .is_ok()
}

/// Claims encoded in our JWTs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenClaims {
    /// Subject — the user's numeric id.
    pub sub: u64,
    /// Role as a string (so the token is self-describing).
    pub role: String,
    /// Expiry as a unix-epoch second count.
    pub exp: u64,
}

/// Issue a JWT signed with `secret`. Returns `(token, exp)`.
pub fn issue_token(
    user_id: UserId,
    role: Role,
    secret: &[u8],
    expires_in_secs: u64,
) -> Result<(String, u64), AuthError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| AuthError::Internal(format!("clock: {}", e)))?
        .as_secs();
    let exp = now + expires_in_secs;
    let claims = TokenClaims {
        sub: user_id.0,
        role: role.as_str().to_owned(),
        exp,
    };
    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret),
    )
    .map_err(|e| AuthError::Internal(format!("jwt encode: {}", e)))?;
    Ok((token, exp))
}

/// Verify a JWT and return `(user_id, role)`. 5-second leeway for clock
/// skew between issuer and verifier.
pub fn verify_token(token: &str, secret: &[u8]) -> Result<(UserId, Role), AuthError> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.leeway = 5;
    let data = decode::<TokenClaims>(token, &DecodingKey::from_secret(secret), &validation)
        .map_err(|e| match e.kind() {
            jsonwebtoken::errors::ErrorKind::ExpiredSignature => AuthError::TokenExpired,
            _ => AuthError::InvalidToken(e.to_string()),
        })?;
    let claims = data.claims;
    let user_id = UserId(claims.sub);
    let role = Role::parse(&claims.role)?;
    Ok((user_id, role))
}

/// A user record.
#[derive(Debug, Clone)]
pub struct User {
    pub id: UserId,
    pub username: String,
    pub password_hash: PasswordHash,
    pub role: Role,
}

/// User persistence boundary. Phase 4 ships the in-memory adapter;
/// Phase 5 (or a future phase) adds Postgres.
pub trait UserStore {
    fn create_user(
        &mut self,
        username: &str,
        password: &str,
        role: Role,
    ) -> Result<User, AuthError>;
    fn find_by_username(&self, username: &str) -> Option<User>;
    fn find_by_id(&self, id: UserId) -> Option<User>;
    fn list_users(&self) -> Vec<User>;
}

/// In-memory user store. Suitable for tests + the Phase 5 dev server.
/// Phase 5's prod path will replace this with a Postgres-backed adapter.
#[derive(Debug, Default)]
pub struct InMemoryUserStore {
    by_id: HashMap<UserId, User>,
    by_username: HashMap<String, UserId>,
    next_id: u64,
}

impl InMemoryUserStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed a user with an explicit id (admin-bootstrap helper for
    /// tests + first-run scripts).
    pub fn insert(&mut self, id: UserId, username: &str, password_hash: PasswordHash, role: Role) {
        let user = User {
            id,
            username: username.to_owned(),
            password_hash,
            role,
        };
        self.by_username.insert(username.to_owned(), id);
        self.by_id.insert(id, user);
        if id.0 >= self.next_id {
            self.next_id = id.0 + 1;
        }
    }
}

impl UserStore for InMemoryUserStore {
    fn create_user(
        &mut self,
        username: &str,
        password: &str,
        role: Role,
    ) -> Result<User, AuthError> {
        if username.is_empty() {
            return Err(AuthError::EmptyUsername);
        }
        if self.by_username.contains_key(username) {
            return Err(AuthError::DuplicateUsername);
        }
        let password_hash = hash_password(password)?;
        self.next_id += 1;
        let id = UserId(self.next_id);
        let user = User {
            id,
            username: username.to_owned(),
            password_hash,
            role,
        };
        self.by_username.insert(username.to_owned(), id);
        self.by_id.insert(id, user.clone());
        Ok(user)
    }

    fn find_by_username(&self, username: &str) -> Option<User> {
        self.by_username
            .get(username)
            .and_then(|id| self.by_id.get(id))
            .cloned()
    }

    fn find_by_id(&self, id: UserId) -> Option<User> {
        self.by_id.get(&id).cloned()
    }

    fn list_users(&self) -> Vec<User> {
        self.by_id.values().cloned().collect()
    }
}

/// Authentication context derived from a verified token. In Phase 5 this
/// is an axum extractor; in tests / Phase 4 we construct it directly via
/// `from_token` or `for_tests`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthContext {
    pub user_id: UserId,
    pub role: Role,
}

impl AuthContext {
    /// Build from a raw token + secret. Verifies signature + expiry.
    pub fn from_token(token: &str, secret: &[u8]) -> Result<Self, AuthError> {
        let (user_id, role) = verify_token(token, secret)?;
        Ok(AuthContext { user_id, role })
    }

    /// Test/CLI convenience: skip the token round-trip.
    pub fn for_tests(user_id: UserId, role: Role) -> Self {
        AuthContext { user_id, role }
    }

    /// Single point of role enforcement. Every protected handler MUST
    /// call this as its first action.
    pub fn require_role(&self, required: Role) -> Result<(), AuthError> {
        if self.role.can_access(required) {
            Ok(())
        } else {
            Err(AuthError::Forbidden {
                have: self.role,
                need: required,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_hierarchy() {
        assert!(Role::Admin.can_access(Role::Admin));
        assert!(Role::Admin.can_access(Role::MarketMaker));
        assert!(Role::Admin.can_access(Role::User));
        assert!(!Role::Admin.can_access(Role::User) == false); // tautology; for type sanity

        assert!(Role::MarketMaker.can_access(Role::User));
        assert!(!Role::MarketMaker.can_access(Role::Admin));

        assert!(Role::User.can_access(Role::User));
        assert!(!Role::User.can_access(Role::MarketMaker));
        assert!(!Role::User.can_access(Role::Admin));
    }

    #[test]
    fn role_round_trip_string() {
        for r in [Role::User, Role::MarketMaker, Role::Admin] {
            assert_eq!(Role::parse(r.as_str()).unwrap(), r);
        }
        assert!(Role::parse("banana").is_err());
    }

    #[test]
    fn password_hash_round_trip() {
        let hash = hash_password("correct horse").unwrap();
        assert!(verify_password("correct horse", &hash));
        assert!(!verify_password("wrong horse", &hash));
        assert!(!verify_password("", &hash));
    }

    #[test]
    fn password_hash_redacted_in_debug() {
        let hash = hash_password("hunter22hunter").unwrap();
        let s = format!("{:?}", hash);
        assert!(s.contains("redacted"));
        assert!(!s.contains("hunter22"));
    }

    #[test]
    fn password_too_short() {
        assert!(matches!(
            hash_password("short"),
            Err(AuthError::PasswordTooShort)
        ));
    }
}
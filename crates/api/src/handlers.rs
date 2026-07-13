//! Auth-aware handlers.
//!
//! In Phase 4 these are ordinary functions. Phase 5 will wrap them as
//! axum routes. The boundary check happens at `AuthContext::require_role`
//! — every admin handler starts with this call.

use common::Symbol;
use ledger::{Amount, Asset, Ledger, LedgerError, UserId};

use crate::auth::{verify_password, AuthContext, AuthError, Role, User, UserStore};

/// Register a new user. Public — no `AuthContext` required.
///
/// `role` defaults to `User`; admins can promote via
/// `admin_create_user`. Anyone calling `register` directly can only
/// create `User` or `MarketMaker` accounts; `Admin` requires an existing
/// admin (enforced in `admin_create_user`).
pub fn register<S: UserStore>(
    store: &mut S,
    username: &str,
    password: &str,
    role: Role,
) -> Result<User, AuthError> {
    if role == Role::Admin {
        return Err(AuthError::Internal(
            "self-registration as admin is not permitted; use admin_create_user".into(),
        ));
    }
    store.create_user(username, password, role)
}

/// Login: verify password, return (User, JWT).
pub fn login<S: UserStore>(
    store: &S,
    username: &str,
    password: &str,
    secret: &[u8],
    expires_in_secs: u64,
) -> Result<(User, String), AuthError> {
    let user = store
        .find_by_username(username)
        .ok_or(AuthError::UnknownUser)?;
    if !verify_password(password, &user.password_hash) {
        return Err(AuthError::InvalidCredentials);
    }
    let (token, _exp) =
        crate::auth::issue_token(user.id, user.role, secret, expires_in_secs)?;
    Ok((user, token))
}

/// Admin: create a new user with an arbitrary role (including Admin).
pub fn admin_create_user<S: UserStore>(
    ctx: &AuthContext,
    store: &mut S,
    username: &str,
    password: &str,
    role: Role,
) -> Result<User, AuthError> {
    ctx.require_role(Role::Admin)?;
    store.create_user(username, password, role)
}

/// Admin: manually adjust a user's balance (demo / testing aid).
/// `is_deposit = true` credits available; `false` debits available.
pub fn admin_adjust_balance<L: Ledger, S: UserStore>(
    ctx: &AuthContext,
    store: &S,
    ledger: &mut L,
    target_user: UserId,
    asset: Asset,
    amount: Amount,
    is_deposit: bool,
) -> Result<(), AuthError> {
    ctx.require_role(Role::Admin)?;
    if store.find_by_id(target_user).is_none() {
        return Err(AuthError::UnknownUser);
    }
    if is_deposit {
        ledger
            .deposit(target_user, asset, amount)
            .map_err(ledger_err)?;
    } else {
        ledger
            .withdraw_available(target_user, asset, amount)
            .map_err(ledger_err)?;
    }
    Ok(())
}

/// Admin: register a new trading symbol. Phase 5 will wire this to actor
/// spawning; for Phase 4 it just validates the symbol name.
pub fn admin_register_symbol(
    ctx: &AuthContext,
    name: &str,
) -> Result<Symbol, AuthError> {
    ctx.require_role(Role::Admin)?;
    if name.is_empty() || !name.contains('-') {
        return Err(AuthError::Internal(format!(
            "invalid symbol {:?}: expected BASE-QUOTE format",
            name
        )));
    }
    Ok(Symbol::from(name))
}

/// Convert a ledger error to an auth error. Kept private to handlers.
fn ledger_err(e: LedgerError) -> AuthError {
    AuthError::Internal(format!("ledger: {}", e))
}
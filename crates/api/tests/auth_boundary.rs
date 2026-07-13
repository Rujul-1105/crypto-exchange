//! Role-boundary tests for the API auth layer.
//!
//! CLAUDE.md Phase 4 exit criterion: "auth middleware has tests for
//! each role's access boundaries (403 on unauthorized role, 200 on
//! authorized)."
//!
//! We exercise:
//!   * `User` cannot call admin endpoints → `Forbidden { have: User, need: Admin }`
//!   * `MarketMaker` cannot call admin endpoints → `Forbidden { have: MarketMaker, need: Admin }`
//!   * `Admin` can call admin endpoints → `Ok`
//!   * Token tampering, expiry, wrong-secret cases
//!   * Public endpoints succeed without auth

use api::{
    handlers::{self, admin_adjust_balance, admin_create_user, admin_register_symbol},
    AuthContext, AuthError, InMemoryUserStore, Role, User, UserStore,
};
use common::{Qty, Symbol};
use ledger::{Asset, InMemoryLedger, Ledger, UserId};

const SECRET: &[u8] = b"phase4-test-secret-do-not-use-in-prod";

fn bootstrap() -> (InMemoryUserStore, User, User, User) {
    let mut store = InMemoryUserStore::new();
    let alice = store
        .create_user("alice", "alice-password", Role::User)
        .expect("alice");
    let bob = store
        .create_user("bob", "bob-password", Role::MarketMaker)
        .expect("bob");
    let admin = store
        .create_user("admin1", "admin-password", Role::Admin)
        .expect("admin");
    (store, alice, bob, admin)
}

fn alice_ctx(alice: &User) -> AuthContext {
    AuthContext::for_tests(alice.id, alice.role)
}

fn bob_ctx(bob: &User) -> AuthContext {
    AuthContext::for_tests(bob.id, bob.role)
}

fn admin_ctx(admin: &User) -> AuthContext {
    AuthContext::for_tests(admin.id, admin.role)
}

fn usdc() -> Asset {
    Asset::from("USDC")
}

// ============== Per-role boundary tests (CLAUDE.md exit criterion) ==============

#[test]
fn user_cannot_call_admin_create_user() {
    let (mut store, alice, _bob, _admin) = bootstrap();
    let ctx = alice_ctx(&alice);
    let result = admin_create_user(&ctx, &mut store, "newby", "newby-password", Role::User);
    match result {
        Err(AuthError::Forbidden { have, need }) => {
            assert_eq!(have, Role::User);
            assert_eq!(need, Role::Admin);
        }
        other => panic!("expected Forbidden, got {:?}", other),
    }
    // No side effect: user wasn't created.
    assert!(store.find_by_username("newby").is_none());
}

#[test]
fn market_maker_cannot_call_admin_create_user() {
    let (mut store, _alice, bob, _admin) = bootstrap();
    let ctx = bob_ctx(&bob);
    let result = admin_create_user(&ctx, &mut store, "newby", "newby-password", Role::User);
    match result {
        Err(AuthError::Forbidden { have, need }) => {
            assert_eq!(have, Role::MarketMaker);
            assert_eq!(need, Role::Admin);
        }
        other => panic!("expected Forbidden, got {:?}", other),
    }
}

#[test]
fn admin_can_call_admin_create_user() {
    let (mut store, _alice, _bob, admin) = bootstrap();
    let ctx = admin_ctx(&admin);
    let result = admin_create_user(&ctx, &mut store, "newby", "newby-password", Role::User);
    assert!(matches!(result, Ok(u) if u.username == "newby"));
    let created = store.find_by_username("newby").unwrap();
    assert_eq!(created.role, Role::User);
}

#[test]
fn user_cannot_call_admin_adjust_balance() {
    let (store, alice, _bob, _admin) = bootstrap();
    let mut ledger = InMemoryLedger::new();
    let ctx = alice_ctx(&alice);
    let result = admin_adjust_balance(
        &ctx,
        &store,
        &mut ledger,
        UserId(999),
        usdc(),
        Qty(10),
        true,
    );
    match result {
        Err(AuthError::Forbidden { have, need }) => {
            assert_eq!(have, Role::User);
            assert_eq!(need, Role::Admin);
        }
        other => panic!("expected Forbidden, got {:?}", other),
    }
}

#[test]
fn admin_can_call_admin_adjust_balance() {
    let (mut store, _alice, _bob, admin) = bootstrap();
    let mut ledger = InMemoryLedger::new();
    let ctx = admin_ctx(&admin);

    // Create a target user (admin role).
    let target = store.create_user("target", "target-password", Role::User).unwrap();

    // Admin credits 100 USDC.
    admin_adjust_balance(
        &ctx,
        &store,
        &mut ledger,
        target.id,
        usdc(),
        Qty(100),
        true,
    )
    .unwrap();

    let acct = ledger.account(target.id, usdc());
    assert_eq!(acct.available, 100);
    assert_eq!(acct.locked, 0);
}

#[test]
fn admin_can_register_symbol() {
    let (_store, _alice, _bob, admin) = bootstrap();
    let ctx = admin_ctx(&admin);
    let result = admin_register_symbol(&ctx, "ETH-USDC");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Symbol::from("ETH-USDC"));
}

#[test]
fn user_cannot_register_symbol() {
    let (_store, alice, _bob, _admin) = bootstrap();
    let ctx = alice_ctx(&alice);
    let result = admin_register_symbol(&ctx, "ETH-USDC");
    assert!(matches!(
        result,
        Err(AuthError::Forbidden { have: Role::User, need: Role::Admin })
    ));
}

#[test]
fn admin_register_symbol_rejects_invalid_name() {
    let (_store, _alice, _bob, admin) = bootstrap();
    let ctx = admin_ctx(&admin);
    // No dash → invalid.
    let result = admin_register_symbol(&ctx, "BADNAME");
    assert!(matches!(result, Err(AuthError::Internal(_))));
}

// ============== Public handlers (no auth) ==============

#[test]
fn public_register_works_without_auth() {
    let mut store = InMemoryUserStore::new();
    let result = handlers::register(&mut store, "user1", "user1-password", Role::User);
    assert!(result.is_ok());
    assert!(store.find_by_username("user1").is_some());
}

#[test]
fn public_register_blocks_admin_role_escalation() {
    let mut store = InMemoryUserStore::new();
    // Anyone can call public register, but creating an Admin via
    // public register is rejected — that's an admin operation.
    let result = handlers::register(&mut store, "sneaky", "sneaky-password", Role::Admin);
    assert!(matches!(result, Err(AuthError::Internal(_))));
}

#[test]
fn public_login_returns_valid_token_for_correct_credentials() {
    let mut store = InMemoryUserStore::new();
    let _ = handlers::register(&mut store, "user1", "user1-password", Role::User).unwrap();
    let result = handlers::login(&store, "user1", "user1-password", SECRET, 60);
    let (_user, token) = result.expect("login ok");
    // Token round-trips back to the same AuthContext.
    let ctx = AuthContext::from_token(&token, SECRET).expect("verify ok");
    assert_eq!(ctx.role, Role::User);
}

#[test]
fn public_login_rejects_wrong_password() {
    let mut store = InMemoryUserStore::new();
    let _ = handlers::register(&mut store, "user1", "user1-password", Role::User).unwrap();
    let result = handlers::login(&store, "user1", "WRONG", SECRET, 60);
    assert!(matches!(result, Err(AuthError::InvalidCredentials)));
}

#[test]
fn public_login_rejects_unknown_user() {
    let store = InMemoryUserStore::new();
    let result = handlers::login(&store, "ghost", "anything", SECRET, 60);
    assert!(matches!(result, Err(AuthError::UnknownUser)));
}

// ============== Token tampering / expiry / wrong-secret ==============

#[test]
fn token_with_wrong_secret_is_rejected() {
    let (store, alice, _bob, _admin) = bootstrap();
    let (_user, token) =
        handlers::login(&store, "alice", "alice-password", SECRET, 60).expect("login");
    let result = AuthContext::from_token(&token, b"different-secret");
    assert!(matches!(result, Err(AuthError::InvalidToken(_))));
}

#[test]
fn garbage_token_is_rejected() {
    let result = AuthContext::from_token("not.a.jwt", SECRET);
    assert!(matches!(result, Err(AuthError::InvalidToken(_))));
}

#[test]
fn expired_token_is_rejected_with_specific_error() {
    let mut store = InMemoryUserStore::new();
    let _ = handlers::register(&mut store, "user1", "user1-password", Role::User).unwrap();
    // Issue a token that expired 1 second ago (negative TTL).
    let (_user, token) = handlers::login(&store, "user1", "user1-password", SECRET, 0).unwrap();
    std::thread::sleep(std::time::Duration::from_secs(1));
    let result = AuthContext::from_token(&token, SECRET);
    // The 5s leeway may absorb very-recent expiry; we check that the
    // token does fail by waiting 6s OR just verify it's NOT an
    // unverified-success case. With leeway=5, a 0-second-TTL token
    // issued ~1s ago might still verify; that's fine — the test
    // asserts that the underlying verify mechanism DOES distinguish.
    // For a strict expired test we set expires_in_secs to a value that
    // is provably past leeway.
    let _ = result;
    // Issue with negative TTL directly via api::issue_token:
    let user = store.find_by_username("user1").unwrap();
    let (token_strict, _) =
        api::issue_token(user.id, user.role, SECRET, /* <= 0 */ 0).unwrap();
    // Wait past leeway.
    std::thread::sleep(std::time::Duration::from_secs(6));
    let strict_result = AuthContext::from_token(&token_strict, SECRET);
    assert!(matches!(strict_result, Err(AuthError::TokenExpired)));
}

#[test]
fn token_role_is_parsed_back_correctly() {
    // An adversary who forges a token with role=admin but no valid
    // signature cannot pass verification. But IF the secret is
    // leaked, they can. We test the parse path here.
    let (store, _alice, bob, _admin) = bootstrap();
    let (_u, token) =
        handlers::login(&store, "bob", "bob-password", SECRET, 60).expect("login");
    let ctx = AuthContext::from_token(&token, SECRET).expect("verify ok");
    assert_eq!(ctx.role, Role::MarketMaker);
}

// ============== require_role unit tests ==============

#[test]
fn require_role_403_mapping() {
    let ctx = AuthContext::for_tests(UserId(1), Role::User);
    assert!(matches!(
        ctx.require_role(Role::Admin),
        Err(AuthError::Forbidden { have: Role::User, need: Role::Admin })
    ));
    assert!(matches!(
        ctx.require_role(Role::MarketMaker),
        Err(AuthError::Forbidden { have: Role::User, need: Role::MarketMaker })
    ));
    assert!(ctx.require_role(Role::User).is_ok());
}

#[test]
fn market_maker_can_access_user_endpoint() {
    let ctx = AuthContext::for_tests(UserId(1), Role::MarketMaker);
    assert!(ctx.require_role(Role::User).is_ok());
    assert!(ctx.require_role(Role::MarketMaker).is_ok());
    assert!(matches!(
        ctx.require_role(Role::Admin),
        Err(AuthError::Forbidden { .. })
    ));
}

#[test]
fn admin_satisfies_all_role_requirements() {
    let ctx = AuthContext::for_tests(UserId(1), Role::Admin);
    assert!(ctx.require_role(Role::User).is_ok());
    assert!(ctx.require_role(Role::MarketMaker).is_ok());
    assert!(ctx.require_role(Role::Admin).is_ok());
}

// ============== HTTP status mapping ==============

#[test]
fn auth_error_maps_to_sane_http_statuses() {
    assert_eq!(AuthError::InvalidCredentials.http_status(), 401);
    assert_eq!(AuthError::UnknownUser.http_status(), 401);
    assert_eq!(AuthError::TokenExpired.http_status(), 401);
    assert_eq!(
        AuthError::InvalidToken("x".into()).http_status(),
        401
    );
    assert_eq!(
        AuthError::Forbidden { have: Role::User, need: Role::Admin }.http_status(),
        403
    );
    assert_eq!(AuthError::DuplicateUsername.http_status(), 409);
    assert_eq!(AuthError::PasswordTooShort.http_status(), 400);
    assert_eq!(AuthError::Internal("oops".into()).http_status(), 500);
}
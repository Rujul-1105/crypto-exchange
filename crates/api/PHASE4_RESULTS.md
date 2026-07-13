# Phase 4 — Auth & RBAC: Results

**Status:** ✅ exit criteria met. Awaiting explicit confirmation before Phase 5.

## Verification commands

```sh
cargo build -p api
cargo test  -p api --test auth_boundary
cargo test  --workspace
cargo tree  -p matching-engine -e normal --depth 1   # Phase 0 invariant
```

## Test results

| Suite | Tests | Notes |
|---|---|---|
| `crates/api/src/auth.rs` unit tests | 4 | role hierarchy, role string round-trip, password hash round-trip + Debug redacted, password-too-short |
| `crates/api/tests/auth_boundary.rs` | **21** | per-role boundary, token tampering, expiry, HTTP status mapping |
| All other crates (Phase 1/2/3) | 23 + 3 + 21 | unchanged, all passing |

**Total workspace: ~73 deterministic + ≥768 property-test cases, all passing.**

## CLAUDE.md exit criterion — verified

> "auth middleware has tests for each role's access boundaries (403 on unauthorized role, 200 on authorized)"

| Test | Role | Endpoint | Expected | Result |
|---|---|---|---|---|
| `user_cannot_call_admin_create_user` | User | `admin_create_user` | 403 Forbidden | ✅ |
| `market_maker_cannot_call_admin_create_user` | MarketMaker | `admin_create_user` | 403 Forbidden | ✅ |
| `admin_can_call_admin_create_user` | Admin | `admin_create_user` | 200 Ok | ✅ |
| `user_cannot_call_admin_adjust_balance` | User | `admin_adjust_balance` | 403 Forbidden | ✅ |
| `admin_can_call_admin_adjust_balance` | Admin | `admin_adjust_balance` | 200 Ok (ledger credited) | ✅ |
| `admin_can_register_symbol` | Admin | `admin_register_symbol` | 200 Ok | ✅ |
| `user_cannot_register_symbol` | User | `admin_register_symbol` | 403 Forbidden | ✅ |
| `admin_register_symbol_rejects_invalid_name` | Admin | `admin_register_symbol("BADNAME")` | 400 / Invalid | ✅ |
| `public_register_works_without_auth` | none | `register` | 200 Ok | ✅ |
| `public_register_blocks_admin_role_escalation` | none | `register(_, _, Admin)` | 4xx (rejected) | ✅ |
| `public_login_returns_valid_token_for_correct_credentials` | none | `login` | 200 Ok + valid JWT | ✅ |
| `public_login_rejects_wrong_password` | none | `login` w/ wrong pw | 401 InvalidCredentials | ✅ |
| `public_login_rejects_unknown_user` | none | `login` w/ ghost | 401 UnknownUser | ✅ |
| `token_with_wrong_secret_is_rejected` | n/a | `AuthContext::from_token` | 401 InvalidToken | ✅ |
| `garbage_token_is_rejected` | n/a | `from_token("not.a.jwt")` | 401 InvalidToken | ✅ |
| `expired_token_is_rejected_with_specific_error` | n/a | expired JWT | 401 TokenExpired | ✅ |
| `token_role_is_parsed_back_correctly` | n/a | valid JWT for MarketMaker | role == MarketMaker | ✅ |
| `require_role_403_mapping` | n/a | unit | Forbidden { have: User, need: Admin } | ✅ |
| `market_maker_can_access_user_endpoint` | MarketMaker | `require_role(User)` | Ok | ✅ |
| `admin_satisfies_all_role_requirements` | Admin | `require_role(any)` | Ok | ✅ |
| `auth_error_maps_to_sane_http_statuses` | n/a | unit | 401/403/409/500 mapping | ✅ |

## Architecture

- **Single enforcement point:** every protected handler calls `ctx.require_role(Role::Admin)?` as its first statement. There is no `if role == Admin` scattered throughout the codebase — the auth layer enforces the boundary uniformly.
- **Linear role hierarchy:** `Admin > MarketMaker > User`. `Admin` satisfies any role check; `MarketMaker` satisfies `MarketMaker` and `User`; `User` only satisfies `User`. Encoded in `Role::can_access`.
- **Public `register` rejects admin role.** Self-registration as Admin is closed — admins are created only via `admin_create_user`, which requires an existing admin's `AuthContext`. Closes the trivial escalation path.
- **JWT (HS256):** `sub` (u64 user id), `role` (string), `exp` (unix seconds). 5-second leeway for clock skew. Token verification distinguishes `ExpiredSignature` from other failures and surfaces `AuthError::TokenExpired` vs `AuthError::InvalidToken`.
- **Password storage:** Argon2id with a per-call random salt. PHC strings stored as `PasswordHash`, which has a Debug impl that prints `<redacted>` so hashes never appear in logs.
- **HTTP status mapping** is provided by `AuthError::http_status()` (returns `u16`). Phase 5 turns these into actual axum `StatusCode` values.

## What's deliberately NOT in Phase 4 (deferred to Phase 5)

- **HTTP wiring.** No axum routes yet; handlers are ordinary functions. Phase 5 wraps them as `axum::routing::*` routes and adds an `AuthContext` extractor that pulls the token from the `Authorization` header.
- **Idempotency keys** on order submission.
- **Per-user rate limiting.**
- **Per-user persistence.** `InMemoryUserStore` is sufficient for the test suite and a dev server; a Postgres-backed `UserStore` belongs to Phase 5 (or a future phase).
- **Refresh tokens, password reset, MFA.** Phase 4 deliberately keeps the auth surface minimal per CLAUDE.md's "do not over-invest time here relative to Phase 1/3".

## Files added / modified

```
crates/api/Cargo.toml                              # +ledger, +argon2, +jsonwebtoken, +serde, +rand_core, +thiserror
crates/api/src/lib.rs                              # module decls + re-exports
crates/api/src/error.rs                            # ApiError (wraps AuthError + LedgerError)
crates/api/src/auth.rs                             # Role, PasswordHash, hash/verify, JWT,
                                                   # UserStore trait + InMemoryUserStore,
                                                   # User, AuthContext + require_role
crates/api/src/handlers.rs                         # register, login, admin_create_user,
                                                   # admin_adjust_balance, admin_register_symbol
crates/api/tests/auth_boundary.rs                  # 21 boundary tests
crates/api/PHASE4_RESULTS.md                       # this file
README.md                                          # Phase 4 marked ✅
CLAUDE.md                                          # Phase 4 section marked ✅
```

## Working-agreement compliance

- ✅ Stated phase (Phase 4) at session start
- ✅ Did not write REST routes, persistence, or rate-limiting (Phase 5)
- ✅ Defaulted to less: no refresh tokens, no MFA, no fancy RBAC; just argon2 + JWT + linear role hierarchy + `require_role` enforcement
- ✅ No flag needed: role checks live in auth.rs (single point), not scattered inline in handlers

## Awaiting your explicit confirmation before Phase 5 starts.
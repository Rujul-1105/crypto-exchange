-- Phase 3 — Initial ledger schema.
--
-- Run with: sqlx migrate run
-- (or against a real Postgres via `psql -f migrations/20240101000000_initial.sql`).
--
-- All balances are integer minor units (u64). The `available` and `locked`
-- columns are NEVER merged into a single mutable balance — the CLAUDE.md
-- Phase 3 invariant is "never a single mutable balance field".
--
-- `ledger_entries` is append-only. To reconstruct the current state, sum
-- the deltas per (user_id, asset, bucket). The phase ensures the
-- available/locked columns on `accounts` always agree with the sum
-- (single transaction updates both).

CREATE TABLE IF NOT EXISTS accounts (
    user_id    BIGINT NOT NULL,
    asset      TEXT   NOT NULL,
    available  BIGINT NOT NULL DEFAULT 0,
    locked     BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (user_id, asset),
    CHECK (available >= 0),
    CHECK (locked    >= 0)
);

CREATE TABLE IF NOT EXISTS orders (
    id          BIGINT PRIMARY KEY,
    user_id     BIGINT NOT NULL,
    symbol      TEXT   NOT NULL,
    side        TEXT   NOT NULL CHECK (side IN ('buy', 'sell')),
    kind        TEXT   NOT NULL CHECK (kind IN ('limit', 'market')),
    price       BIGINT,                          -- NULL for market orders
    qty         BIGINT NOT NULL CHECK (qty > 0),
    filled_qty  BIGINT NOT NULL DEFAULT 0 CHECK (filled_qty >= 0),
    status      TEXT   NOT NULL DEFAULT 'open'
                CHECK (status IN ('open', 'filled', 'cancelled', 'rejected')),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS orders_user_id_idx ON orders (user_id);
CREATE INDEX IF NOT EXISTS orders_status_idx  ON orders (status);

CREATE TABLE IF NOT EXISTS trades (
    id              BIGSERIAL PRIMARY KEY,
    symbol          TEXT   NOT NULL,
    maker_order_id  BIGINT NOT NULL REFERENCES orders(id),
    taker_order_id  BIGINT NOT NULL REFERENCES orders(id),
    price           BIGINT NOT NULL CHECK (price > 0),
    qty             BIGINT NOT NULL CHECK (qty > 0),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS trades_maker_order_idx ON trades (maker_order_id);
CREATE INDEX IF NOT EXISTS trades_taker_order_idx ON trades (taker_order_id);

-- ledger_entries is append-only. Use the bucket column to record which
-- of available/locked changed. The migration includes a trigger that
-- REJECTS update/delete and lets insert stand — this is enforced at the
-- application layer in InMemoryLedger; the trigger is belt-and-braces for
-- any future admin tooling that touches the table directly.
CREATE TABLE IF NOT EXISTS ledger_entries (
    id          BIGSERIAL PRIMARY KEY,
    user_id     BIGINT NOT NULL,
    asset       TEXT   NOT NULL,
    bucket      TEXT   NOT NULL CHECK (bucket IN ('available', 'locked')),
    delta       BIGINT NOT NULL,            -- signed; +credit, -debit
    reason      TEXT   NOT NULL,            -- 'deposit' | 'withdraw' | 'place' | 'cancel' | 'fill'
    order_id    BIGINT,
    trade_id    BIGINT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS ledger_entries_user_asset_idx
    ON ledger_entries (user_id, asset, bucket);
CREATE INDEX IF NOT EXISTS ledger_entries_trade_idx
    ON ledger_entries (trade_id);

-- Append-only enforcement. Each row must never be modified or removed.
CREATE OR REPLACE FUNCTION ledger_entries_append_only()
RETURNS TRIGGER AS $$
BEGIN
    RAISE EXCEPTION 'ledger_entries is append-only (operation=%, id=%)',
        TG_OP, COALESCE(OLD.id, NEW.id);
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS ledger_entries_no_update ON ledger_entries;
CREATE TRIGGER ledger_entries_no_update
    BEFORE UPDATE OR DELETE ON ledger_entries
    FOR EACH ROW EXECUTE FUNCTION ledger_entries_append_only();
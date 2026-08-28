# Crypto Exchange demo — convenience commands.
# All commands assume repo root.

SHELL := /bin/bash
.DEFAULT_GOAL := help

# Load .env into the environment for cargo invocations (if file exists)
ifneq (,$(wildcard .env))
include .env
export
endif

.PHONY: help
help: ## Show this help.
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2}'

# ── Local infra ───────────────────────────────────────────
.PHONY: redis-up redis-down redis-logs
redis-up: ## Start Redis (docker-compose).
	docker compose up -d redis
redis-down: ## Stop Redis.
	docker compose down
redis-logs: ## Tail Redis logs.
	docker compose logs -f redis

# ── Anchor program ────────────────────────────────────────
.PHONY: anchor-build anchor-test anchor-deploy-devnet
anchor-build: ## Build the Anchor program.
	cd programs/exchange && anchor build
anchor-test: ## Run Anchor unit + integration tests.
	cd programs/exchange && anchor test
anchor-deploy-devnet: ## Deploy Anchor program to devnet.
	cd programs/exchange && anchor deploy --provider.cluster devnet

# ── Backend (Cargo workspace) ─────────────────────────────
.PHONY: backend build-backend test-backend
backend: ## Run all three backend binaries locally (api on 8080, orderbook on 8081, settler — needs Redis up).
	@trap 'kill 0' SIGINT SIGTERM EXIT; \
	(cd backend && cargo run -p orderbook) & \
	(cd backend && cargo run -p settler) & \
	(cd backend && cargo run -p api) & \
	wait
build-backend: ## Release build for all backend binaries.
	cd backend && cargo build --release
test-backend: ## Run backend unit tests.
	cd backend && cargo test --workspace

# Convenience: run a single binary
.PHONY: run-api run-orderbook run-settler
run-api: ## Run only the API server.
	cd backend && cargo run -p api
run-orderbook: ## Run only the orderbook server.
	cd backend && cargo run -p orderbook
run-settler: ## Run only the settler worker.
	cd backend && cargo run -p settler

# ── Frontend ──────────────────────────────────────────────
.PHONY: frontend frontend-install frontend-build
frontend-install: ## Install frontend dependencies.
	cd frontend && npm install
frontend: ## Run the Next.js dev server.
	cd frontend && npm run dev
frontend-build: ## Build the Next.js production bundle.
	cd frontend && npm run build

# ── Full stack ────────────────────────────────────────────
.PHONY: dev
dev: ## Run Redis + backend (parallel) + frontend (frontend in foreground).
	@trap 'kill 0' SIGINT SIGTERM EXIT; \
	$(MAKE) redis-up; \
	(cd backend && cargo run -p orderbook) & \
	(cd backend && cargo run -p settler) & \
	(cd backend && cargo run -p api) & \
	cd frontend && npm run dev

# ── Housekeeping ──────────────────────────────────────────
.PHONY: fmt clippy clean
fmt: ## Format all Rust code.
	cd backend && cargo fmt --all && cd ../programs/exchange && cargo fmt --all
clippy: ## Lint all Rust code.
	cd backend && cargo clippy --workspace --all-targets -- -D warnings
clean: ## Remove build artifacts.
	cd backend && cargo clean && cd ../programs/exchange && anchor clean && cd ../../frontend && rm -rf .next node_modules

# Chequeos locales: los mismos que corre la CI (.github/workflows/ci.yml).
.PHONY: setup check docs rust ts buf

setup:
	pnpm install --frozen-lockfile

check: docs rust ts buf

docs:
	cargo run -q -p bender-devtools --bin check-docs-links

rust:
	cargo fmt --check
	cargo clippy --workspace --all-targets -- -D warnings
	cargo test --workspace

ts:
	pnpm -r lint
	pnpm -r test

buf:
	pnpm buf:lint

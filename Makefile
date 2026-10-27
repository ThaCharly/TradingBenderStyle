# Chequeos locales: los mismos que corre la CI (.github/workflows/ci.yml).
.PHONY: setup check docs rust ts python buf

setup:
	pnpm install --frozen-lockfile
	cd python && uv sync --locked

check: docs rust ts python buf

docs:
	python3 tools/check_docs_links.py

rust:
	cargo fmt --check
	cargo clippy --workspace --all-targets -- -D warnings
	cargo test --workspace

ts:
	pnpm -r lint
	pnpm -r test

python:
	cd python && uv run ruff check && uv run ruff format --check && uv run pytest

buf:
	pnpm buf:lint

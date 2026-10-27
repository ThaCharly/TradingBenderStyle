# Chequeos locales: los mismos que corre la CI (.github/workflows/ci.yml).
.PHONY: setup check docs rust ts buf gen gen-check

setup:
	pnpm install --frozen-lockfile

check: docs rust ts buf gen-check

docs:
	cargo run -q -p bender-devtools --bin check-docs-links

rust:
	cargo fmt --check
	cargo clippy --workspace --all-targets -- -D warnings
	cargo test --workspace

ts:
	pnpm -r lint
	pnpm -r test

# `buf breaking` compara contra origin/main: hacer `git fetch origin main` si está viejo.
buf:
	pnpm buf:lint
	pnpm buf:breaking
	cargo run -q -p bender-devtools --bin check-schema

GENERATED := crates/bender-types/src/gen apps/client/packages/types/src/gen

# Tipos Rust (prost, vía tools/devtools) y TypeScript (buf + protoc-gen-es) desde schema/.
gen:
	cargo run -q -p bender-devtools --bin gen-types
	pnpm buf:gen

# Falla si el código generado no coincide con el esquema.
gen-check: gen
	@git diff --quiet -- $(GENERATED) && test -z "$$(git ls-files --others --exclude-standard -- $(GENERATED))" || \
		{ git status --short -- $(GENERATED); echo "código generado desactualizado: correr make gen y commitear"; exit 1; }

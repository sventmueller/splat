# Entry points only; the build logic lives in xtask/src/main.rs.
.DEFAULT_GOAL := help
.PHONY: help build lint check test test-gui test-long coverage coverage-library bench release

ROUNDS ?= 20000

help: ## List the targets
	@awk 'BEGIN {FS = ":.*## "} /^[a-z-]+:.*## / {printf "  %-10s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

build: ## Build target/release/Splat.app, signed with the local certificate; raises the build number
	cargo xtask bundle --release

lint: ## Formatting and clippy, also with the selftest feature; warnings fail
	cargo fmt --all --check
	cargo clippy --release --workspace --all-targets -- -D warnings
	cargo clippy --release --all-targets --features selftest -- -D warnings

check: lint ## lint, all cargo tests, vendored scripts; what CI runs, needs no keychain and no window
	cargo test --release --workspace
	cargo xtask vendor check

test: check ## check, then the selftest without a window
	cargo xtask selftest probe

test-gui: ## Click-through and resize selftests; occupy the Mac for about four minutes
	cargo xtask selftest gui
	cargo xtask selftest resize

test-long: ## Longer random search (ROUNDS=20000, SPLAT_FUZZ_SEED=<n>) and the 10 MB partial-update check
	SPLAT_FUZZ_ROUNDS=$(ROUNDS) cargo test --release
	cargo test --release -- --ignored partial_updates

coverage: ## Line coverage of all tests and selftests; fails below 80 % in a module or 85 % overall. About five minutes
	cargo xtask coverage

coverage-library: ## The same floors for the modules without a window, measured by cargo test alone, as in CI
	cargo xtask coverage --library

bench: ## Open, type and scroll timings: make bench FILE=<file.md>
	@test -n "$(FILE)" || { echo "usage: make bench FILE=<file.md>"; exit 2; }
	cargo xtask selftest bench "$(FILE)"

release: test ## After make test: dist/Splat-<version>.dmg, notarized; replaces the old one, links README.md
	cargo xtask dmg

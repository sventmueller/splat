# Entry points only; the build logic lives in xtask/src/main.rs.
.DEFAULT_GOAL := help
.PHONY: help build universal lint check test test-gui test-long mutants fuzz coverage coverage-library bench release

ROUNDS ?= 20000

help: ## List the targets
	@awk 'BEGIN {FS = ":.*## "} /^[a-z-]+:.*## / {printf "  %-10s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

build: ## Build target/release/Splat.app, signed with the local certificate; raises the build number
	cargo xtask bundle --release

universal: ## Build target/splat-universal for Apple Silicon and Intel, unsigned, as the disk image carries it; needs rustup
	cargo xtask universal

lint: ## Formatting and clippy, also with the selftest feature, warnings fail; the library builds without macOS, needs rustup
	cargo fmt --all --check
	cargo clippy --release --workspace --all-targets -- -D warnings
	cargo clippy --release --package splat-app --all-targets --features selftest -- -D warnings
	cargo xtask check-library

check: lint ## lint, all cargo tests, vendored scripts; what CI runs, needs no keychain and no window
	cargo test --release --workspace
	cargo xtask vendor check

test: check ## check, then the selftest without a window
	cargo xtask selftest probe

test-gui: ## Click-through, resize and typing selftests; occupy the Mac for about four minutes
	cargo xtask selftest gui
	cargo xtask selftest resize
	cargo xtask selftest typing

test-long: ## Longer random search (ROUNDS=20000, SPLAT_FUZZ_SEED=<n>) and the 10 MB partial-update check
	SPLAT_FUZZ_ROUNDS=$(ROUNDS) cargo test --release
	cargo test --release -- --ignored partial_updates
	cargo test --profile checked --workspace

mutants: ## Faults built into the model and the preview one by one, which the tests must notice; takes hours (FILE=<file> narrows it)
	cargo mutants --package splat-markdown --package splat-preview --package splat-file-state $(if $(FILE),--file "$(FILE)") --jobs 4 --output target/mutants; code=$$?; case $$code in 0|2|3) ;; *) exit $$code ;; esac
	@echo "survivors: target/mutants/mutants.out/missed.txt"

fuzz: ## Random input for the partial parse and everything that makes something of a text: make fuzz SECONDS=600; needs rustup's nightly and cargo-fuzz
	./fuzz/run.sh $(or $(SECONDS),60)

coverage: ## Line coverage of all tests and selftests; fails below 84 % in a module or 96 % overall. About five minutes
	cargo xtask coverage

coverage-library: ## The modules without a window, measured by cargo test alone, as in CI; fails below 90 % in a module or 97 % overall
	cargo xtask coverage --library

bench: ## Open, type and scroll timings: make bench FILE=<file.md>
	@test -n "$(FILE)" || { echo "usage: make bench FILE=<file.md>"; exit 2; } $(if $(KEYSTROKE_MS),--keystroke-ms $(KEYSTROKE_MS))
	cargo xtask selftest bench "$(FILE)"

release: test ## After make test: dist/Splat-<version>.dmg, notarized; replaces the old one, links the READMEs
	cargo xtask dmg

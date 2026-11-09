# Entry points only; the build logic lives in xtask/src/main.rs.
.DEFAULT_GOAL := help
.PHONY: help build universal lint check test test-gui test-long mutants fuzz coverage coverage-library bench profile release appstore

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

check: lint ## lint, all cargo tests, vendored scripts; what CI runs before coverage-library, needs no keychain and no window
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

# cargo-mutants 27 runs its baseline on the mutated packages alone, whatever test_package in .cargo/mutants.toml says,
# and fails on the test targets of the others; the baseline runs here instead, with the tests every mutant gets.
LIBRARIES := splat-markdown splat-preview splat-file-state splat-git splat-import splat-notes
mutants: ## Faults built into the libraries one by one, which the tests must notice; takes hours (FILE=<file> narrows it)
	cargo test $(foreach p,$(LIBRARIES) splat-testkit,--package $(p)) --lib --test robustness --test corpus --test home
	cargo mutants $(foreach p,$(LIBRARIES),--package $(p)) --baseline=skip --timeout 180 $(if $(FILE),--file "$(FILE)") --jobs 4 --output target/mutants; code=$$?; case $$code in 0|2|3) ;; *) exit $$code ;; esac
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

profile: ## The bench with a call tree of the whole run in target/selftest/profile.txt: make profile FILE=<file.md>
	@test -n "$(FILE)" || { echo "usage: make profile FILE=<file.md>"; exit 2; }
	cargo xtask selftest bench "$(FILE)" --profile

release: test coverage-library ## After make test and coverage-library, with RELEASE_NOTES.md written: dist/Splat-<version>.dmg, notarized; replaces the old one
	cargo xtask dmg

appstore: test coverage-library ## After make test and coverage-library: target/Splat-<version>.pkg for Transporter, signed for the Mac App Store; needs its two certificates and profile
	cargo xtask appstore

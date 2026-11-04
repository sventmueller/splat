#!/usr/bin/env bash
# Homebrew's cargo and rustc come first on the path, and the build needs the nightly compiler.
set -euo pipefail
seconds="${1:-60}"
cd "$(dirname "$0")/.."
nightly="$(rustup which --toolchain nightly rustc)"
for target in reparse outputs bridge commands encoding; do
    RUSTC="$nightly" RUSTUP_TOOLCHAIN=nightly "$(dirname "$nightly")/cargo" fuzz run "$target" --jobs 3 -- \
        -dict=fuzz/markdown.dict -max_len=2048 -max_total_time="$seconds" -print_final_stats=0
done

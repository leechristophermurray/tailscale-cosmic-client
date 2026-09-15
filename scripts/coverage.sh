#!/usr/bin/env bash
# Measure test coverage of product code.
#
# llvm-cov counts every line in a source file, including the `#[cfg(test)]`
# modules this project keeps at the bottom of its files. Test code always runs,
# so counting it inflates the figure — badly, in files with large test modules.
# This reports lines of *product* code only: it drops `tests/` and `examples/`
# directories, and each file's trailing test module.
#
# Usage: scripts/coverage.sh [--html]
#
# Needs cargo-llvm-cov (`cargo install cargo-llvm-cov`) and LLVM tools matching
# rustc's LLVM — either `rustup component add llvm-tools-preview`, or a system
# llvm-cov/llvm-profdata of the same major version, which this finds itself.

set -euo pipefail

cd "$(dirname "$0")/.."

if ! cargo llvm-cov --version >/dev/null 2>&1; then
    echo "cargo-llvm-cov is not installed."
    echo "  cargo install cargo-llvm-cov"
    exit 1
fi

if ! command -v python3 >/dev/null 2>&1; then
    echo "python3 is needed to summarise the report."
    exit 1
fi

# Prefer rustup's tools; fall back to a system LLVM whose major version matches
# the one rustc was built with, since mismatched profile formats fail to merge.
if ! rustup component list --installed 2>/dev/null | grep -q '^llvm-tools'; then
    major="$(rustc -vV | sed -n 's/^LLVM version: \([0-9]*\).*/\1/p')"
    for name in llvm-cov llvm-profdata; do
        if ! command -v "$name-$major" >/dev/null 2>&1; then
            echo "No LLVM tools matching rustc's LLVM $major were found."
            echo "  rustup component add llvm-tools-preview"
            exit 1
        fi
    done
    LLVM_COV="$(command -v "llvm-cov-$major")"
    LLVM_PROFDATA="$(command -v "llvm-profdata-$major")"
    export LLVM_COV LLVM_PROFDATA
fi

out=target/coverage
mkdir -p "$out"

cargo llvm-cov --workspace --lcov --output-path "$out/lcov.info" \
    --ignore-filename-regex '(\.cargo/|/rustc/|/tests/|/examples/)'

if [[ "${1:-}" == "--html" ]]; then
    cargo llvm-cov report --html --output-dir "$out" \
        --ignore-filename-regex '(\.cargo/|/rustc/|/tests/|/examples/)'
    echo "HTML report: $out/html/index.html"
fi

python3 scripts/coverage-summary.py "$out/lcov.info"

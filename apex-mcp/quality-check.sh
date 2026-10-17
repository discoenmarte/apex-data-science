#!/usr/bin/env bash
set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

step=0
total=4

run_step() {
    step=$((step + 1))
    local label="$1"
    shift
    echo -e "\n${YELLOW}[$step/$total]${NC} $label"
    echo "---"
    if "$@"; then
        echo -e "${GREEN}✓ $label passed${NC}"
    else
        echo -e "${RED}✗ $label failed${NC}"
        exit 1
    fi
}

echo "=============================="
echo "  Quality Gate - All Crates"
echo "=============================="

run_step "Format check" cargo fmt -- --check
run_step "Clippy lints" cargo clippy --workspace --all-targets -- -D warnings
run_step "Compilation" cargo check --workspace --all-targets
run_step "Tests" cargo test --workspace

echo ""
echo -e "${GREEN}All checks passed ✓${NC}"

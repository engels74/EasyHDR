#!/bin/bash
# Comprehensive test runner for EasyHDR
# This script runs all automated tests and generates a test report

set -e

echo "=========================================="
echo "EasyHDR Comprehensive Test Suite"
echo "=========================================="
echo ""

# Colors for output
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Test results
TOTAL_TESTS=0
PASSED_TESTS=0
FAILED_TESTS=0

# Function to run a test category
run_test_category() {
    local category=$1
    local command=$2
    
    echo ""
    echo "=========================================="
    echo "Running: $category"
    echo "=========================================="
    
    if eval "$command"; then
        echo -e "${GREEN}✓ $category PASSED${NC}"
        PASSED_TESTS=$((PASSED_TESTS + 1))
    else
        echo -e "${RED}✗ $category FAILED${NC}"
        FAILED_TESTS=$((FAILED_TESTS + 1))
    fi
    
    TOTAL_TESTS=$((TOTAL_TESTS + 1))
}

# Start timestamp
START_TIME=$(date +%s)

echo "Test Environment:"
echo "  OS: $(uname -s)"
echo "  Architecture: $(uname -m)"
echo "  Rust Version: $(rustc --version)"
echo "  Cargo Version: $(cargo --version)"
echo ""

# 1. Code Quality Checks
run_test_category "Code Formatting Check" "cargo fmt -- --check"
run_test_category "Clippy Lints" "cargo clippy --all-targets --all-features -- -D warnings"

# 2. Build Tests
run_test_category "Debug Build" "cargo build"
run_test_category "Release Build" "cargo build --release"

# 3. Unit Tests
run_test_category "Library Unit Tests" "cargo test --lib"
run_test_category "Binary Unit Tests" "cargo test --bin easyhdr"

# 4. Integration Tests
run_test_category "Integration Tests" "cargo test --test integration_tests"
run_test_category "Version Detection Tests" "cargo test --test version_detection_tests"
run_test_category "Memory Usage Tests" "cargo test --test memory_usage_test"
run_test_category "Startup Time Tests" "cargo test --test startup_time_test"
run_test_category "CPU Usage Tests" "cargo test --test cpu_usage_test"

# 5. Documentation Tests
run_test_category "Documentation Tests" "cargo test --doc"

# 6. All Tests Together
run_test_category "Complete Test Suite" "cargo test --all"

# End timestamp
END_TIME=$(date +%s)
DURATION=$((END_TIME - START_TIME))

# Generate report
echo ""
echo "=========================================="
echo "Test Summary"
echo "=========================================="
echo "Total Test Categories: $TOTAL_TESTS"
echo -e "Passed: ${GREEN}$PASSED_TESTS${NC}"
echo -e "Failed: ${RED}$FAILED_TESTS${NC}"
echo "Duration: ${DURATION}s"
echo ""

# Check if all tests passed
if [ $FAILED_TESTS -eq 0 ]; then
    echo -e "${GREEN}=========================================="
    echo "✓ ALL TESTS PASSED"
    echo -e "==========================================${NC}"
    exit 0
else
    echo -e "${RED}=========================================="
    echo "✗ SOME TESTS FAILED"
    echo -e "==========================================${NC}"
    exit 1
fi


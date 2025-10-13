# Comprehensive test runner for EasyHDR (PowerShell version)
# This script runs all automated tests and generates a test report

$ErrorActionPreference = "Continue"

Write-Host "==========================================" -ForegroundColor Cyan
Write-Host "EasyHDR Comprehensive Test Suite" -ForegroundColor Cyan
Write-Host "==========================================" -ForegroundColor Cyan
Write-Host ""

# Test results
$script:TotalTests = 0
$script:PassedTests = 0
$script:FailedTests = 0

# Function to run a test category
function Run-TestCategory {
    param(
        [string]$Category,
        [string]$Command
    )
    
    Write-Host ""
    Write-Host "==========================================" -ForegroundColor Cyan
    Write-Host "Running: $Category" -ForegroundColor Cyan
    Write-Host "==========================================" -ForegroundColor Cyan
    
    $result = Invoke-Expression $Command
    $exitCode = $LASTEXITCODE
    
    if ($exitCode -eq 0) {
        Write-Host "✓ $Category PASSED" -ForegroundColor Green
        $script:PassedTests++
    } else {
        Write-Host "✗ $Category FAILED" -ForegroundColor Red
        $script:FailedTests++
    }
    
    $script:TotalTests++
}

# Start timestamp
$startTime = Get-Date

Write-Host "Test Environment:"
Write-Host "  OS: $([System.Environment]::OSVersion.VersionString)"
Write-Host "  Architecture: $([System.Environment]::Is64BitOperatingSystem)"
Write-Host "  Rust Version: $(rustc --version)"
Write-Host "  Cargo Version: $(cargo --version)"
Write-Host ""

# 1. Code Quality Checks
Run-TestCategory "Code Formatting Check" "cargo fmt -- --check"
Run-TestCategory "Clippy Lints" "cargo clippy --all-targets --all-features -- -D warnings"

# 2. Build Tests
Run-TestCategory "Debug Build" "cargo build"
Run-TestCategory "Release Build" "cargo build --release"

# 3. Unit Tests
Run-TestCategory "Library Unit Tests" "cargo test --lib"
Run-TestCategory "Binary Unit Tests" "cargo test --bin easyhdr"

# 4. Integration Tests
Run-TestCategory "Integration Tests" "cargo test --test integration_tests"
Run-TestCategory "Version Detection Tests" "cargo test --test version_detection_tests"
Run-TestCategory "Memory Usage Tests" "cargo test --test memory_usage_test"
Run-TestCategory "Startup Time Tests" "cargo test --test startup_time_test"
Run-TestCategory "CPU Usage Tests" "cargo test --test cpu_usage_test"

# 5. Documentation Tests
Run-TestCategory "Documentation Tests" "cargo test --doc"

# 6. All Tests Together
Run-TestCategory "Complete Test Suite" "cargo test --all"

# End timestamp
$endTime = Get-Date
$duration = ($endTime - $startTime).TotalSeconds

# Generate report
Write-Host ""
Write-Host "==========================================" -ForegroundColor Cyan
Write-Host "Test Summary" -ForegroundColor Cyan
Write-Host "==========================================" -ForegroundColor Cyan
Write-Host "Total Test Categories: $($script:TotalTests)"
Write-Host "Passed: $($script:PassedTests)" -ForegroundColor Green
Write-Host "Failed: $($script:FailedTests)" -ForegroundColor Red
Write-Host "Duration: $([math]::Round($duration, 2))s"
Write-Host ""

# Check if all tests passed
if ($script:FailedTests -eq 0) {
    Write-Host "==========================================" -ForegroundColor Green
    Write-Host "✓ ALL TESTS PASSED" -ForegroundColor Green
    Write-Host "==========================================" -ForegroundColor Green
    exit 0
} else {
    Write-Host "==========================================" -ForegroundColor Red
    Write-Host "✗ SOME TESTS FAILED" -ForegroundColor Red
    Write-Host "==========================================" -ForegroundColor Red
    exit 1
}


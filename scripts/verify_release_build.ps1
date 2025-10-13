# EasyHDR Release Build Verification Script
# This script verifies the release build meets all requirements for task 15.4

param(
    [string]$BinaryPath = "target\release\easyhdr.exe"
)

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "EasyHDR Release Build Verification" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""

$ErrorCount = 0
$WarningCount = 0

# Function to print test result
function Test-Result {
    param(
        [string]$TestName,
        [bool]$Passed,
        [string]$Message = "",
        [string]$Requirement = ""
    )
    
    if ($Passed) {
        Write-Host "[✓] $TestName" -ForegroundColor Green
        if ($Message) {
            Write-Host "    $Message" -ForegroundColor Gray
        }
    } else {
        Write-Host "[✗] $TestName" -ForegroundColor Red
        if ($Message) {
            Write-Host "    $Message" -ForegroundColor Yellow
        }
        $script:ErrorCount++
    }
    
    if ($Requirement) {
        Write-Host "    Requirement: $Requirement" -ForegroundColor DarkGray
    }
    Write-Host ""
}

# Test 1: Binary exists
Write-Host "1. Checking binary existence..." -ForegroundColor Yellow
if (Test-Path $BinaryPath) {
    Test-Result "Binary exists" $true "Found at: $BinaryPath" "11.4"
} else {
    Test-Result "Binary exists" $false "Binary not found at: $BinaryPath" "11.4"
    Write-Host "Please build the release binary first: cargo build --release" -ForegroundColor Red
    exit 1
}

# Test 2: Binary size
Write-Host "2. Checking binary size..." -ForegroundColor Yellow
$FileInfo = Get-Item $BinaryPath
$FileSizeMB = [math]::Round($FileInfo.Length / 1MB, 2)
$SizeOK = ($FileSizeMB -ge 2) -and ($FileSizeMB -le 5)

if ($SizeOK) {
    Test-Result "Binary size" $true "Size: $FileSizeMB MB (target: 2-5 MB)" "11.6"
} else {
    if ($FileSizeMB -lt 2) {
        Test-Result "Binary size" $true "Size: $FileSizeMB MB (smaller than expected, but acceptable)" "11.6"
    } else {
        Test-Result "Binary size" $false "Size: $FileSizeMB MB (exceeds 5 MB target)" "11.6"
    }
}

# Test 3: Check if it's a Windows executable
Write-Host "3. Checking file type..." -ForegroundColor Yellow
$FileType = (Get-Item $BinaryPath).Extension
if ($FileType -eq ".exe") {
    Test-Result "File type" $true "Valid Windows executable (.exe)" "11.3"
} else {
    Test-Result "File type" $false "Not a Windows executable: $FileType" "11.3"
}

# Test 4: Check for embedded icon
Write-Host "4. Checking embedded icon..." -ForegroundColor Yellow
try {
    $Icon = [System.Drawing.Icon]::ExtractAssociatedIcon($BinaryPath)
    if ($Icon) {
        Test-Result "Embedded icon" $true "Icon found in executable" "10.2, 10.5"
        $Icon.Dispose()
    } else {
        Test-Result "Embedded icon" $false "No icon found in executable" "10.2, 10.5"
    }
} catch {
    Test-Result "Embedded icon" $false "Error extracting icon: $_" "10.2, 10.5"
}

# Test 5: Check Windows subsystem (GUI vs Console)
Write-Host "5. Checking Windows subsystem..." -ForegroundColor Yellow
try {
    # Read PE header to check subsystem
    $FileStream = [System.IO.File]::OpenRead($BinaryPath)
    $BinaryReader = New-Object System.IO.BinaryReader($FileStream)
    
    # Read DOS header
    $dosSignature = $BinaryReader.ReadUInt16()
    if ($dosSignature -ne 0x5A4D) { # "MZ"
        throw "Invalid DOS signature"
    }
    
    # Seek to PE header offset
    $FileStream.Seek(0x3C, [System.IO.SeekOrigin]::Begin) | Out-Null
    $peHeaderOffset = $BinaryReader.ReadUInt32()
    
    # Seek to PE header
    $FileStream.Seek($peHeaderOffset, [System.IO.SeekOrigin]::Begin) | Out-Null
    $peSignature = $BinaryReader.ReadUInt32()
    if ($peSignature -ne 0x00004550) { # "PE\0\0"
        throw "Invalid PE signature"
    }
    
    # Skip COFF header (20 bytes)
    $FileStream.Seek(20, [System.IO.SeekOrigin]::Current) | Out-Null
    
    # Read optional header magic
    $magic = $BinaryReader.ReadUInt16()
    
    # Skip to subsystem field (offset varies by architecture)
    if ($magic -eq 0x010B) { # PE32
        $FileStream.Seek(66, [System.IO.SeekOrigin]::Current) | Out-Null
    } elseif ($magic -eq 0x020B) { # PE32+
        $FileStream.Seek(66, [System.IO.SeekOrigin]::Current) | Out-Null
    } else {
        throw "Unknown PE format"
    }
    
    $subsystem = $BinaryReader.ReadUInt16()
    
    $BinaryReader.Close()
    $FileStream.Close()
    
    # Subsystem values: 2 = GUI, 3 = Console
    if ($subsystem -eq 2) {
        Test-Result "Windows subsystem" $true "GUI subsystem (no console window)" "10.1, 10.6"
    } elseif ($subsystem -eq 3) {
        Test-Result "Windows subsystem" $false "Console subsystem (console window will appear)" "10.1, 10.6"
    } else {
        Test-Result "Windows subsystem" $false "Unknown subsystem: $subsystem" "10.1, 10.6"
    }
} catch {
    Test-Result "Windows subsystem" $false "Error checking subsystem: $_" "10.1, 10.6"
}

# Test 6: Check dependencies
Write-Host "6. Checking dependencies..." -ForegroundColor Yellow
try {
    # This is a basic check - for detailed analysis, use dumpbin or Dependency Walker
    $DllImports = @()
    
    # Try to get imported DLLs using dumpbin if available
    $DumpbinPath = (Get-Command dumpbin -ErrorAction SilentlyContinue).Source
    if ($DumpbinPath) {
        $DumpbinOutput = & dumpbin /imports $BinaryPath 2>&1 | Select-String "\.dll"
        $DllImports = $DumpbinOutput | ForEach-Object { $_.ToString().Trim() } | Select-Object -Unique
        
        # Check for non-system DLLs
        $NonSystemDlls = $DllImports | Where-Object { 
            $_ -notmatch "kernel32|user32|gdi32|advapi32|shell32|ole32|oleaut32|ntdll|msvcrt|ws2_32|bcrypt|crypt32|secur32|userenv|dwmapi|uxtheme|comctl32|comdlg32|winspool|version"
        }
        
        if ($NonSystemDlls.Count -eq 0) {
            Test-Result "Dependencies" $true "Only system DLLs detected" "11.4"
        } else {
            Test-Result "Dependencies" $false "Non-system DLLs found: $($NonSystemDlls -join ', ')" "11.4"
        }
    } else {
        Write-Host "    [!] dumpbin not found - skipping detailed dependency check" -ForegroundColor Yellow
        Write-Host "    Install Visual Studio Build Tools for detailed analysis" -ForegroundColor Gray
        $script:WarningCount++
    }
} catch {
    Write-Host "    [!] Error checking dependencies: $_" -ForegroundColor Yellow
    $script:WarningCount++
}

# Test 7: Check build configuration
Write-Host "7. Checking Cargo.toml build configuration..." -ForegroundColor Yellow
$CargoTomlPath = "Cargo.toml"
if (Test-Path $CargoTomlPath) {
    $CargoContent = Get-Content $CargoTomlPath -Raw
    
    # Check for release profile settings
    $HasLTO = $CargoContent -match 'lto\s*=\s*true'
    $HasStrip = $CargoContent -match 'strip\s*=\s*true'
    $HasOptLevel = $CargoContent -match 'opt-level\s*=\s*"z"'
    $HasCodegenUnits = $CargoContent -match 'codegen-units\s*=\s*1'
    $HasPanicAbort = $CargoContent -match 'panic\s*=\s*"abort"'
    
    Test-Result "LTO enabled" $HasLTO "" "11.2"
    Test-Result "Strip symbols" $HasStrip "" "11.1"
    Test-Result "Size optimization" $HasOptLevel "" "11.1"
    Test-Result "Codegen units = 1" $HasCodegenUnits "" "11.2"
    Test-Result "Panic = abort" $HasPanicAbort "" "11.1"
} else {
    Test-Result "Cargo.toml check" $false "Cargo.toml not found" ""
}

# Test 8: Check for required assets
Write-Host "8. Checking required assets..." -ForegroundColor Yellow
$RequiredAssets = @(
    "assets\icon.ico",
    "assets\icon_hdr_on.ico",
    "assets\icon_hdr_off.ico"
)

foreach ($Asset in $RequiredAssets) {
    if (Test-Path $Asset) {
        Test-Result "Asset: $Asset" $true "" "10.2, 10.5"
    } else {
        Test-Result "Asset: $Asset" $false "Asset not found" "10.2, 10.5"
    }
}

# Test 9: Check build.rs configuration
Write-Host "9. Checking build.rs configuration..." -ForegroundColor Yellow
$BuildRsPath = "build.rs"
if (Test-Path $BuildRsPath) {
    $BuildRsContent = Get-Content $BuildRsPath -Raw
    
    $HasSlintBuild = $BuildRsContent -match 'slint_build::compile'
    $HasIconEmbed = $BuildRsContent -match 'set_icon'
    $HasVersionInfo = $BuildRsContent -match 'ProductName|FileDescription'
    
    Test-Result "Slint compilation" $HasSlintBuild "" "11.5"
    Test-Result "Icon embedding" $HasIconEmbed "" "10.2"
    Test-Result "Version info" $HasVersionInfo "" "11.5"
} else {
    Test-Result "build.rs check" $false "build.rs not found" ""
}

# Summary
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "Verification Summary" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""

if ($ErrorCount -eq 0 -and $WarningCount -eq 0) {
    Write-Host "✓ All checks passed!" -ForegroundColor Green
    Write-Host ""
    Write-Host "Next steps:" -ForegroundColor Cyan
    Write-Host "1. Test on Windows 10 21H2" -ForegroundColor White
    Write-Host "2. Test on Windows 11 21H2" -ForegroundColor White
    Write-Host "3. Test on Windows 11 24H2" -ForegroundColor White
    Write-Host "4. Test with HDR and non-HDR displays" -ForegroundColor White
    Write-Host "5. Test with multiple monitors" -ForegroundColor White
    Write-Host ""
    Write-Host "See RELEASE_TEST_PLAN.md for detailed test procedures" -ForegroundColor Gray
    exit 0
} elseif ($ErrorCount -eq 0) {
    Write-Host "⚠ Checks passed with $WarningCount warning(s)" -ForegroundColor Yellow
    Write-Host ""
    Write-Host "Review warnings above and proceed with testing if acceptable." -ForegroundColor Yellow
    exit 0
} else {
    Write-Host "✗ $ErrorCount error(s) and $WarningCount warning(s) found" -ForegroundColor Red
    Write-Host ""
    Write-Host "Please fix the errors above before proceeding with testing." -ForegroundColor Red
    exit 1
}


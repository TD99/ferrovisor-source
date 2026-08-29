$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$previousCargoHome = $env:CARGO_HOME
Push-Location $root
try {
    if ($args.Count -eq 0) {
        $target = "baremetal"
    }
    elseif ($args.Count -eq 2 -and $args[0] -eq "--target") {
        $target = $args[1]
    }
    else {
        throw "usage: .\scripts\build.ps1 [--target baremetal|vmware|hyperv]"
    }
    if ($target -notin "baremetal", "vmware", "hyperv") {
        throw "--target must be baremetal, vmware, or hyperv"
    }

    foreach ($tool in "rustup", "cargo", "cl", "lib") {
        if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
            throw "$tool is required. Install Rust with rustup and the Visual Studio C++ build tools, then reopen PowerShell."
        }
    }
    if ($target -eq "hyperv" -and -not (Get-Command qemu-img -ErrorAction SilentlyContinue)) {
        throw "qemu-img is required for the hyperv target. Install QEMU and add it to PATH."
    }

    $shimDir = Join-Path $root "target\uefi-shim"
    New-Item -ItemType Directory -Force -Path $shimDir | Out-Null
    & cl /nologo /c /TC /O2 /Oi- /GS- "build\uefi_shim.c" "/Fo$shimDir\uefi_shim.obj"
    if ($LASTEXITCODE -ne 0) { throw "failed to compile the UEFI compatibility shim" }
    & lib /nologo "/OUT:$shimDir\libferrovisor_uefi_shim.a" "$shimDir\uefi_shim.obj"
    if ($LASTEXITCODE -ne 0) { throw "failed to archive the UEFI compatibility shim" }

    rustup show active-toolchain
    $cargoHome = Join-Path $root "target\cargo-home"
    New-Item -ItemType Directory -Force -Path $cargoHome | Out-Null
    @"
[target.x86_64-unknown-uefi]
rustflags = ["-Lnative=$($shimDir.Replace('\', '/'))", "-lstatic=ferrovisor_uefi_shim"]
"@ | Set-Content -LiteralPath (Join-Path $cargoHome "config.toml") -NoNewline
    $env:CARGO_HOME = $cargoHome
    cargo run --release -- --target $target
    if ($LASTEXITCODE -ne 0) { throw "Cargo failed to build Ferrovisor" }

    $images = @("dist\ferrovisor-bios.img", "dist\ferrovisor-uefi.img")
    if ($target -eq "vmware") {
        $images += "dist\ferrovisor-bios.vmdk", "dist\ferrovisor-bios.vmx"
        $images += "dist\ferrovisor-uefi.vmdk", "dist\ferrovisor-uefi.vmx"
    }
    elseif ($target -eq "hyperv") {
        $images += "dist\ferrovisor-bios.vhdx", "dist\ferrovisor-uefi.vhdx"
    }
    $missing = $images | Where-Object { -not (Test-Path -LiteralPath $_ -PathType Leaf) }
    if ($missing) {
        throw "build completed without required artifacts: $($missing -join ', ')"
    }

    Write-Host ""
    Write-Host "Images are ready in .\dist"
    if ($target -eq "vmware") {
        Write-Host "Open .\dist\ferrovisor-bios.vmx in VMware Workstation."
    }
    elseif ($target -eq "hyperv") {
        Write-Host "Attach .\dist\ferrovisor-bios.vhdx to a Hyper-V Generation 1 VM."
    }
}
finally {
    $env:CARGO_HOME = $previousCargoHome
    Pop-Location
}

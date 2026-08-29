#requires -RunAsAdministrator
[CmdletBinding()]
param(
    [string]$VMName = "ferrovisor-bios"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$diskPath = Join-Path $root "dist\ferrovisor-bios.vhdx"
$vm = Get-VM -Name $VMName

if ($vm.Generation -ne 1) {
    throw "$VMName must be a Generation 1 VM using ferrovisor-bios.vhdx"
}

$attachedDisk = @(
    Get-VMHardDiskDrive -VMName $VMName | Where-Object {
        [IO.Path]::GetFullPath($_.Path) -ieq $diskPath
    }
)
if ($attachedDisk.Count -gt 1) {
    throw "expected at most one attachment of $diskPath on $VMName"
}
if ($attachedDisk.Count -eq 1) {
    $controllerType = $attachedDisk[0].ControllerType
    $controllerNumber = $attachedDisk[0].ControllerNumber
    $controllerLocation = $attachedDisk[0].ControllerLocation
}
else {
    # Generation 1 VMs boot VHDX images from the IDE bus.  Do not assume IDE
    # 0:0 is free: New-VM commonly puts its initial disk there, and a DVD
    # drive can occupy an IDE slot as well.
    $ideSlots = @(
        @{ ControllerNumber = 0; ControllerLocation = 0 }
        @{ ControllerNumber = 0; ControllerLocation = 1 }
        @{ ControllerNumber = 1; ControllerLocation = 0 }
        @{ ControllerNumber = 1; ControllerLocation = 1 }
    )
    $occupiedIdeSlots = @(
        @(Get-VMHardDiskDrive -VMName $VMName)
        @(Get-VMDvdDrive -VMName $VMName)
    ) | Where-Object { $_.ControllerType -ieq "IDE" } |
        ForEach-Object { "$($_.ControllerNumber):$($_.ControllerLocation)" }

    $freeIdeSlot = $ideSlots |
        Where-Object {
            "$($_.ControllerNumber):$($_.ControllerLocation)" -notin $occupiedIdeSlots
        } |
        Select-Object -First 1

    if ($null -eq $freeIdeSlot) {
        throw "No free IDE slot is available on $VMName. Remove an unused IDE disk or DVD drive and retry."
    }

    $controllerType = "IDE"
    $controllerNumber = $freeIdeSlot.ControllerNumber
    $controllerLocation = $freeIdeSlot.ControllerLocation
}

if ($vm.State -ne "Off") {
    Stop-VM -Name $VMName -TurnOff
    do {
        Start-Sleep -Milliseconds 250
        $vm = Get-VM -Name $VMName
    } while ($vm.State -ne "Off")
}

$detached = $false

if ($attachedDisk.Count -eq 1) {
    Remove-VMHardDiskDrive -VMName $VMName -ControllerType $controllerType `
        -ControllerNumber $controllerNumber -ControllerLocation $controllerLocation
    $detached = $true
}

try {
    & (Join-Path $PSScriptRoot "build.ps1") --target hyperv
    if ($LASTEXITCODE -ne 0) {
        throw "Hyper-V image build failed"
    }

    Add-VMHardDiskDrive -VMName $VMName -ControllerType $controllerType `
        -ControllerNumber $controllerNumber -ControllerLocation $controllerLocation `
        -Path $diskPath
    $detached = $false
    Set-VMBios -VMName $VMName -StartupOrder IDE, CD, Floppy, LegacyNetworkAdapter
    Write-Host "Rebuilt, attached, and selected $diskPath as the IDE boot disk for $VMName."
}
catch {
    if ($detached -and (Test-Path -LiteralPath $diskPath -PathType Leaf)) {
        Add-VMHardDiskDrive -VMName $VMName -ControllerType $controllerType `
            -ControllerNumber $controllerNumber -ControllerLocation $controllerLocation `
            -Path $diskPath
    }
    throw
}

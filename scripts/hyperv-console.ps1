param(
    [string]$PipeName = "ferrovisor-com1"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$pipe = [System.IO.Pipes.NamedPipeClientStream]::new(
    ".",
    $PipeName,
    [System.IO.Pipes.PipeDirection]::InOut,
    [System.IO.Pipes.PipeOptions]::None
)

try {
    Write-Host "Waiting for \\.\pipe\$PipeName..."
    $pipe.Connect()
    Write-Host "Connected. Restart the VM if it already booted to see its complete output."

    while (($byte = $pipe.ReadByte()) -ne -1) {
        [Console]::Write([char]$byte)
    }
}
finally {
    $pipe.Dispose()
}

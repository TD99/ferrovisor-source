param(
    [string]$PipeName = "ferrovisor-com1",
    [ValidateRange(1, 3600)]
    [int]$TimeoutSeconds = 60
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
    $pipe.Connect($TimeoutSeconds * 1000)
    Write-Host "Connected. Restart the VM if it already booted to see its complete output."

    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    $buffer = [byte[]]::new(1)
    while ([DateTime]::UtcNow -lt $deadline) {
        $read = $pipe.ReadAsync($buffer, 0, 1)
        if (-not $read.Wait(250)) {
            continue
        }
        if ($read.Result -eq 0) {
            break
        }
        [Console]::Write([char]$buffer[0])
    }
    Write-Host "`nConsole timeout reached."
}
finally {
    $pipe.Dispose()
}

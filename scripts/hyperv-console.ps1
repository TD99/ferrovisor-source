param(
    [string]$PipeName = "ferrovisor-com1",
    [ValidateRange(1, 3600)]
    [int]$TimeoutSeconds = 60,
    [switch]$NoTimeout,
    [string[]]$Send
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
    Write-Host "Connected. Type into this terminal to send text to the Tiny64 guest."

    $encoding = [Text.Encoding]::ASCII
    foreach ($command in $Send) {
        $bytes = $encoding.GetBytes("$command`n")
        $pipe.Write($bytes, 0, $bytes.Length)
    }
    $pipe.Flush()

    $deadline = if ($NoTimeout) { [DateTime]::MaxValue } else { [DateTime]::UtcNow.AddSeconds($TimeoutSeconds) }
    $buffer = [byte[]]::new(1)
    $read = $pipe.ReadAsync($buffer, 0, 1)
    while ([DateTime]::UtcNow -lt $deadline) {
        if ($read.IsCompleted) {
            if ($read.GetAwaiter().GetResult() -eq 0) {
                break
            }
            [Console]::Write([char]$buffer[0])
            $read = $pipe.ReadAsync($buffer, 0, 1)
        }

        while ([Console]::KeyAvailable) {
            $key = [Console]::ReadKey($true)
            $byte = switch ($key.Key) {
                "Enter" { 10; break }
                "Backspace" { 8; break }
                default {
                    $code = [int][char]$key.KeyChar
                    if ($code -ge 32 -and $code -le 126) { [byte]$code }
                }
            }
            if ($null -ne $byte) {
                $pipe.WriteByte($byte)
                $pipe.Flush()
            }
        }
        Start-Sleep -Milliseconds 10
    }
    if (-not $NoTimeout) {
        Write-Host "`nConsole timeout reached."
    }
}
finally {
    $pipe.Dispose()
}

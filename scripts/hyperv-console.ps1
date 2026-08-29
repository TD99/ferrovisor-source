param(
    [string]$PipeName = "ferrovisor-com1",
    [ValidateRange(0, 3600)]
    [int]$TimeoutSeconds = 0,
    [switch]$NoTimeout,
    [string[]]$Send
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$pipe = [System.IO.Pipes.NamedPipeClientStream]::new(
    ".",
    $PipeName,
    [System.IO.Pipes.PipeDirection]::InOut,
    [System.IO.Pipes.PipeOptions]::Asynchronous
)

try {
    Write-Host "Waiting for \\.\pipe\$PipeName..."
    if ($NoTimeout -or $TimeoutSeconds -eq 0) {
        $pipe.Connect()
    }
    else {
        $pipe.Connect($TimeoutSeconds * 1000)
    }
    Write-Host "Connected. Type into this terminal to send text to the Tiny64 guest."

    $encoding = [Text.Encoding]::ASCII
    foreach ($command in $Send) {
        $bytes = $encoding.GetBytes("$command`n")
        $pipe.Write($bytes, 0, $bytes.Length)
    }
    $pipe.Flush()

    $deadline = if ($NoTimeout -or $TimeoutSeconds -eq 0) { [DateTime]::MaxValue } else { [DateTime]::UtcNow.AddSeconds($TimeoutSeconds) }
    $buffer = [byte[]]::new(4096)
    $read = $pipe.ReadAsync($buffer, 0, $buffer.Length)
    while ([DateTime]::UtcNow -lt $deadline) {
        if ($read.IsCompleted) {
            $count = $read.GetAwaiter().GetResult()
            if ($count -eq 0) {
                break
            }
            [Console]::Write($encoding.GetString($buffer, 0, $count))
            $read = $pipe.ReadAsync($buffer, 0, $buffer.Length)
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
    if (-not $NoTimeout -and $TimeoutSeconds -ne 0) {
        Write-Host "`nConsole timeout reached."
    }
}
finally {
    $pipe.Dispose()
}

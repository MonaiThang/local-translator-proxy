<#
.SYNOPSIS
  Drive the real local-translator-proxy binary and prove one user-facing
  behavior, capturing evidence under target/verification/.

.DESCRIPTION
  Starts the built proxy on an isolated loopback port, points it at the
  deterministic Model Runtime stub, exercises one scenario over HTTP the way
  the game does, asserts the response bytes and the model-call count, writes
  evidence, and stops only the processes it started.

.PARAMETER Scenario
  doctor | translate | multi-line-tokens | passthrough-mismatch |
  passthrough-injected-newline | passthrough-timeout | passthrough-runtime-down |
  no-model-call

.PARAMETER ProxyPort
  Loopback port for the proxy. Default 5099 (never the game's 5000).

.PARAMETER StubPort
  Loopback port for the Model Runtime stub. Default 7331.

.EXAMPLE
  pwsh -File .opencode/skills/verify-local-translator-proxy/scripts/verify.ps1 -Scenario translate
#>
param(
    [ValidateSet(
        'doctor',
        'translate',
        'multi-line-tokens',
        'passthrough-mismatch',
        'passthrough-injected-newline',
        'passthrough-timeout',
        'passthrough-runtime-down',
        'no-model-call'
    )]
    [string]$Scenario = 'translate',
    [int]$ProxyPort = 5099,
    [int]$StubPort = 7331
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..\..')).Path
$exe = Join-Path $repo 'target\debug\local-translator-proxy.exe'
$stubScript = Join-Path $PSScriptRoot 'runtime-stub.mjs'

$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$evidence = Join-Path $repo "target\verification\$stamp-$Scenario"
New-Item -ItemType Directory -Force -Path $evidence | Out-Null

$proxy = $null
$stub = $null
$checks = [System.Collections.Generic.List[object]]::new()
$requestIndex = 0

function Add-Check([string]$Name, [bool]$Pass, [string]$Detail) {
    $checks.Add([pscustomobject]@{ name = $Name; passed = $Pass; detail = $Detail })
    $label = if ($Pass) { 'PASS' } else { 'FAIL' }
    Write-Host ("[{0}] {1}: {2}" -f $label, $Name, $Detail)
}

function Assert-PortFree([int]$Port, [string]$Role) {
    $listeners = @(Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue)
    if ($listeners.Count -gt 0) {
        throw "$Role port $Port is already in use. Pass a different -ProxyPort or -StubPort."
    }
}

function Wait-Until([scriptblock]$Probe, [int]$Seconds = 20) {
    $deadline = (Get-Date).AddSeconds($Seconds)
    while ((Get-Date) -lt $deadline) {
        try {
            if (& $Probe) { return $true }
        } catch {
            # Not ready yet.
        }
        Start-Sleep -Milliseconds 150
    }
    return $false
}

function Invoke-Proxy([string]$Text, [string]$From = 'zh-CN', [string]$To = 'en') {
    $url = "http://127.0.0.1:$ProxyPort/?text=$([uri]::EscapeDataString($Text))&from=$From&to=$To"
    $client = [System.Net.Http.HttpClient]::new()
    $client.Timeout = [TimeSpan]::FromSeconds(30)
    try {
        $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
        $response = $client.GetAsync($url).GetAwaiter().GetResult()
        $bytes = $response.Content.ReadAsByteArrayAsync().GetAwaiter().GetResult()
        $stopwatch.Stop()

        $script:requestIndex++
        $contentType = if ($null -eq $response.Content.Headers.ContentType) {
            ''
        } else {
            $response.Content.Headers.ContentType.ToString()
        }
        $record = [pscustomobject]@{
            request = $script:requestIndex
            url = $url
            status = [int]$response.StatusCode
            contentType = $contentType
            elapsedMs = $stopwatch.ElapsedMilliseconds
            bodyUtf8 = [System.Text.Encoding]::UTF8.GetString($bytes)
            bodyHex = [Convert]::ToHexString($bytes)
        }
        [IO.File]::WriteAllBytes((Join-Path $evidence "body-$($script:requestIndex).bin"), $bytes)
        $record | ConvertTo-Json -Depth 4 | Set-Content -Path (Join-Path $evidence "response-$($script:requestIndex).json")
        return $record
    } finally {
        $client.Dispose()
    }
}

function Get-StubRequests {
    @(Invoke-RestMethod -Uri "http://127.0.0.1:$StubPort/requests" -TimeoutSec 10)
}

# Scenario setup: stub mode, stub delay, proxy deadline, and whether a stub runs.
$stubMode = 'translate'
$stubDelayMs = 0
$timeoutSecs = 5
$runStub = $true

switch ($Scenario) {
    'doctor' { }
    'translate' { }
    'multi-line-tokens' { }
    'passthrough-mismatch' { $stubMode = 'mismatch' }
    'passthrough-injected-newline' { $stubMode = 'inject' }
    'passthrough-timeout' { $stubDelayMs = 3000; $timeoutSecs = 1 }
    'passthrough-runtime-down' { $runStub = $false; $timeoutSecs = 2 }
    'no-model-call' { }
    default { throw "unhandled scenario $Scenario" }
}

try {
    Assert-PortFree $ProxyPort 'Proxy'

    $env:PATH = "C:\msys64\mingw64\bin;" + $env:PATH
    & cargo +stable-x86_64-pc-windows-gnu build 2>&1 |
        Tee-Object -FilePath (Join-Path $evidence 'build.log')
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed; see build.log' }
    if (-not (Test-Path $exe)) { throw "proxy binary not found at $exe" }

    if ($runStub) {
        Assert-PortFree $StubPort 'Model Runtime stub'
        $env:STUB_PORT = "$StubPort"
        $env:STUB_MODE = $stubMode
        $env:STUB_DELAY_MS = "$stubDelayMs"
        $stub = Start-Process -FilePath 'node' -ArgumentList @($stubScript) -PassThru -NoNewWindow `
            -RedirectStandardOutput (Join-Path $evidence 'stub.log') `
            -RedirectStandardError (Join-Path $evidence 'stub.err.log')
        $stubReady = Wait-Until { (Invoke-RestMethod -Uri "http://127.0.0.1:$StubPort/health" -TimeoutSec 2).ok }
        if (-not $stubReady) { throw 'Model Runtime stub did not become ready; see stub.log' }
    } else {
        Assert-PortFree $StubPort 'Closed runtime endpoint'
    }

    $env:TRANSLATOR_PROXY_SERVER_LISTEN_ADDR = "127.0.0.1:$ProxyPort"
    $env:TRANSLATOR_PROXY_MODEL_RUNTIME_ENDPOINT = "http://127.0.0.1:$StubPort/api/chat"
    $env:TRANSLATOR_PROXY_MODEL_RUNTIME_TIMEOUT_SECS = "$timeoutSecs"
    $env:TRANSLATOR_PROXY_MODEL_RUNTIME_MAX_IN_FLIGHT = '4'
    $env:RUST_LOG = 'info'

    $proxy = Start-Process -FilePath $exe -WorkingDirectory $repo -PassThru -NoNewWindow `
        -RedirectStandardOutput (Join-Path $evidence 'proxy.log') `
        -RedirectStandardError (Join-Path $evidence 'proxy.err.log')

    # The literal {text} placeholder never reaches the model, so it is a safe
    # readiness probe and does not disturb the model-call counter.
    $proxyReady = Wait-Until {
        $probe = Invoke-Proxy '{text}'
        ($probe.status -eq 200) -and ($probe.contentType -like 'text/plain*')
    }
    if (-not $proxyReady) { throw 'proxy did not become ready; see proxy.log' }

    switch ($Scenario) {
        'doctor' {
            $probe = Invoke-Proxy '{text}'
            Add-Check 'proxy answers 200' ($probe.status -eq 200) "status=$($probe.status)"
            Add-Check 'content type is plain UTF-8 text' ($probe.contentType -like 'text/plain*charset=utf-8*') "contentType=$($probe.contentType)"
            Add-Check 'placeholder returns unchanged' ($probe.bodyUtf8 -ceq '{text}') "body=$($probe.bodyUtf8)"
            if ($runStub) {
                $health = Invoke-RestMethod -Uri "http://127.0.0.1:$StubPort/health" -TimeoutSec 5
                Add-Check 'stub reports its mode' ($health.ok -and $health.mode -eq $stubMode) "mode=$($health.mode)"
            }
            Add-Check 'doctor made zero model calls' ((Get-StubRequests).Count -eq 0) "modelCalls=$((Get-StubRequests).Count)"
        }
        'translate' {
            $result = Invoke-Proxy '你好'
            Add-Check 'status is 200' ($result.status -eq 200) "status=$($result.status)"
            Add-Check 'content type is plain UTF-8 text' ($result.contentType -like 'text/plain*charset=utf-8*') "contentType=$($result.contentType)"
            Add-Check 'translated body' ($result.bodyUtf8 -ceq 'EN:你好') "body=$($result.bodyUtf8)"
            Add-Check 'one model call' ((Get-StubRequests).Count -eq 1) "modelCalls=$((Get-StubRequests).Count)"
        }
        'multi-line-tokens' {
            $source = "你好`r`n<b>世界</b>"
            $result = Invoke-Proxy $source
            $expected = "EN:你好`r`nEN:<b>世界</b>"
            Add-Check 'status is 200' ($result.status -eq 200) "status=$($result.status)"
            Add-Check 'CRLF separator is preserved' ($result.bodyUtf8 -ceq $expected) "bodyHex=$($result.bodyHex)"
            Add-Check 'two model calls, one per non-blank line' ((Get-StubRequests).Count -eq 2) "modelCalls=$((Get-StubRequests).Count)"
        }
        'passthrough-mismatch' {
            $result = Invoke-Proxy '<b>你好</b>'
            Add-Check 'status is 200' ($result.status -eq 200) "status=$($result.status)"
            Add-Check 'dropped token yields the original bytes' ($result.bodyUtf8 -ceq '<b>你好</b>') "body=$($result.bodyUtf8)"
            Add-Check 'one model call was attempted' ((Get-StubRequests).Count -eq 1) "modelCalls=$((Get-StubRequests).Count)"
        }
        'passthrough-injected-newline' {
            $result = Invoke-Proxy '你好'
            Add-Check 'status is 200' ($result.status -eq 200) "status=$($result.status)"
            Add-Check 'injected newline yields the original bytes' ($result.bodyUtf8 -ceq '你好') "body=$($result.bodyUtf8)"
            Add-Check 'no newline in the response' (-not $result.bodyUtf8.Contains("`n")) "bodyHex=$($result.bodyHex)"
        }
        'passthrough-timeout' {
            $result = Invoke-Proxy '你好'
            Add-Check 'status is 200' ($result.status -eq 200) "status=$($result.status)"
            Add-Check 'timeout yields the original bytes' ($result.bodyUtf8 -ceq '你好') "body=$($result.bodyUtf8)"
            Add-Check 'whole unit respects the 1s deadline' ($result.elapsedMs -lt 2500) "elapsedMs=$($result.elapsedMs)"
        }
        'passthrough-runtime-down' {
            $result = Invoke-Proxy '你好'
            Add-Check 'status is 200' ($result.status -eq 200) "status=$($result.status)"
            Add-Check 'unreachable runtime yields the original bytes' ($result.bodyUtf8 -ceq '你好') "body=$($result.bodyUtf8)"
            Add-Check 'content type is plain UTF-8 text' ($result.contentType -like 'text/plain*charset=utf-8*') "contentType=$($result.contentType)"
        }
        'no-model-call' {
            $placeholder = Invoke-Proxy '{text}'
            Add-Check 'placeholder returns unchanged' ($placeholder.bodyUtf8 -ceq '{text}') "body=$($placeholder.bodyUtf8)"

            $blank = Invoke-Proxy '   '
            Add-Check 'whitespace-only unit returns unchanged' ($blank.bodyUtf8 -ceq '   ') "bodyHex=$($blank.bodyHex)"

            $unsupported = Invoke-Proxy '你好' 'ja' 'en'
            Add-Check 'unsupported pair returns unchanged' ($unsupported.bodyUtf8 -ceq '你好') "body=$($unsupported.bodyUtf8)"

            $diffTarget = Invoke-Proxy '你好' 'zh-CN' 'fr'
            Add-Check 'unsupported target returns unchanged' ($diffTarget.bodyUtf8 -ceq '你好') "body=$($diffTarget.bodyUtf8)"

            Add-Check 'zero model calls' ((Get-StubRequests).Count -eq 0) "modelCalls=$((Get-StubRequests).Count)"
        }
    }
} catch {
    Add-Check 'run completed without error' $false $_.Exception.Message
} finally {
    foreach ($process in @($proxy, $stub)) {
        if ($null -ne $process) {
            try {
                if (-not $process.HasExited) { Stop-Process -Id $process.Id -Force -ErrorAction Stop }
                Wait-Process -Id $process.Id -Timeout 5 -ErrorAction SilentlyContinue
            } catch {
                Write-Warning "could not stop PID $($process.Id): $($_.Exception.Message)"
            }
        }
    }

    $failed = @($checks | Where-Object { -not $_.passed })
    $summary = [pscustomobject]@{
        scenario = $Scenario
        passed = ($failed.Count -eq 0) -and ($checks.Count -gt 0)
        checks = $checks
        proxyPort = $ProxyPort
        stubPort = $StubPort
        evidence = $evidence
    }
    $summary | ConvertTo-Json -Depth 6 | Set-Content -Path (Join-Path $evidence 'summary.json')

    # Evidence is proof: it survives teardown. Only the processes this run
    # started are stopped.
    Write-Host "Evidence: $evidence"
    if ($failed.Count -gt 0 -or $checks.Count -eq 0) { exit 1 }
}

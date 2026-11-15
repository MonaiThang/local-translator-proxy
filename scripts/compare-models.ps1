#Requires -Version 7.4

param(
    [string[]]$Models = @('translategemma:4b'),
    [string]$CorpusPath = 'corpus/zh-en.json',
    [int]$WarmupPasses = 1,
    [int]$MeasuredPasses = 5,
    [int]$PortBase = 5101,
    [int]$TimeoutSecs = 5,
    [string]$OutputDir = 'target/model-comparison',
    [string]$ProxyExe = 'target/debug/local-translator-proxy.exe',
    [string]$OllamaEndpoint = 'http://127.0.0.1:11434'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot

$knownCategories = @(
    'combat', 'colour', 'dialogue', 'item', 'line-break',
    'markup', 'punctuation', 'square-bracket', 'ui'
)
$requiredCategories = @('colour', 'dialogue', 'line-break', 'markup', 'punctuation', 'square-bracket')

$reasonMarkers = [ordered]@{
    'timeout'             = 'translation unit timed out'
    'runtime error'       = 'failure=runtime error:'
    'token mismatch'      = 'reason=rendering token mismatch'
    'malformed token'     = 'reason=malformed rendering token'
    'injected line break' = 'reason=translation injected a line break'
    'language pair'       = 'unsupported language pair'
}

function Resolve-RepoPath([string]$Path) {
    if ([System.IO.Path]::IsPathRooted($Path)) { return $Path }
    return (Join-Path $repoRoot $Path)
}

function Assert-PreservedInOrder {
    param([string]$Id, [string]$Label, [string]$Text, [string[]]$Tokens)
    $cursor = 0
    foreach ($token in $Tokens) {
        $index = $Text.IndexOf($token, $cursor, [System.StringComparison]::Ordinal)
        if ($index -lt 0) {
            throw "entry '${Id}': ${Label} does not contain '$token' in order"
        }
        $cursor = $index + $token.Length
    }
}

function Test-Corpus {
    param([object]$Corpus)
    if ($Corpus.version -ne 1) { throw "corpus version is $($Corpus.version), expected 1" }
    if ($Corpus.from -cne 'zh-CN') { throw "corpus from is '$($Corpus.from)', expected 'zh-CN'" }
    if ($Corpus.to -cne 'en') { throw "corpus to is '$($Corpus.to)', expected 'en'" }
    if ($Corpus.entries.Count -lt 20) {
        throw "corpus has $($Corpus.entries.Count) entries, expected at least 20"
    }

    $seenIds = @{}
    $coveredCategories = @{}
    foreach ($entry in $Corpus.entries) {
        $id = $entry.id
        if ($seenIds.ContainsKey($id)) { throw "entry '${id}': duplicate id" }
        $seenIds[$id] = $true

        if ($knownCategories -notcontains $entry.category) {
            throw "entry '${id}': unknown category '$($entry.category)'"
        }
        $coveredCategories[$entry.category] = $true

        if (-not $entry.source) { throw "entry '${id}': source is empty" }
        if (-not $entry.reference) { throw "entry '${id}': reference is empty" }
        if ($entry.source.Contains("`r") -or $entry.source.Contains("`n")) {
            throw "entry '${id}': source contains a real CR or LF; a corpus entry is one physical line"
        }
        if ($entry.reference.Contains("`r") -or $entry.reference.Contains("`n")) {
            throw "entry '${id}': reference contains a real CR or LF"
        }

        $preserve = @($entry.must_preserve)
        Assert-PreservedInOrder -Id $id -Label 'source' -Text $entry.source -Tokens $preserve
        Assert-PreservedInOrder -Id $id -Label 'reference' -Text $entry.reference -Tokens $preserve
    }

    foreach ($category in $requiredCategories) {
        if (-not $coveredCategories.ContainsKey($category)) {
            throw "corpus has no '$category' entry"
        }
    }
}

function Assert-ModelsInstalled {
    param([string[]]$Requested, [string]$Endpoint)
    try {
        $tags = Invoke-RestMethod -Uri "$Endpoint/api/tags" -TimeoutSec 15
    } catch {
        throw "cannot reach the Model Runtime at $Endpoint/api/tags. Is Ollama running? ($($_.Exception.Message))"
    }
    $installed = @($tags.models.name) + @($tags.models.model)
    foreach ($model in $Requested) {
        if ($installed -notcontains $model) {
            throw "model '$model' is not installed on $Endpoint. Run: ollama pull $model"
        }
    }
}

function Assert-ProxyBuilt {
    param([string]$Exe)
    $needsBuild = -not (Test-Path $Exe -PathType Leaf)
    if (-not $needsBuild) {
        $builtAt = (Get-Item $Exe).LastWriteTime
        $inputs = @(Get-ChildItem (Join-Path $repoRoot 'src') -Recurse -File) +
            @(Get-Item (Join-Path $repoRoot 'Cargo.toml')) +
            @(Get-Item (Join-Path $repoRoot 'Cargo.lock'))
        $needsBuild = [bool]($inputs | Where-Object { $_.LastWriteTime -gt $builtAt })
    }
    if (-not $needsBuild) { return }

    $previousPath = $env:PATH
    try {
        $env:PATH = "C:\msys64\mingw64\bin;$previousPath"
        $output = & cargo +stable-x86_64-pc-windows-gnu build 2>&1
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build failed:`n$($output | Out-String)"
        }
    } finally {
        $env:PATH = $previousPath
    }
}

function Get-SubstringCount {
    param([string]$Text, [string]$Needle)
    $count = 0
    $index = 0
    while (($index = $Text.IndexOf($Needle, $index, [System.StringComparison]::Ordinal)) -ge 0) {
        $count++
        $index += $Needle.Length
    }
    return $count
}

function Get-LogSlice {
    param([string]$Path, [long]$Offset)
    if (-not (Test-Path $Path -PathType Leaf)) { return '' }
    $stream = [System.IO.File]::Open(
        $Path, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]::ReadWrite)
    try {
        if ($Offset -ge $stream.Length) { return '' }
        $stream.Position = $Offset
        $reader = [System.IO.StreamReader]::new($stream, [System.Text.Encoding]::UTF8, $false, 4096, $true)
        try { return $reader.ReadToEnd() } finally { $reader.Dispose() }
    } finally {
        $stream.Dispose()
    }
}

function Wait-ProxyReady {
    param(
        [System.Net.Http.HttpClient]$Client,
        [string]$BaseUrl,
        [System.Diagnostics.Process]$Process,
        [string]$Model
    )
    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    while ([DateTime]::UtcNow -lt $deadline) {
        if ($Process.HasExited) {
            throw "proxy for '$Model' exited during startup with code $($Process.ExitCode)"
        }
        try {
            $body = $Client.GetStringAsync("$BaseUrl/?text=%7Btext%7D").GetAwaiter().GetResult()
            if ($body -eq '{text}') { return }
        } catch { }
        Start-Sleep -Milliseconds 200
    }
    throw "proxy for '$Model' on $BaseUrl was not ready within 15 seconds"
}

function Get-Median {
    param([double[]]$Values)
    if ($Values.Count -eq 0) { throw 'median of an empty set' }
    $sorted = $Values | Sort-Object
    $count = $sorted.Count
    if ($count % 2 -eq 1) { return [double]$sorted[[int](($count - 1) / 2)] }
    return ([double]$sorted[$count / 2 - 1] + [double]$sorted[$count / 2]) / 2
}

function Get-PreservedStatus {
    param([string]$Text, [string[]]$Tokens)
    $cursor = 0
    foreach ($token in $Tokens) {
        $index = $Text.IndexOf($token, $cursor, [System.StringComparison]::Ordinal)
        if ($index -lt 0) { return "missing $token" }
        $cursor = $index + $token.Length
    }
    return 'ok'
}

function Invoke-ModelRun {
    param(
        [string]$Model,
        [int]$Port,
        [object]$Corpus,
        [string]$ProxyExe,
        [string]$RuntimeEndpoint,
        [string]$LogDirectory
    )

    $modelSlug = $Model -replace '[^A-Za-z0-9._-]', '_'
    $stdoutLog = Join-Path $LogDirectory "$modelSlug.out.log"
    $stderrLog = Join-Path $LogDirectory "$modelSlug.err.log"
    Remove-Item $stdoutLog, $stderrLog -ErrorAction SilentlyContinue

    $listenAddr = "127.0.0.1:$Port"
    $process = Start-Process -FilePath $ProxyExe -WorkingDirectory $repoRoot -PassThru `
        -WindowStyle Hidden `
        -RedirectStandardOutput $stdoutLog -RedirectStandardError $stderrLog `
        -Environment @{
        TRANSLATOR_PROXY_SERVER_LISTEN_ADDR         = $listenAddr
        TRANSLATOR_PROXY_MODEL_RUNTIME_ENDPOINT     = $RuntimeEndpoint
        TRANSLATOR_PROXY_MODEL_RUNTIME_MODEL        = $Model
        TRANSLATOR_PROXY_MODEL_RUNTIME_TIMEOUT_SECS = "$TimeoutSecs"
        RUST_LOG                                    = 'info'
    }

    $baseUrl = "http://$listenAddr"
    $client = [System.Net.Http.HttpClient]::new()
    $client.Timeout = [TimeSpan]::FromSeconds($TimeoutSecs + 10)

    try {
        Wait-ProxyReady -Client $client -BaseUrl $baseUrl -Process $process -Model $Model

        $results = @{}
        foreach ($entry in $Corpus.entries) {
            $results[$entry.id] = @{
                Latencies = [System.Collections.Generic.List[double]]::new()
                Outputs   = [System.Collections.Generic.List[string]]::new()
            }
        }

        $offsets = $null
        $totalPasses = $WarmupPasses + $MeasuredPasses
        for ($pass = 1; $pass -le $totalPasses; $pass++) {
            $measured = $pass -gt $WarmupPasses
            if ($measured) {
                # Reason counts cover only the measured passes; a cold-load
                # timeout during warm-up would pollute them.
                $offsets = @{
                    stdout = (Get-Item $stdoutLog).Length
                    stderr = (Get-Item $stderrLog).Length
                }
            }
            foreach ($entry in $Corpus.entries) {
                $encoded = [System.Uri]::EscapeDataString($entry.source)
                $uri = "$baseUrl/?text=$encoded&from=$($Corpus.from)&to=$($Corpus.to)"
                $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
                $body = $client.GetStringAsync($uri).GetAwaiter().GetResult()
                $stopwatch.Stop()
                if ($measured) {
                    $results[$entry.id].Latencies.Add([double]$stopwatch.Elapsed.TotalMilliseconds)
                    $results[$entry.id].Outputs.Add($body)
                }
            }
        }

        return @{
            Results   = $results
            Offsets   = $offsets
            StdoutLog = $stdoutLog
            StderrLog = $stderrLog
        }
    } finally {
        $client.Dispose()
        if (-not $process.HasExited) {
            $process.Kill()
            $process.WaitForExit(5000) | Out-Null
        }
        $process.Dispose()
    }
}

if ($Models.Count -eq 0) { throw 'no models requested' }
if ($MeasuredPasses -lt 1) { throw 'MeasuredPasses must be at least 1' }

$CorpusPath = Resolve-RepoPath $CorpusPath
$ProxyExe = Resolve-RepoPath $ProxyExe
$OutputDir = Resolve-RepoPath $OutputDir
$runtimeEndpoint = $OllamaEndpoint.TrimEnd('/') + '/api/chat'

$corpus = Get-Content $CorpusPath -Raw | ConvertFrom-Json
Test-Corpus -Corpus $corpus

Assert-ModelsInstalled -Requested $Models -Endpoint $OllamaEndpoint
Assert-ProxyBuilt -Exe $ProxyExe
New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null

$modelReports = @()
$summaryLines = [System.Collections.Generic.List[string]]::new()
$summaryLines.Add('| Model | Translated | Passthrough | Median ms | Min ms | Max ms | Timeout | Runtime error | Token mismatch | Malformed token | Injected break | Lang pair | Unstable |')
$summaryLines.Add('|---|---|---|---|---|---|---|---|---|---|---|---|---|')

$index = 0
foreach ($model in $Models) {
    $port = $PortBase + $index
    $index++
    Write-Host "Running $model on port $port ($MeasuredPasses measured passes over $($corpus.entries.Count) entries)..."
    $run = Invoke-ModelRun -Model $model -Port $port -Corpus $corpus -ProxyExe $ProxyExe `
        -RuntimeEndpoint $runtimeEndpoint -LogDirectory $OutputDir

    $logText = (Get-LogSlice -Path $run.StdoutLog -Offset $run.Offsets.stdout) +
    (Get-LogSlice -Path $run.StderrLog -Offset $run.Offsets.stderr)
    $reasonCounts = @{}
    foreach ($reason in $reasonMarkers.Keys) {
        $reasonCounts[$reason] = Get-SubstringCount -Text $logText -Needle $reasonMarkers[$reason]
    }

    $rows = @()
    $allLatencies = [System.Collections.Generic.List[double]]::new()
    $translated = 0
    $unstableCount = 0
    foreach ($entry in $corpus.entries) {
        $data = $run.Results[$entry.id]
        $latencies = [double[]]$data.Latencies.ToArray()
        $outputs = [string[]]$data.Outputs.ToArray()
        $allLatencies.AddRange($latencies)

        $output = $outputs[0]
        # Each corpus entry is one physical line, so ordinal equality with the
        # source is exactly the Passthrough Response.
        $passthrough = [string]::Equals($output, $entry.source, [System.StringComparison]::Ordinal)
        $status = if ($passthrough) { 'passthrough' } else { 'translated' }
        if (-not $passthrough) { $translated++ }

        $unstable = (($outputs | Select-Object -Unique).Count) -gt 1
        if ($unstable) { $unstableCount++ }

        $rows += @{
            id         = $entry.id
            category   = $entry.category
            source     = $entry.source
            reference  = $entry.reference
            status     = $status
            preserved  = (Get-PreservedStatus -Text $output -Tokens @($entry.must_preserve))
            median_ms  = [math]::Round((Get-Median -Values $latencies), 1)
            min_ms     = [math]::Round(($latencies | Measure-Object -Minimum).Minimum, 1)
            max_ms     = [math]::Round(($latencies | Measure-Object -Maximum).Maximum, 1)
            unstable   = $unstable
            output     = $output
            latencies  = $latencies
        }
    }

    $pooled = [double[]]$allLatencies.ToArray()
    $passthroughCount = $rows.Count - $translated
    $summary = [ordered]@{
        model                = $model
        translated           = $translated
        passthrough          = $passthroughCount
        median_ms            = [math]::Round((Get-Median -Values $pooled), 1)
        min_ms               = [math]::Round(($pooled | Measure-Object -Minimum).Minimum, 1)
        max_ms               = [math]::Round(($pooled | Measure-Object -Maximum).Maximum, 1)
        unstable             = $unstableCount
        reasons              = $reasonCounts
        log_paths            = @($run.StdoutLog, $run.StderrLog)
    }
    $summaryLines.Add(('| {0} | {1} | {2} | {3} | {4} | {5} | {6} | {7} | {8} | {9} | {10} | {11} | {12} |' -f `
            $model, $translated, $passthroughCount, $summary.median_ms, $summary.min_ms, $summary.max_ms,
            $reasonCounts['timeout'], $reasonCounts['runtime error'], $reasonCounts['token mismatch'],
            $reasonCounts['malformed token'], $reasonCounts['injected line break'],
            $reasonCounts['language pair'], $unstableCount))
    $modelReports += @{ model = $model; summary = $summary; entries = $rows }
}

$runtimeVersion = (Invoke-RestMethod -Uri "$OllamaEndpoint/api/version" -TimeoutSec 15).version
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$mdPath = Join-Path $OutputDir "report-$stamp.md"
$jsonPath = Join-Path $OutputDir "report-$stamp.json"
$generatedAt = (Get-Date).ToString('yyyy-MM-dd HH:mm:ss zzz')
$proxyInfo = (Get-Item $ProxyExe)
$entryCount = $corpus.entries.Count

$md = [System.Collections.Generic.List[string]]::new()
$md.Add('# Model comparison: ' + ($Models -join ' vs '))
$md.Add('')
$md.Add('## Environment')
$md.Add('')
$md.Add("- Date: $generatedAt")
$md.Add("- Model runtime version: $runtimeVersion ($OllamaEndpoint)")
$md.Add("- Proxy binary: $ProxyExe (built $($proxyInfo.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss')))")
$md.Add("- Corpus: $CorpusPath ($entryCount entries)")
$md.Add("- Passes: $WarmupPasses warm-up (discarded), $MeasuredPasses measured; Translation Unit timeout ${TimeoutSecs}s")
$md.Add('')
$md.Add('Models ran sequentially because a single Model Runtime keeps one model resident; interleaving the sides would measure model loads rather than translation.')
$md.Add('')
$md.Add("Reason counts are substring hits in each model's proxy log over the measured passes only. ``passthrough`` rows carry no per-row reason; the summary attributes them by log line.")
$md.Add('')
$md.Add('## Summary')
$md.Add('')
foreach ($line in $summaryLines) { $md.Add($line) }
$md.Add('')
$md.Add('## Entries')
foreach ($entry in $corpus.entries) {
    $md.Add('')
    $md.Add("### $($entry.id) ($($entry.category))")
    $md.Add('')
    $md.Add("Source: ``$($entry.source)``")
    $md.Add('')
    $md.Add("Reference: ``$($entry.reference)``")
    $notes = $entry.PSObject.Properties['notes']
    if ($notes) { $md.Add(''); $md.Add("Notes: $($notes.Value)") }
    $md.Add('')
    $md.Add('| Model | Status | Preserved | Median (min-max) ms | Output |')
    $md.Add('|---|---|---|---|---|')
    foreach ($report in $modelReports) {
        $row = $report.entries | Where-Object { $_.id -eq $entry.id }
        $outputEscaped = $row.output -replace '\|', '\|'
        $md.Add(('| {0} | {1} | {2} | {3} ({4}-{5}) | ``{6}`` |' -f `
                $report.model, $row.status, $row.preserved, $row.median_ms, $row.min_ms, $row.max_ms, $outputEscaped))
    }
}

$report = @{
    generated_at = (Get-Date).ToString('o')
    environment  = @{
        date                  = $generatedAt
        model_runtime_version = $runtimeVersion
        ollama_endpoint       = $OllamaEndpoint
        proxy_binary          = $ProxyExe
        corpus_path           = $CorpusPath
        entry_count           = $entryCount
        warmup_passes         = $WarmupPasses
        measured_passes       = $MeasuredPasses
        timeout_secs          = $TimeoutSecs
        note                  = 'Models ran sequentially because a single Model Runtime keeps one model resident; interleaving the sides would measure model loads rather than translation.'
    }
    models       = $modelReports
}

Set-Content -Path $mdPath -Value ($md -join "`r`n") -Encoding utf8
Set-Content -Path $jsonPath -Value ($report | ConvertTo-Json -Depth 6) -Encoding utf8

Write-Host ''
Write-Host "Report: $mdPath"
Write-Host "JSON:   $jsonPath"
Write-Host ''
foreach ($line in $summaryLines) { Write-Host $line }

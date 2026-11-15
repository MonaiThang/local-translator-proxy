#Requires -Version 7.4

param(
    [string]$BaseUrl = 'http://127.0.0.1:5000/',
    [string]$TextFile = 'scripts/bench-lines.txt',
    [int]$Count = 200,
    [string]$From = 'zh-CN',
    [string]$To = 'en',
    [int]$TimeoutSecs = 5,
    [int]$BudgetMs = 1000
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot

function Resolve-RepoPath([string]$Path) {
    if ([System.IO.Path]::IsPathRooted($Path)) { return $Path }
    return (Join-Path $repoRoot $Path)
}

$TextFile = Resolve-RepoPath $TextFile

$lines = @(Get-Content $TextFile | Where-Object { $_.Trim().Length -gt 0 })
if ($lines.Count -eq 0) { throw "no non-blank lines in $TextFile" }

$base = $BaseUrl.TrimEnd('/') + '/'
$client = [System.Net.Http.HttpClient]::new()
$client.Timeout = [TimeSpan]::FromSeconds($TimeoutSecs)

$latencies = [System.Collections.Generic.List[double]]::new()
$passthrough = 0
$coldMs = 0.0

try {
    for ($i = 0; $i -lt $Count; $i++) {
        $line = $lines[$i % $lines.Count]
        $encoded = [uri]::EscapeDataString($line)
        $uri = "$base" + "?text=$encoded&from=$From&to=$To"

        $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
        $body = $client.GetStringAsync($uri).GetAwaiter().GetResult()
        $stopwatch.Stop()

        $ms = $stopwatch.Elapsed.TotalMilliseconds
        if ($i -eq 0) {
            $coldMs = $ms
        } else {
            $latencies.Add($ms)
        }
        if ([string]::Equals($body, $line, [System.StringComparison]::Ordinal)) {
            $passthrough++
        }
    }
} finally {
    $client.Dispose()
}

$warm = [double[]]$latencies.ToArray()
if ($warm.Count -eq 0) { throw 'Count must be at least 2 so warm statistics exist' }
$sorted = [double[]]($warm | Sort-Object)
$n = $sorted.Count
$min = $sorted[0]
$max = $sorted[$n - 1]
if ($n % 2 -eq 1) {
    $median = [double]$sorted[[int](($n - 1) / 2)]
} else {
    $median = ([double]$sorted[$n / 2 - 1] + [double]$sorted[$n / 2]) / 2
}
$p95 = $sorted[[int][math]::Ceiling(0.95 * $n) - 1]

$cold = [math]::Round($coldMs, 1)
$min = [math]::Round($min, 1)
$median = [math]::Round($median, 1)
$p95 = [math]::Round($p95, 1)
$max = [math]::Round($max, 1)

Write-Host "cold_ms=$cold"
Write-Host "min_ms=$min median_ms=$median p95_ms=$p95 max_ms=$max (n=$n)"
Write-Host "passthrough=$passthrough"

if ($passthrough -gt 0) {
    Write-Host "FAIL: $passthrough of $Count requests returned the original line"
    exit 1
}
if ($p95 -gt $BudgetMs) {
    Write-Host "FAIL: p95_ms=$p95 exceeds the ${BudgetMs}ms budget"
    exit 1
}
exit 0

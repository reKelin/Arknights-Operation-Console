param([string]$Executable = "src-tauri/target/release/arknights-operation-console.exe")
$ErrorActionPreference = "Stop"
$exe = (Resolve-Path -LiteralPath $Executable).Path
$root = Join-Path (Get-Location) ".local/desktop-smoke"
New-Item -ItemType Directory -Force -Path $root | Out-Null
$originalAppData = $env:APPDATA
$originalLocalAppData = $env:LOCALAPPDATA
$process = $null
try {
    # 不接触开发者已有配置；默认不启用监控或真实游戏输入。
    $env:APPDATA = Join-Path $root "roaming"
    $env:LOCALAPPDATA = Join-Path $root "local"
    New-Item -ItemType Directory -Force -Path $env:APPDATA, $env:LOCALAPPDATA | Out-Null
    $process = Start-Process -FilePath $exe -PassThru -RedirectStandardOutput (Join-Path $root "stdout.log") -RedirectStandardError (Join-Path $root "stderr.log")
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    $windowSeen = $false
    while ([DateTime]::UtcNow -lt $deadline) {
        $process.Refresh()
        if ($process.HasExited) { throw "应用启动后退出，退出码：$($process.ExitCode)" }
        if ($process.MainWindowHandle -ne 0 -and $process.Responding) { $windowSeen = $true; break }
        Start-Sleep -Milliseconds 250
    }
    if (-not $windowSeen) { throw "没有发现可响应的原生主窗口" }
    # 不把短暂出现后立即崩溃当作启动成功。
    Start-Sleep -Seconds 2
    $process.Refresh()
    if ($process.HasExited -or -not $process.Responding) { throw "主窗口出现后应用退出或失去响应" }
    Write-Output "原生进程和主窗口启动检查通过；不代表 WebView 内容、安装升级、WGC 或游戏输入已验收。"
} finally {
    if ($null -ne $process -and -not $process.HasExited) { Stop-Process -Id $process.Id -Force }
    $env:APPDATA = $originalAppData
    $env:LOCALAPPDATA = $originalLocalAppData
}

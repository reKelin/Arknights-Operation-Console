$ErrorActionPreference = "Stop"
$destination = Join-Path (Get-Location) ".local/release"
New-Item -ItemType Directory -Force -Path $destination | Out-Null
$binary = Get-Item -LiteralPath "src-tauri/target/release/arknights-operation-runner.exe"
$nsis = @(Get-ChildItem -Path "src-tauri/target/release/bundle/nsis/*.exe" -File)
$msi = @(Get-ChildItem -Path "src-tauri/target/release/bundle/msi/*.msi" -File)
if ($nsis.Count -eq 0 -or $msi.Count -eq 0) { throw "缺少 NSIS 或 MSI 安装包" }
$files = @($binary) + $nsis + $msi
foreach ($file in $files) { Copy-Item -LiteralPath $file.FullName -Destination $destination }
@{
    commit = $env:GITHUB_SHA
    runId = $env:GITHUB_RUN_ID
    version = (Get-Content -Raw package.json | ConvertFrom-Json).version
    nativeSmoke = "process-and-window-only"
} | ConvertTo-Json | Set-Content -Encoding utf8NoBOM (Join-Path $destination "build-info.json")
$checksums = Get-ChildItem -LiteralPath $destination -File | Where-Object { $_.Name -ne "SHA256SUMS" } | Sort-Object Name | ForEach-Object {
    $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $_.FullName).Hash.ToLowerInvariant()
    "$hash  $($_.Name)"
}
# sha256sum 在 Linux 发布任务中读取，固定使用 LF。
[IO.File]::WriteAllText((Join-Path $destination "SHA256SUMS"), (($checksums -join "`n") + "`n"), [Text.UTF8Encoding]::new($false))

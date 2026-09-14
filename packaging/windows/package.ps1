$ErrorActionPreference = 'Stop'

$packageDir = Join-Path $env:RUNNER_TEMP 'zerium-package'
Remove-Item -Recurse -Force $packageDir, dist -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $packageDir, dist | Out-Null
Copy-Item target\release\zerium.exe $packageDir\zerium.exe
Copy-Item LICENSE $packageDir\LICENSE

$ffmpegBin = 'target\ffmpeg-sdk\bin'
Get-ChildItem "$ffmpegBin\*.dll" | Copy-Item -Destination $packageDir
$vcRuntimeDir = $env:VC_RUNTIME_DIR
if (-not $vcRuntimeDir) {
    throw 'VC_RUNTIME_DIR is not set; install_dependencies.ps1 must run first.'
}
$vcRuntimeFiles = @(Get-ChildItem (Join-Path $vcRuntimeDir '*.dll'))
if ($vcRuntimeFiles.Count -eq 0) {
    throw "No Visual C++ runtime DLLs found in $vcRuntimeDir."
}
$vcRuntimeFiles | Copy-Item -Destination $packageDir

$version = (Select-String -Path Cargo.toml -Pattern '^version = "([0-9]+\.[0-9]+\.[0-9]+)"' |
    Select-Object -First 1).Matches.Groups[1].Value
$ArchiveName = "zerium-$version-windows-x86_64.msi"
$versionParts = $version.Split('.')
$msiBuild = [Math]::Min(65535, [int]$env:GITHUB_RUN_NUMBER)
$msiVersion = "$($versionParts[0]).$($versionParts[1]).$msiBuild"

$wixBin = (Get-ChildItem "${env:ProgramFiles(x86)}\WiX Toolset v*\bin\candle.exe" |
    Sort-Object FullName | Select-Object -Last 1).DirectoryName
$wixDir = Join-Path $env:RUNNER_TEMP 'wix'
New-Item -ItemType Directory -Force $wixDir | Out-Null

& "$wixBin\heat.exe" dir $packageDir `
    -cg ApplicationFiles `
    -dr INSTALLFOLDER `
    -gg -g1 -scom -sreg -srd `
    -var var.PackageDir `
    -out "$wixDir\files.wxs"
& "$wixBin\candle.exe" `
    "-dPackageDir=$packageDir" `
    "-dIconPath=$((Resolve-Path assets\zerium.ico).Path)" `
    "-dProductVersion=$msiVersion" `
    packaging\windows\zerium.wxs "$wixDir\files.wxs" `
    -out "$wixDir\"
& "$wixBin\light.exe" `
    -ext (Join-Path $wixBin 'WixUIExtension.dll') `
    -cultures:en-us `
    "$wixDir\zerium.wixobj" "$wixDir\files.wixobj" `
    -pdbout "$wixDir\zerium.wixpdb" `
    -out "dist\$ArchiveName"

$ErrorActionPreference = 'Stop'

$packageDir = Join-Path $env:RUNNER_TEMP 'zerium-package'
Remove-Item -Recurse -Force $packageDir, dist -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $packageDir, dist | Out-Null
Copy-Item target\release\zerium.exe $packageDir\zerium.exe
Copy-Item LICENSE $packageDir\LICENSE
Copy-Item assets\inter\OFL.txt $packageDir\Inter-OFL.txt

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

# Keep the installer agreement in sync with the license shipped in the package.
$licenseRtfPath = Join-Path $wixDir 'license.rtf'
$licenseText = [System.IO.File]::ReadAllText((Resolve-Path LICENSE).Path)
$licenseText = $licenseText.Replace('\', '\\').Replace('{', '\{').Replace('}', '\}')
$licenseText = $licenseText -replace '\r\n|\r|\n', '\line '
$licenseRtf = '{\rtf1\ansi\deff0{\fonttbl{\f0\fmodern Courier New;}}\f0\fs18\pard ' + $licenseText + '}'
[System.IO.File]::WriteAllText($licenseRtfPath, $licenseRtf, [System.Text.Encoding]::ASCII)

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
    "-dLicenseRtfPath=$licenseRtfPath" `
    packaging\windows\zerium.wxs "$wixDir\files.wxs" `
    -out "$wixDir\"
& "$wixBin\light.exe" `
    -ext (Join-Path $wixBin 'WixUIExtension.dll') `
    -cultures:en-us `
    "$wixDir\zerium.wixobj" "$wixDir\files.wixobj" `
    -pdbout "$wixDir\zerium.wixpdb" `
    -out "dist\$ArchiveName"

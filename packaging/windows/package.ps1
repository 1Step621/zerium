$ErrorActionPreference = 'Stop'

if (-not $env:EDITBIN) {
    throw 'EDITBIN is not set; install_dependencies.ps1 must run first.'
}

$packageDir = Join-Path $env:RUNNER_TEMP 'zerium-package'
Remove-Item -Recurse -Force $packageDir, dist -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $packageDir, dist | Out-Null
Copy-Item target\release\zerium.exe $packageDir\zerium.exe
# Rust's MSVC entry point works for both subsystems. Change only the copy's
# PE subsystem so terminal invocations get a console without compiling again.
$cliPath = Join-Path $packageDir 'zerium.com'
Copy-Item target\release\zerium.exe $cliPath
& $env:EDITBIN /NOLOGO /SUBSYSTEM:CONSOLE $cliPath
if ($LASTEXITCODE -ne 0) {
    Remove-Item $cliPath
    throw 'Failed to set the Zerium CLI console subsystem.'
}
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

if (-not $env:ZERIUM_RELEASE_VERSION) {
    throw 'ZERIUM_RELEASE_VERSION is not set.'
}

$licensePath = Join-Path $env:RUNNER_TEMP 'zerium-license.txt'
Copy-Item LICENSE $licensePath

& vpk pack --packId zerium --packTitle Zerium --packAuthors Zerium `
    --packVersion $env:ZERIUM_RELEASE_VERSION --packDir $packageDir `
    --mainExe zerium.exe --runtime win-x64 --channel win-x86_64 `
    --icon assets\zerium.ico `
    --msi --instLocation PerUser --instLicense $licensePath --outputDir dist
if ($LASTEXITCODE -ne 0) {
    throw 'Velopack packaging failed.'
}

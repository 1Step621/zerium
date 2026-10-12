$ErrorActionPreference = 'Stop'

if (-not $env:EDITBIN) {
    throw 'EDITBIN is not set; install_dependencies.ps1 must run first.'
}

$packageDir = Join-Path $env:RUNNER_TEMP 'zerium-package'
Remove-Item -Recurse -Force $packageDir -ErrorAction SilentlyContinue
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

$gstRoot = $env:GSTREAMER_ROOT
if (-not $gstRoot) { throw 'GSTREAMER_ROOT is not set.' }
Get-ChildItem (Join-Path $gstRoot 'bin\*.dll') | Copy-Item -Destination $packageDir
New-Item -ItemType Directory -Force "$packageDir\lib", "$packageDir\libexec\gstreamer-1.0" | Out-Null
Copy-Item (Join-Path $gstRoot 'lib\gstreamer-1.0') "$packageDir\lib" -Recurse
Copy-Item (Join-Path $gstRoot 'libexec\gstreamer-1.0\gst-plugin-scanner.exe') "$packageDir\libexec\gstreamer-1.0"
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
    --mainExe zerium.exe --runtime win-x64 `
    --icon assets\zerium.ico `
    --msi --instLocation PerUser --instLicense $licensePath --outputDir dist
if ($LASTEXITCODE -ne 0) {
    throw 'Velopack packaging failed.'
}

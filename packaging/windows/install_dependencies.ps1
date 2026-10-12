$ErrorActionPreference = 'Stop'

$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
$vsInstall = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
$editbin = Get-ChildItem (Join-Path $vsInstall 'VC\Tools\MSVC\*\bin\Hostx64\x64\editbin.exe') -File |
    Sort-Object FullName | Select-Object -Last 1
if (-not $editbin) {
    throw 'The x64 MSVC editbin tool was not found on the runner.'
}
"EDITBIN=$($editbin.FullName)" | Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append
$vcRuntime = Get-ChildItem (Join-Path $vsInstall 'VC\Redist\MSVC\*\x64\Microsoft.VC*.CRT') -Directory |
    Sort-Object FullName | Select-Object -Last 1
if (-not $vcRuntime) {
    throw 'The x64 Visual C++ runtime was not found on the runner.'
}
"VC_RUNTIME_DIR=$($vcRuntime.FullName)" | Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append

# Official MSVC runtime and SDK; D3D12 shared-memory APIs require 1.26 or later.
$gstVersion = '1.26.10'
$gstInstallDir = Join-Path $env:RUNNER_TEMP 'gstreamer'
$gstRoot = Join-Path $gstInstallDir '1.0\msvc_x86_64'
foreach ($component in @('gstreamer-1.0', 'gstreamer-1.0-devel')) {
    $name = "$component-msvc-x86_64-$gstVersion.msi"
    $installer = Join-Path $env:RUNNER_TEMP $name
    Invoke-WebRequest "https://gstreamer.freedesktop.org/data/pkg/windows/$gstVersion/msvc/$name" -OutFile $installer
    $process = Start-Process msiexec.exe -Wait -PassThru -ArgumentList @('/i', "`"$installer`"", '/quiet', '/norestart', 'ADDLOCAL=ALL', "INSTALLDIR=`"$gstInstallDir`"")
    if ($process.ExitCode -notin @(0, 3010)) { throw "GStreamer installer failed: $($process.ExitCode)" }
}
if (-not (Test-Path (Join-Path $gstRoot 'lib\pkgconfig\glib-2.0.pc'))) {
    throw "GStreamer development files were not installed in $gstRoot."
}
# GStreamer's D3D12 pkg-config metadata references DirectX-Headers, but the MSI
# omits that dependency's development files.
vcpkg install directx-headers:x64-windows
if ($LASTEXITCODE -ne 0) { throw 'Failed to install DirectX-Headers.' }
$directXPkgConfig = Join-Path $env:VCPKG_INSTALLATION_ROOT 'installed\x64-windows\lib\pkgconfig'
"GSTREAMER_ROOT=$gstRoot" | Out-File $env:GITHUB_ENV -Encoding utf8 -Append
"PKG_CONFIG_PATH=$gstRoot\lib\pkgconfig;$directXPkgConfig" | Out-File $env:GITHUB_ENV -Encoding utf8 -Append
"$gstRoot\bin" | Out-File $env:GITHUB_PATH -Encoding utf8 -Append
# Rust's system-deps uses pkg-config even with the MSVC SDK.
choco install pkgconfiglite --yes --no-progress
if ($LASTEXITCODE -ne 0) { throw 'Failed to install pkg-config.' }

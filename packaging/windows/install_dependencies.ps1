$ErrorActionPreference = 'Stop'

$ffmpegArchive = Join-Path $env:RUNNER_TEMP 'ffmpeg-9.0.1-29-gad500d59cb-windows-x86_64.zip'
$ffmpegUrl = 'https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-09-14-13-17/ffmpeg-n9.0.1-29-gad500d59cb-win64-gpl-shared-9.0.zip'
Remove-Item -Recurse -Force target\ffmpeg-sdk -ErrorAction SilentlyContinue
Invoke-WebRequest -Uri $ffmpegUrl -OutFile $ffmpegArchive
Expand-Archive -Path $ffmpegArchive -DestinationPath target\ffmpeg-extract
$ffmpegRoot = Get-ChildItem target\ffmpeg-extract -Directory | Select-Object -First 1
Move-Item $ffmpegRoot.FullName target\ffmpeg-sdk
Remove-Item -Recurse -Force target\ffmpeg-extract
"FFMPEG_DIR=$env:GITHUB_WORKSPACE\target\ffmpeg-sdk" | Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append

$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
$vsInstall = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
$vcRuntime = Get-ChildItem (Join-Path $vsInstall 'VC\Redist\MSVC\*\x64\Microsoft.VC*.CRT') -Directory |
    Sort-Object FullName | Select-Object -Last 1
if (-not $vcRuntime) {
    throw 'The x64 Visual C++ runtime was not found on the runner.'
}
"VC_RUNTIME_DIR=$($vcRuntime.FullName)" | Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append

choco install wixtoolset --no-progress --yes

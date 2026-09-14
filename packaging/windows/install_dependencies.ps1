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

choco install wixtoolset --no-progress --yes

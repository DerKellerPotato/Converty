# Builds the Windows release files into dist\:
#   Converty-<version>-windows-x64-portable.zip, -setup.exe (Inno Setup) and .msi (WiX v5).
# Needs: Rust (MSVC), Inno Setup 6 (iscc) and the WiX .NET tool (dotnet tool install --global wix --version 5.0.2).
$ErrorActionPreference = "Stop"
$Root = Resolve-Path "$PSScriptRoot\..\.."
Set-Location $Root
$Version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches.Groups[1].Value

cargo build --release -p converty
if ($LASTEXITCODE) { exit $LASTEXITCODE }

New-Item -ItemType Directory -Force dist | Out-Null

# Portable zip
$Stage = "target\windows\Converty-$Version"
Remove-Item -Recurse -Force target\windows -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $Stage | Out-Null
Copy-Item target\release\converty.exe $Stage
Copy-Item LICENSE "$Stage\LICENSE.txt"
Copy-Item README.md $Stage
Compress-Archive -Path $Stage -DestinationPath "dist\Converty-$Version-windows-x64-portable.zip" -Force

# Installer (.exe)
$Iscc = (Get-Command iscc -ErrorAction SilentlyContinue).Source
if (-not $Iscc) { $Iscc = "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe" }
& $Iscc "/DAppVersion=$Version" "/DSourceDir=$Root\target\release" packaging\windows\converty.iss
if ($LASTEXITCODE) { exit $LASTEXITCODE }

# MSI
wix build -arch x64 -d "Version=$Version" -d "SourceDir=target\release" packaging\windows\converty.wxs -o "dist\Converty-$Version-windows-x64.msi"
if ($LASTEXITCODE) { exit $LASTEXITCODE }
Remove-Item dist\*.wixpdb -ErrorAction SilentlyContinue

Get-ChildItem dist

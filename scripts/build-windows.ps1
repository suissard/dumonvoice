# Build complet : PWA -> embarquée dans le daemon -> installeur.
# Prérequis : Node 20+, Rust stable (MSVC), CMake, LLVM (libclang), Inno Setup 6.
param(
  [switch]$Cuda,      # accélération NVIDIA (CUDA Toolkit requis)
  [switch]$Vulkan,    # accélération Vulkan (Vulkan SDK requis)
  [switch]$NoInstaller
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

Write-Host '==> PWA' -ForegroundColor Cyan
Push-Location "$root\pwa"
npm ci
npm run build
Pop-Location

Write-Host '==> Daemon Rust' -ForegroundColor Cyan
$features = @()
if ($Cuda) { $features += 'whisper-cuda' }
if ($Vulkan) { $features += 'whisper-vulkan' }
$cargoArgs = @('build', '--release', '--manifest-path', "$root\daemon\Cargo.toml")
if ($features.Count -gt 0) { $cargoArgs += @('--features', ($features -join ',')) }
& cargo @cargoArgs
if ($LASTEXITCODE -ne 0) { throw 'cargo build a échoué' }

if (-not $NoInstaller) {
  Write-Host '==> Installeur' -ForegroundColor Cyan
  $iscc = Get-Command iscc -ErrorAction SilentlyContinue
  if (-not $iscc) { $iscc = "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe" } else { $iscc = $iscc.Source }
  $version = (Select-String -Path "$root\daemon\Cargo.toml" -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
  & $iscc "/DAppVersion=$version" "$root\installer\dumonvoice.iss"
  if ($LASTEXITCODE -ne 0) { throw 'ISCC a échoué' }
  Write-Host "Installeur : $root\installer\Output" -ForegroundColor Green
}

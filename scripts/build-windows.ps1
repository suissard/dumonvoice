# Build complet : PWA -> embarquée dans le daemon -> installeur.
#
# Avant de lancer quoi que ce soit, le script vérifie tous les prérequis logiciels :
#   Node.js 20+ (npm), Rust (toolchain MSVC), Visual Studio Build Tools (C++), CMake,
#   LLVM (libclang), Inno Setup 6 (sauf -NoInstaller), CUDA Toolkit (-Cuda), Vulkan SDK (-Vulkan).
# Pour chaque élément manquant, il affiche le lien de téléchargement et propose de l'installer
# automatiquement via winget.
#
#   .\scripts\build-windows.ps1                  # vérifie, propose d'installer ce qui manque, puis compile
#   .\scripts\build-windows.ps1 -CheckOnly       # vérifie seulement (code de sortie 1 si un prérequis manque)
#   .\scripts\build-windows.ps1 -InstallMissing  # installe ce qui manque sans poser de question
param(
  [switch]$Cuda,           # accélération NVIDIA (CUDA Toolkit requis)
  [switch]$Vulkan,         # accélération Vulkan (Vulkan SDK requis)
  [switch]$NoInstaller,    # ne pas produire l'installeur Inno Setup
  [switch]$CheckOnly,      # vérifier les prérequis puis s'arrêter
  [switch]$InstallMissing  # installer les prérequis manquants sans confirmation
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

# ---------------------------------------------------------------------------------------------
# Détection des prérequis
# ---------------------------------------------------------------------------------------------

function Update-SessionPath {
  # Recharge le PATH depuis le registre (après une installation winget, sans rouvrir le terminal).
  # Les entrées déjà présentes dans la session sont conservées (venv, outils ajoutés à la main…).
  $current = @($env:Path -split ';' | Where-Object { $_ })
  foreach ($scope in 'Machine', 'User') {
    $value = [Environment]::GetEnvironmentVariable('Path', $scope)
    if (-not $value) { continue }
    foreach ($dir in ($value -split ';' | Where-Object { $_ })) {
      $expanded = [Environment]::ExpandEnvironmentVariables($dir)
      if ($current -notcontains $expanded) { $current += $expanded }
    }
  }
  $env:Path = $current -join ';'
  foreach ($name in 'CUDA_PATH', 'VULKAN_SDK', 'LIBCLANG_PATH') {
    if (-not (Get-Item "env:$name" -ErrorAction SilentlyContinue)) {
      $v = [Environment]::GetEnvironmentVariable($name, 'Machine')
      if (-not $v) { $v = [Environment]::GetEnvironmentVariable($name, 'User') }
      if ($v) { Set-Item "env:$name" $v }
    }
  }
  # Emplacements par défaut qui ne sont pas toujours ajoutés au PATH.
  foreach ($dir in "$env:USERPROFILE\.cargo\bin", "$env:ProgramFiles\nodejs", "$env:ProgramFiles\CMake\bin") {
    if ((Test-Path $dir) -and ($env:Path -split ';' -notcontains $dir)) { $env:Path = "$dir;$env:Path" }
  }
}

function Get-ToolVersion([string]$Command, [string[]]$Arguments, [string]$Pattern) {
  $cmd = Get-Command $Command -ErrorAction SilentlyContinue
  if (-not $cmd) { return $null }
  try { $out = (& $cmd.Source @Arguments 2>&1 | Out-String) } catch { return $null }
  $m = [regex]::Match($out, $Pattern)
  if ($m.Success) { return [version]$m.Groups[1].Value }
  return $null
}

function Find-InnoSetup {
  $cmd = Get-Command iscc -ErrorAction SilentlyContinue
  if ($cmd) { return $cmd.Source }
  $candidates = @(
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
  )
  foreach ($c in $candidates) { if ($c -and (Test-Path $c)) { return $c } }
  return $null
}

function Find-LibClang {
  $dirs = @()
  if ($env:LIBCLANG_PATH) { $dirs += $env:LIBCLANG_PATH }
  $clang = Get-Command clang -ErrorAction SilentlyContinue
  if ($clang) { $dirs += (Split-Path -Parent $clang.Source) }
  $dirs += "$env:ProgramFiles\LLVM\bin"
  if (${env:ProgramFiles(x86)}) { $dirs += "${env:ProgramFiles(x86)}\LLVM\bin" }
  foreach ($d in $dirs) { if ($d -and (Test-Path (Join-Path $d 'libclang.dll'))) { return $d } }
  return $null
}

function Find-MsvcTools {
  $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
  if (-not (Test-Path $vswhere)) { return $null }
  $path = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null
  if ($path) { return ($path | Select-Object -First 1) }
  return $null
}

function Test-Prerequisites {
  # Renvoie la liste des prérequis avec leur état (Ok, Detail) et comment les obtenir.
  $list = @()

  $node = Get-ToolVersion 'node' @('--version') 'v(\d+\.\d+\.\d+)'
  $npm = Get-Command npm -ErrorAction SilentlyContinue
  $list += [pscustomobject]@{
    Name = 'Node.js 20+ (npm)'
    Ok = [bool]($node -and $node.Major -ge 20 -and $npm)
    Detail = if ($node) { "v$node" } else { 'introuvable' }
    Url = 'https://nodejs.org/fr/download'
    WingetId = 'OpenJS.NodeJS.LTS'; WingetArgs = @()
  }

  $cargo = Get-ToolVersion 'cargo' @('--version') 'cargo (\d+\.\d+\.\d+)'
  $rustHost = $null
  if (Get-Command rustc -ErrorAction SilentlyContinue) {
    $m = [regex]::Match((rustc -vV 2>&1 | Out-String), 'host: (\S+)')
    if ($m.Success) { $rustHost = $m.Groups[1].Value }
  }
  $rustOk = [bool]($cargo -and $rustHost -like '*-msvc')
  $list += [pscustomobject]@{
    Name = 'Rust stable (toolchain MSVC)'
    Ok = $rustOk
    Detail = if (-not $cargo) { 'introuvable' } elseif (-not $rustOk) { "toolchain $rustHost (MSVC requise)" } else { "cargo $cargo, $rustHost" }
    Url = 'https://rustup.rs'
    WingetId = 'Rustlang.Rustup'; WingetArgs = @()
    Fix = if ($cargo -and -not $rustOk -and (Get-Command rustup -ErrorAction SilentlyContinue)) {
      { rustup toolchain install stable-x86_64-pc-windows-msvc; rustup default stable-x86_64-pc-windows-msvc }
    } else { $null }
  }

  $vs = Find-MsvcTools
  $list += [pscustomobject]@{
    Name = 'Visual Studio Build Tools (C++ x64)'
    Ok = [bool]$vs
    Detail = if ($vs) { $vs } else { 'introuvable (charge de travail « Développement Desktop en C++ »)' }
    Url = 'https://visualstudio.microsoft.com/fr/visual-cpp-build-tools/'
    WingetId = 'Microsoft.VisualStudio.2022.BuildTools'
    WingetArgs = @('--override', '--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended')
  }

  $cmake = Get-ToolVersion 'cmake' @('--version') 'cmake version (\d+\.\d+\.\d+)'
  $list += [pscustomobject]@{
    Name = 'CMake 3.14+'
    Ok = [bool]($cmake -and $cmake -ge [version]'3.14.0')
    Detail = if ($cmake) { "$cmake" } else { 'introuvable' }
    Url = 'https://cmake.org/download/'
    WingetId = 'Kitware.CMake'; WingetArgs = @()
  }

  $libclang = Find-LibClang
  $list += [pscustomobject]@{
    Name = 'LLVM (libclang)'
    Ok = [bool]$libclang
    Detail = if ($libclang) { $libclang } else { 'libclang.dll introuvable' }
    Url = 'https://github.com/llvm/llvm-project/releases/latest'
    WingetId = 'LLVM.LLVM'; WingetArgs = @()
  }

  if (-not $NoInstaller) {
    $iscc = Find-InnoSetup
    $list += [pscustomobject]@{
      Name = 'Inno Setup 6'
      Ok = [bool]$iscc
      Detail = if ($iscc) { $iscc } else { 'ISCC.exe introuvable' }
      Url = 'https://jrsoftware.org/isdl.php'
      WingetId = 'JRSoftware.InnoSetup'; WingetArgs = @()
    }
  }

  if ($Cuda) {
    $nvcc = Get-ToolVersion 'nvcc' @('--version') 'release (\d+\.\d+)'
    if (-not $nvcc -and $env:CUDA_PATH -and (Test-Path "$env:CUDA_PATH\bin\nvcc.exe")) { $nvcc = 'CUDA_PATH' }
    $list += [pscustomobject]@{
      Name = 'CUDA Toolkit (-Cuda)'
      Ok = [bool]$nvcc
      Detail = if ($nvcc) { "$nvcc" } else { 'nvcc introuvable' }
      Url = 'https://developer.nvidia.com/cuda-downloads'
      WingetId = 'Nvidia.CUDA'; WingetArgs = @()
    }
  }

  if ($Vulkan) {
    $vk = $env:VULKAN_SDK -and (Test-Path $env:VULKAN_SDK)
    $list += [pscustomobject]@{
      Name = 'Vulkan SDK (-Vulkan)'
      Ok = [bool]$vk
      Detail = if ($vk) { $env:VULKAN_SDK } else { 'variable VULKAN_SDK absente' }
      Url = 'https://vulkan.lunarg.com/sdk/home#windows'
      WingetId = 'KhronosGroup.VulkanSDK'; WingetArgs = @()
    }
  }

  return $list
}

function Show-Prerequisites($list) {
  Write-Host '==> Vérification des prérequis' -ForegroundColor Cyan
  foreach ($p in $list) {
    if ($p.Ok) {
      Write-Host ("  [OK]       {0,-38} {1}" -f $p.Name, $p.Detail) -ForegroundColor Green
    } else {
      Write-Host ("  [MANQUANT] {0,-38} {1}" -f $p.Name, $p.Detail) -ForegroundColor Red
      Write-Host ("             Télécharger : {0}" -f $p.Url) -ForegroundColor Yellow
    }
  }
}

function Install-Prerequisite($p) {
  if ($p.Fix) {
    Write-Host "  -> $($p.Name) : correction de la toolchain" -ForegroundColor Cyan
    & $p.Fix
    return
  }
  Write-Host "  -> winget install $($p.WingetId)" -ForegroundColor Cyan
  $wingetArgs = @('install', '--id', $p.WingetId, '-e', '--source', 'winget',
    '--accept-package-agreements', '--accept-source-agreements') + $p.WingetArgs
  & winget @wingetArgs
  # 0 = installé ; -1978335189 (0x8A15002B) = déjà installé / aucune mise à jour applicable.
  if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne -1978335189) {
    Write-Warning "winget a échoué pour $($p.Name) (code $LASTEXITCODE). Installez-le manuellement : $($p.Url)"
  }
}

# ---------------------------------------------------------------------------------------------
# Vérification (et installation éventuelle) avant tout build
# ---------------------------------------------------------------------------------------------

Update-SessionPath
$prereqs = Test-Prerequisites
Show-Prerequisites $prereqs
$missing = @($prereqs | Where-Object { -not $_.Ok })

if ($missing.Count -gt 0 -and -not $CheckOnly) {
  $winget = Get-Command winget -ErrorAction SilentlyContinue
  $installable = @($missing | Where-Object { $_.Fix -or $winget })
  if ($installable.Count -eq 0) {
    Write-Host ''
    Write-Host 'winget est indisponible : installez les éléments manquants via les liens ci-dessus' -ForegroundColor Yellow
    Write-Host "(ou installez « App Installer » depuis le Microsoft Store : https://apps.microsoft.com/detail/9NBLGGH4NNS1)." -ForegroundColor Yellow
  } else {
    $doInstall = [bool]$InstallMissing
    if (-not $doInstall) {
      Write-Host ''
      $names = ($installable | ForEach-Object { $_.Name }) -join ', '
      $answer = Read-Host "Installer automatiquement ($names) ? [O/n]"
      $doInstall = ($answer -eq '' -or $answer -match '^(o|oui|y|yes)$')
    }
    if ($doInstall) {
      Write-Host '==> Installation des prérequis manquants' -ForegroundColor Cyan
      foreach ($p in $installable) { Install-Prerequisite $p }
      Update-SessionPath
      $prereqs = Test-Prerequisites
      Show-Prerequisites $prereqs
      $missing = @($prereqs | Where-Object { -not $_.Ok })
    }
  }
}

if ($missing.Count -gt 0) {
  Write-Host ''
  Write-Host "Prérequis manquants : $((($missing | ForEach-Object { $_.Name }) -join ', '))." -ForegroundColor Red
  Write-Host "Installez-les (liens ci-dessus), ouvrez un nouveau terminal puis relancez ce script." -ForegroundColor Red
  Write-Host "Astuce : .\scripts\build-windows.ps1 -InstallMissing installe tout via winget sans question." -ForegroundColor Yellow
  exit 1
}

Write-Host 'Tous les prérequis sont présents.' -ForegroundColor Green
if ($CheckOnly) { exit 0 }

# bindgen (whisper-rs) a besoin de savoir où se trouve libclang.dll.
if (-not $env:LIBCLANG_PATH) { $env:LIBCLANG_PATH = Find-LibClang }

# ---------------------------------------------------------------------------------------------
# Build
# ---------------------------------------------------------------------------------------------

Write-Host '==> PWA' -ForegroundColor Cyan
Push-Location "$root\pwa"
npm ci
if ($LASTEXITCODE -ne 0) { Pop-Location; throw 'npm ci a échoué' }
npm run build
if ($LASTEXITCODE -ne 0) { Pop-Location; throw 'npm run build a échoué' }
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
  $iscc = Find-InnoSetup
  $version = (Select-String -Path "$root\daemon\Cargo.toml" -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
  & $iscc "/DAppVersion=$version" "$root\installer\dumonvoice.iss"
  if ($LASTEXITCODE -ne 0) { throw 'ISCC a échoué' }
  Write-Host "Installeur : $root\installer\Output" -ForegroundColor Green
}

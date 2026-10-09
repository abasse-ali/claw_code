<#
Réinstalle claw depuis les sources de ce dépôt, même quand le panneau de
VS Code est ouvert.

    .\scripts\install.ps1

Sous Windows, un programme en cours d'exécution verrouille son fichier :
l'ancien claw.exe est donc mis de côté avant l'installation. Si la
compilation échoue ou est interrompue, il est remis en place : on ne se
retrouve jamais sans claw. Les copies mises de côté lors d'installations
précédentes sont effacées quand plus rien ne s'en sert.

-Root : dossier d'installation (défaut : celui de cargo, ~\.cargo).
-CratePath : crate à installer (défaut : crates/claw-cli).
#>
param(
    [string]$Root = (Join-Path $env:USERPROFILE '.cargo'),
    [string]$CratePath = 'crates/claw-cli'
)

$ErrorActionPreference = 'Stop'
$workspace = Split-Path -Parent $PSScriptRoot
$bin = Join-Path $Root 'bin'
$exe = Join-Path $bin 'claw.exe'
$aside = Join-Path $bin ('claw.exe.ancien-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))

$moved = $false
if (Test-Path $exe) {
    Rename-Item $exe $aside
    $moved = $true
}

$installed = $false
Push-Location $workspace
try {
    cargo install --path $CratePath --locked --target-dir target --root $Root
    $installed = ($LASTEXITCODE -eq 0) -and (Test-Path $exe)
}
finally {
    Pop-Location
    if (-not $installed -and $moved -and -not (Test-Path $exe)) {
        Rename-Item $aside $exe
    }
}

if (-not $installed) {
    if ($moved) {
        Write-Host "Installation échouée : l'ancienne version de claw est remise en place." -ForegroundColor Red
    }
    else {
        Write-Host 'Installation échouée.' -ForegroundColor Red
    }
    exit 1
}

# Anciennes copies : celle d'un claw encore lancé reste verrouillée et sera
# effacée à la prochaine installation.
foreach ($old in Get-ChildItem -Path $bin -Filter 'claw.exe.ancien*' -ErrorAction SilentlyContinue) {
    try { Remove-Item $old.FullName -ErrorAction Stop } catch { }
}

$commit = ((& $exe --version | Select-String 'Commit').ToString() -replace '\s+', ' ').Trim()
Write-Host "claw est installé ($commit)." -ForegroundColor Green
Write-Host 'Le panneau de VS Code se recharge seul dans quelques secondes.'

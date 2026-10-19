<#
.SYNOPSIS
  Exécuteur de Claw Web sur ce PC : les sessions tournent ici, dans Docker Desktop, sans
  consommer les minutes GitHub Actions du compte.

.DESCRIPTION
  install  construit l'image (Linux, Python, git, Rust, Node, Chrome, exécuteur de GitHub),
           l'inscrit auprès du dépôt des sessions et la lance ; elle redémarre avec Docker.
  status   état du conteneur et de l'exécuteur vu par GitHub.
  stop     arrête l'exécuteur (les sessions attendent alors dans la file de GitHub).
  start    le relance.
  remove   le désinscrit de GitHub, puis supprime le conteneur, l'image et le volume.

  Il faut Docker Desktop lancé et la commande gh connectée (gh auth login) avec un compte
  administrateur du dépôt des sessions. Dans l'appli, Réglages, « Sessions » : « Sur mon PC ».

.EXAMPLE
  .\claw-pc.ps1 install
.EXAMPLE
  .\claw-pc.ps1 status
#>
[CmdletBinding()]
param(
    [Parameter(Position = 0)]
    [ValidateSet('install', 'status', 'start', 'stop', 'remove')]
    [string]$Action = 'status',

    # Dépôt des sessions de Claw Web (celui que l'appli appelle « moteur »).
    [ValidatePattern('^[\w.-]+/[\w.-]+$')]
    [string]$Engine = 'abasse-ali/claw-sessions'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$Image = 'claw-pc-runner'
$Container = 'claw-pc'
$Volume = 'claw-pc-runner'
$Label = 'claw-pc'

# Commande externe : sortie et code de retour. Windows PowerShell 5.1 change en erreur ce qu'une
# commande écrit sur stderr dès qu'on le redirige ; ici, seul le code de retour compte.
function Invoke-Native {
    param([string]$File, [string[]]$Arguments)
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $output = & $File @Arguments 2>&1
        $code = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $previous
    }
    [pscustomobject]@{ Code = $code; Text = (@($output) | ForEach-Object { "$_" }) -join "`n" }
}

function Assert-Tools {
    foreach ($tool in 'docker', 'gh') {
        if ($null -eq (Get-Command $tool -ErrorAction SilentlyContinue)) {
            throw "Commande $tool introuvable : installez Docker Desktop et GitHub CLI (gh)."
        }
    }
    if ((Invoke-Native docker @('info')).Code -ne 0) {
        throw 'Docker ne répond pas : lancez Docker Desktop, attendez « Engine running », puis recommencez.'
    }
    if ((Invoke-Native gh @('auth', 'status')).Code -ne 0) {
        throw 'gh n''est pas connecté : lancez gh auth login, puis recommencez.'
    }
}

function Get-ContainerState {
    $found = Invoke-Native docker @('ps', '-a', '--filter', "name=^$Container$", '--format', '{{.State}}')
    if ($found.Code -ne 0) { return '' }
    return $found.Text.Trim()
}

# Exécuteurs du dépôt des sessions vus par GitHub (nom, état, occupé), ceux de l'étiquette claw-pc.
# Le JSON est lu ici : Windows PowerShell 5.1 abîme les guillemets passés à une commande externe.
function Get-GitHubRunners {
    $answer = Invoke-Native gh @('api', "repos/$Engine/actions/runners")
    if ($answer.Code -ne 0) { throw "GitHub refuse la liste des exécuteurs de $Engine : $($answer.Text)" }
    foreach ($runner in @(($answer.Text | ConvertFrom-Json).runners)) {
        if (@($runner.labels | Where-Object { $_.name -eq $Label }).Count -eq 0) { continue }
        [pscustomobject]@{ Id = $runner.id; Name = $runner.name; Status = $runner.status; Busy = [bool]$runner.busy }
    }
}

function Install-Runner {
    Assert-Tools
    if (Get-ContainerState) {
        Write-Output "Le conteneur $Container existe déjà : .\claw-pc.ps1 status (ou remove, puis install)."
        return
    }
    $latest = Invoke-Native gh @('api', 'repos/actions/runner/releases/latest', '--jq', '.tag_name')
    if ($latest.Code -ne 0 -or $latest.Text.Trim() -notmatch '^v(\d+\.\d+\.\d+)$') {
        throw "Version de l'exécuteur de GitHub introuvable : $($latest.Text)"
    }
    $version = $Matches[1]
    Write-Output "Construction de l'image $Image (exécuteur $version) : plusieurs minutes la première fois..."
    & docker build --tag $Image --build-arg "RUNNER_VERSION=$version" $PSScriptRoot
    if ($LASTEXITCODE -ne 0) { throw "La construction de l'image a échoué (code $LASTEXITCODE)." }

    # Jeton d'inscription (une heure) : jamais affiché, passé par un fichier effacé aussitôt.
    $token = Invoke-Native gh @('api', '-X', 'POST', "repos/$Engine/actions/runners/registration-token", '--jq', '.token')
    if ($token.Code -ne 0 -or -not $token.Text.Trim()) {
        throw "GitHub refuse le jeton d'inscription : il faut être administrateur de $Engine."
    }
    $envFile = Join-Path $env:TEMP "claw-pc-$([guid]::NewGuid().ToString('N')).env"
    try {
        $lines = @("RUNNER_URL=https://github.com/$Engine", "RUNNER_TOKEN=$($token.Text.Trim())", "RUNNER_NAME=$Label")
        [System.IO.File]::WriteAllLines($envFile, $lines, (New-Object System.Text.UTF8Encoding($false)))
        & docker run --detach --name $Container --restart unless-stopped --env-file $envFile `
            --volume "${Volume}:/home/runner/actions-runner" $Image | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "Le lancement du conteneur a échoué (code $LASTEXITCODE)." }
    }
    finally {
        Remove-Item -LiteralPath $envFile -Force -ErrorAction SilentlyContinue
    }

    Write-Output 'Inscription auprès de GitHub...'
    for ($try = 0; $try -lt 30; $try++) {
        Start-Sleep -Seconds 2
        $online = @(Get-GitHubRunners | Where-Object { $_.Status -eq 'online' })
        if ($online.Count -gt 0) {
            Write-Output "Exécuteur $Label en ligne sur $Engine. Dans l'appli : Réglages, Sessions, « Sur mon PC »."
            Write-Output 'Pour qu''il tourne dès que le PC démarre : Docker Desktop, Settings, General, « Start Docker Desktop when you sign in ».'
            return
        }
    }
    throw "L'exécuteur n'apparaît pas en ligne : docker logs $Container"
}

function Show-Status {
    $state = Get-ContainerState
    if (-not $state) { $state = 'absent (.\claw-pc.ps1 install)' }
    Write-Output "Conteneur $Container : $state"
    foreach ($runner in @(Get-GitHubRunners)) {
        $work = if ($runner.Busy) { 'en session' } else { 'libre' }
        Write-Output "GitHub, exécuteur $($runner.Name) : $($runner.Status), $work"
    }
}

function Remove-Runner {
    Assert-Tools
    if ((Get-ContainerState) -eq 'running') {
        $removal = Invoke-Native gh @('api', '-X', 'POST', "repos/$Engine/actions/runners/remove-token", '--jq', '.token')
        if ($removal.Code -eq 0 -and $removal.Text.Trim()) {
            # Désinscription propre depuis le conteneur (le jeton passe par l'environnement, sans
            # guillemets : il n'a pas d'espace, et PowerShell 5.1 les abîmerait).
            $null = Invoke-Native docker @('exec', '--env', "REMOVE_TOKEN=$($removal.Text.Trim())", $Container,
                'sh', '-c', './config.sh remove --token $REMOVE_TOKEN')
        }
    }
    # Encore inscrit (conteneur arrêté) : effacé directement chez GitHub.
    foreach ($runner in @(Get-GitHubRunners)) {
        $null = Invoke-Native gh @('api', '-X', 'DELETE', "repos/$Engine/actions/runners/$($runner.Id)")
    }
    $null = Invoke-Native docker @('rm', '--force', $Container)
    $null = Invoke-Native docker @('volume', 'rm', $Volume)
    $null = Invoke-Native docker @('image', 'rm', $Image)
    Write-Output "Exécuteur $Label retiré de $Engine ; conteneur, volume et image supprimés."
}

switch ($Action) {
    'install' { Install-Runner }
    'status' { Show-Status }
    'start' {
        $null = Invoke-Native docker @('start', $Container)
        Show-Status
    }
    'stop' {
        $null = Invoke-Native docker @('stop', $Container)
        Show-Status
    }
    'remove' { Remove-Runner }
}

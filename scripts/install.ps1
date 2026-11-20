<#
.SYNOPSIS
  Instala o Taskbar Battery para o usuário atual (sem administrador).
.DESCRIPTION
  Copia o taskbar-battery.exe para %LOCALAPPDATA%\Programs\taskbar-battery, cria o atalho no Menu Iniciar,
  liga "Iniciar com o Windows" e registra o programa em Aplicativos instalados. Rode a partir da pasta
  que contém o taskbar-battery.exe (o zip da release) ou passe -ExePath.
.PARAMETER ExePath
  Caminho do taskbar-battery.exe. Padrão: o que está ao lado deste script (ou em ..\target\release).
.PARAMETER InstallDir
  Pasta de destino. Padrão: %LOCALAPPDATA%\Programs\taskbar-battery.
.PARAMETER NoStartup
  Não liga "Iniciar com o Windows".
.PARAMETER NoLaunch
  Não abre o programa ao final.
.PARAMETER DryRun
  Só mostra o que seria feito.
#>
[CmdletBinding()]
param(
    [string]$ExePath,
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA 'Programs\taskbar-battery'),
    [switch]$NoStartup,
    [switch]$NoLaunch,
    [switch]$DryRun
)
$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path

if (-not $ExePath) {
    foreach ($c in @((Join-Path $here 'taskbar-battery.exe'), (Join-Path $here '..\target\release\taskbar-battery.exe'))) {
        if (Test-Path $c) { $ExePath = (Resolve-Path $c).Path; break }
    }
}
if (-not $ExePath -or -not (Test-Path $ExePath)) { throw 'taskbar-battery.exe não encontrado. Use -ExePath.' }

$version = ((cmd /c "`"$ExePath`" --version") -join '') -replace '^\s*taskbar-battery\s+', ''
if (-not $version) { $version = '0.0.0' }
$exe = Join-Path $InstallDir 'taskbar-battery.exe'
$lnk = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Taskbar Battery.lnk'
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$unKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\TaskbarBattery'

function Step($msg, [scriptblock]$act) {
    if ($DryRun) { Write-Host "[dry-run] $msg" } else { Write-Host $msg; & $act }
}

Step 'Encerrando instância em execução' { Get-Process taskbar-battery -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep -Milliseconds 500 }
Step "Copiando para $InstallDir" {
    New-Item -ItemType Directory -Force $InstallDir | Out-Null
    Copy-Item $ExePath $exe -Force
    foreach ($f in 'README.md', 'LICENSE', 'uninstall.ps1') {
        $src = @((Join-Path $here $f), (Join-Path $here "..\$f")) | Where-Object { Test-Path $_ } | Select-Object -First 1
        if ($src) { Copy-Item $src (Join-Path $InstallDir $f) -Force }
    }
    # Arquivos baixados trazem a marca 'vindo da internet' (Zone.Identifier). Se ela ficasse no exe instalado,
    # o Windows pediria confirmação ("Executar?") a cada login e o app não iniciaria sozinho.
    Get-ChildItem $InstallDir -File | Unblock-File
}
Step 'Criando atalho no Menu Iniciar' {
    $s = (New-Object -ComObject WScript.Shell).CreateShortcut($lnk)
    $s.TargetPath = $exe; $s.WorkingDirectory = $InstallDir; $s.Description = 'Bateria dos periféricos na bandeja'; $s.Save()
}
if (-not $NoStartup) {
    Step 'Ligando "Iniciar com o Windows"' { Set-ItemProperty $runKey -Name TaskbarBattery -Value "`"$exe`"" }
}
Step 'Registrando em Aplicativos instalados' {
    New-Item $unKey -Force | Out-Null
    $vals = @{
        DisplayName = 'Taskbar Battery'; DisplayVersion = $version; Publisher = 'taskbar-battery'
        InstallLocation = $InstallDir; DisplayIcon = $exe
        UninstallString = "powershell.exe -NoProfile -ExecutionPolicy Bypass -File `"$InstallDir\uninstall.ps1`""
    }
    foreach ($k in $vals.Keys) { New-ItemProperty $unKey -Name $k -Value $vals[$k] -PropertyType String -Force | Out-Null }
    foreach ($k in 'NoModify', 'NoRepair') { New-ItemProperty $unKey -Name $k -Value 1 -PropertyType DWord -Force | Out-Null }
}
if (-not $NoLaunch) { Step 'Iniciando' { Start-Process $exe } }
Write-Host "Pronto. Instalado em $InstallDir"
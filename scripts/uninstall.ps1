<#
.SYNOPSIS
  Remove o Taskbar Battery do usuário atual: processo, inicialização, atalho, registro, configurações e arquivos.
#>
[CmdletBinding()]
param([switch]$KeepSettings)
$ErrorActionPreference = 'SilentlyContinue'
Get-Process taskbar-battery | Stop-Process -Force
Remove-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name TaskbarBattery
Remove-Item 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\TaskbarBattery' -Recurse -Force
Remove-Item (Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Taskbar Battery.lnk') -Force
if (-not $KeepSettings) {
    Remove-Item (Join-Path $env:APPDATA 'taskbar-battery') -Recurse -Force
    Remove-Item (Join-Path $env:LOCALAPPDATA 'taskbar-battery') -Recurse -Force
}
# a própria pasta de instalação é removida depois que este script termina
$dir = Split-Path -Parent $MyInvocation.MyCommand.Path
Start-Process cmd.exe -ArgumentList "/c timeout /t 2 >nul & rmdir /s /q `"$dir`"" -WindowStyle Hidden
Write-Host 'Taskbar Battery removido.'
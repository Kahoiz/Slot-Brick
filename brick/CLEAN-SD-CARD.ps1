# Run on Windows after updating Slot and shutting down the Brick.
# Example: .\CLEAN-SD-CARD.ps1 -Card E:\ -Backup "$env:USERPROFILE\Desktop\Slot-backup"
param([Parameter(Mandatory=$true)][string]$Card,[Parameter(Mandatory=$true)][string]$Backup)
$ErrorActionPreference='Stop'
$root=[IO.Path]::GetFullPath($Card).TrimEnd([IO.Path]::DirectorySeparatorChar)+[IO.Path]::DirectorySeparatorChar
$backupRoot=[IO.Path]::GetFullPath($Backup).TrimEnd([IO.Path]::DirectorySeparatorChar)+[IO.Path]::DirectorySeparatorChar
if ($backupRoot.StartsWith($root,[StringComparison]::OrdinalIgnoreCase)) {throw 'Backup must be outside the SD card.'}
foreach ($required in @('Slot\System\slot','.slot-boot\boot.sh')) {
 if (!(Test-Path -LiteralPath (Join-Path $root $required) -PathType Leaf)) {throw "Not a Slot card: missing $required"}
}
if (Test-Path -LiteralPath $backupRoot) {throw 'Choose a new, empty backup folder name.'}
New-Item -ItemType Directory -Path $backupRoot | Out-Null
$targets=New-Object System.Collections.Generic.List[string]
# Only recognize the optional launchers distributed with this port.
foreach ($entry in @(@('Tools\tg5040\Slot.pak','Slot/System/launch.sh'),@('Apps\SlotBootRestore','runtrimui-slot-backup.sh'))) {
 $launch=Join-Path $root ($entry[0]+'\launch.sh')
 if ((Test-Path -LiteralPath $launch) -and ((Get-Content -LiteralPath $launch -Raw).Contains($entry[1]))) {$targets.Add($entry[0])}
}
# Installer archives are kept off-card, never deleted without verified backup.
$installer=Join-Path $root 'Slot\Installer'
if (Test-Path -LiteralPath $installer) {
 foreach ($item in Get-ChildItem -LiteralPath $installer -Directory -Force) {
  if ($item.Name -match '^trimui-installed-[0-9]+$') {
   $hook=Join-Path $item.FullName 'app\runtrimui.sh'
   if ((Test-Path -LiteralPath $hook) -and ((Get-Content -LiteralPath $hook -Raw).Contains('SLOT_BRICK_BOOT_V2'))) {$targets.Add('Slot\Installer\'+$item.Name)}
  }
 }
}
foreach ($relative in $targets) {
 $src=Join-Path $root $relative; $dst=Join-Path $backupRoot $relative
 $items=@(Get-Item -LiteralPath $src -Force)+@(Get-ChildItem -LiteralPath $src -Recurse -Force)
 if ($items | Where-Object {($_.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0}) {throw "Refusing links in $relative"}
 New-Item -ItemType Directory -Path (Split-Path $dst -Parent) -Force | Out-Null
 Copy-Item -LiteralPath $src -Destination $dst -Recurse -Force
 foreach ($f in Get-ChildItem -LiteralPath $src -Recurse -File -Force) {
  $copy=Join-Path $dst $f.FullName.Substring($src.Length).TrimStart([IO.Path]::DirectorySeparatorChar)
  if ((Get-FileHash -LiteralPath $f.FullName -Algorithm SHA256).Hash -ne (Get-FileHash -LiteralPath $copy -Algorithm SHA256).Hash) {throw "Backup mismatch: $relative. Original retained."}
 }
 Remove-Item -LiteralPath $src -Recurse -Force
 Write-Host "Backed up and removed: $relative"
}
foreach ($relative in @('Tools\tg5040','Tools','Slot\Installer')) {
 $path=Join-Path $root $relative
 if ((Test-Path -LiteralPath $path -PathType Container) -and !(Get-ChildItem -LiteralPath $path -Force)) {Remove-Item -LiteralPath $path}
}
Write-Host 'Done. Games, saves, BIOS, settings, boot-logo backups and stock OS folders were preserved.'
Write-Host "Restore any removed tool by copying it back from $backupRoot"

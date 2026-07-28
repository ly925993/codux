$root = $PSScriptRoot
if (-not $root) { $root = Split-Path -Parent $MyInvocation.MyCommand.Path }
if ($root.StartsWith('\\?\UNC\')) { $root = '\\' + $root.Substring(8) }
elseif ($root.StartsWith('\\?\')) { $root = $root.Substring(4) }

$helper = Join-Path $root "..\codux-wrapper-helper.exe"
if (-not (Test-Path -LiteralPath $helper -PathType Leaf)) {
  Write-Error "codux-memory: bundled helper is missing"
  exit 127
}

& $helper --codux-wrapper-helper memory-plan @args
exit $LASTEXITCODE

# Adds this PC's public SSH key to a server's authorized_keys (asks for the
# server password once). Usage: .\add-ssh-key.ps1 root@10.0.0.106 [path\to\key.pub]
param(
  [Parameter(Mandatory = $true)][string]$Target,
  [string]$PublicKey = "$env:USERPROFILE\.ssh\id_rsa.pub"
)
if (-not (Test-Path $PublicKey)) { Write-Error "No public key at $PublicKey"; exit 1 }
$key = (Get-Content $PublicKey -Raw).Trim()
$remote = "umask 077; mkdir -p ~/.ssh; touch ~/.ssh/authorized_keys; grep -qxF '$key' ~/.ssh/authorized_keys || echo '$key' >> ~/.ssh/authorized_keys; echo KEY_INSTALLED"
ssh -o StrictHostKeyChecking=accept-new $Target $remote

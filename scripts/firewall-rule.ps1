# Ajoute manuellement les règles pare-feu (profil Privé uniquement) — à lancer en administrateur.
param([string]$Exe = "$env:LOCALAPPDATA\Programs\DumonVoice\dumonvoice.exe", [int]$Port = 18451)
netsh advfirewall firewall add rule name="DumonVoice Mobile" dir=in action=allow protocol=TCP localport=$Port profile=private program="$Exe"
netsh advfirewall firewall add rule name="DumonVoice mDNS" dir=in action=allow protocol=UDP localport=5353 profile=private program="$Exe"

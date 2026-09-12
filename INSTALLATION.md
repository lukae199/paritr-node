# Paritr Protocol 9 installieren und verwalten

Diese Anleitung beschreibt die Endanwender-Installation. Für die Veröffentlichung
der Pakete gilt zusätzlich `deployment/README.md`.

## Unterstützte Wege

| Plattform | `auto` verwendet | Alternativen |
|---|---|---|
| Linux x86-64/ARM64 | Docker Engine | native Binärdatei |
| macOS Intel/Apple Silicon | native LaunchAgent-Node | Docker Desktop |
| Windows 10/11 x64 | native Node | Docker Desktop mit WSL2 |
| Windows 11 ARM64 | x64-Node unter Emulation | Docker nur bei unterstütztem Docker Desktop |
| Windows Server | native Node | kein Docker Desktop |
| FreeBSD x86-64/ARM64 | native rc.d-Node | kein Docker-Pfad |

ARMv7, RISC-V, ppc64le und FreeBSD gelten bis zu Hardware-CI und Langzeittests
als Tier 2.

## Geführte Ein-Befehl-Installation

Linux, macOS und FreeBSD:

```bash
curl -fsSL https://paritr.highactive.de/downloads/bootstrap.sh | sh
```

Windows PowerShell:

```powershell
irm https://paritr.highactive.de/downloads/bootstrap.ps1 | iex
```

Der Assistent fragt nur relevante Werte ab:

- Docker oder native Installation;
- Mining ja/nein und nur dann eine öffentliche Paritr-Auszahlungsadresse;
- Anzahl CPU-Worker, Intensität und RandomX light/fast;
- optional Portal-URL und einmaligen Pairing-Code;
- optional eine bereits vorhandene öffentliche HTTPS-Adresse;
- optional die ausdrückliche Freigabe des öffentlichen TCP-Ports.

Der Installer fragt niemals nach Wallet-Seed, privatem Wallet-Schlüssel oder
Node-Admin-Secret. Pairing benötigt keine eingehende Portfreigabe.

## Nicht-interaktive Linux-Installation

Validierende, private Docker-Node:

```bash
curl -fsSLo /tmp/paritr-bootstrap.sh https://paritr.highactive.de/downloads/bootstrap.sh
sh /tmp/paritr-bootstrap.sh --deployment docker --light --yes
```

Mining-Node:

```bash
sh /tmp/paritr-bootstrap.sh \
  --deployment docker \
  --address 'P_DEINE_ADRESSE' \
  --fast \
  --cores 0 \
  --intensity 80 \
  --yes
```

Gekoppelte private Node:

```bash
sh /tmp/paritr-bootstrap.sh \
  --deployment docker \
  --portal-url 'https://wallet.example' \
  --pair-code 'PRTR-DEIN-CODE' \
  --yes
```

## Nicht-interaktive Windows-Installation

Native Node:

```powershell
Invoke-WebRequest https://paritr.highactive.de/downloads/bootstrap.ps1 -OutFile "$env:TEMP\paritr-bootstrap.ps1"
& "$env:TEMP\paritr-bootstrap.ps1" -Deployment native -Yes
```

Docker Desktop wird nur mit `-Deployment docker` installiert. Wenn WSL2 noch
nicht aktiv ist, fordert der Assistent einen Neustart und kann danach erneut mit
demselben Befehl ausgeführt werden.

## Verwaltung

Linux/macOS/FreeBSD:

```bash
~/paritr-node/manage.sh status
~/paritr-node/manage.sh logs
~/paritr-node/manage.sh doctor
~/paritr-node/manage.sh backup
~/paritr-node/manage.sh update
~/paritr-node/manage.sh pair PRTR-CODE https://wallet.example
```

Windows:

```powershell
Set-Location "$env:LOCALAPPDATA\Paritr\node-p9"
.\manage.ps1 status
.\manage.ps1 logs
.\manage.ps1 doctor
.\manage.ps1 backup
.\manage.ps1 update
.\manage.ps1 pair PRTR-CODE https://wallet.example
```

Dieselben Verwaltungsbefehle funktionieren bei nativer und Docker-Installation.
`config` zeigt nur eine redigierte Konfiguration; lokale Schlüssel und Tokens
werden nicht ausgegeben.

## Docker-Berechtigungen unter Linux

Der Installer muss den Benutzer nicht in die Gruppe `docker` aufnehmen. Wenn der
Socket für den Benutzer nicht erreichbar ist, fordert die Verwaltung gezielt
`sudo` an. Wer den Benutzer selbst der Docker-Gruppe hinzufügt, gewährt ihm damit
praktisch Root-Rechte auf dem Host.

## Daten und Updates

- Docker-Daten liegen im Volume `paritr-p9-data`.
- Native Daten liegen im Installationsverzeichnis unter `data/`.
- Eine erneute Installation übernimmt eine bestehende gültige P9-Konfiguration.
- `backup` stoppt die Node kurz für eine konsistente Sicherung.
- `update` lädt den aktuellen Setup-Assistenten samt Prüfsumme und führt die
  Installation erneut idempotent aus.
- Container werden ausschließlich über einen in `release.env` festgelegten
  Image-Digest gestartet.

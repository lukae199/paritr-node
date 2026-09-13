# Paritr Protocol 9 installieren und verwalten

Diese Anleitung beschreibt die Endanwender-Installation. Für die Veröffentlichung
der Pakete gilt zusätzlich `deployment/README.md`.

## Unterstützte Wege

| Plattform | `auto` verwendet | Alternativen |
|---|---|---|
| Linux x86-64/ARM64 | native Binärdatei | Docker Engine |
| macOS Intel/Apple Silicon | native LaunchAgent-Node | Docker Desktop |
| Windows 10/11 x64 | native Node | Docker Desktop mit WSL2 |
| Windows 11 ARM64 | native ARM64-Node | Docker nur bei unterstütztem Docker Desktop |
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

Der Assistent fragt nur nach Docker oder nativer Installation; auf Linux ist
die native Variante der Standard, damit mDNS ohne Container-Netzwerk-
Sonderregeln funktioniert. Mining,
Auszahlungsadresse, CPU-Leistung, RandomX Light/Fast, Gerätename, öffentliche
Adresse und Wallet-Pairing werden anschließend im lokalen Browser eingerichtet.

Der Installer fragt niemals nach Wallet-Seed, privatem Wallet-Schlüssel oder
Node-Admin-Secret. Pairing benötigt keine eingehende Portfreigabe.

## Lokale Browser-Verwaltung

Am Ende der Installation werden die individuelle `.local`-Adresse und das
zufällig erzeugte Admin-Secret einmal im Terminal angezeigt. Später lassen sich
beide mit `manage.sh access` beziehungsweise `manage.ps1 access` erneut abrufen.
Die Oberfläche ist normalerweise unter einer Adresse wie
`http://pnode-a7f3c9e1.local:5052` erreichbar; auf demselben Gerät funktioniert
immer `http://127.0.0.1:5052`.

Die Browser-Seite zeigt Status, Höhe, Peers, Hashrate, Laufzeit und Logs. Dort
lassen sich der Node-Betrieb starten/stoppen/neustarten sowie Mining, Threads,
Intensität, Light/Fast, Reward-Adresse, Gerätename, öffentliche URL,
Synchronisation und Portal-Pairing verwalten. Im gestoppten Zustand bleiben
Verwaltungsseite und Portal-Agent erreichbar, während P2P und Mining ruhen. Das
Admin-Secret verbleibt im `sessionStorage` des Browsers. Wallet-Seed und private
Wallet-Schlüssel gehören niemals in die Node-Oberfläche.

Für vorinstallierte Raspberry-/Orange-Pi-Geräte ist die native Installation zu
verwenden: Nur so wird der individuelle Hostname direkt per mDNS/DNS-SD im LAN
angekündigt. Jeder Erststart erzeugt eine dauerhaft gespeicherte 128-Bit-
Device-ID und daraus einen kollisionsarmen Namen `pnode-xxxxxxxx.local`. Ein
individuell gewählter Name wird vor dem Speichern per DNS-SD gegen andere
Paritr-Nodes im LAN geprüft. Für vorinstallierte Geräte sollte das beim
Provisionieren ausgegebene Admin-Secret auf einem Geräteaufkleber oder in einem
separaten Übergabeprotokoll festgehalten werden.

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
~/paritr-node/manage.sh access
~/paritr-node/manage.sh pair PRTR-CODE https://wallet.example
```

Windows:

```powershell
Set-Location "$env:LOCALAPPDATA\Paritr\node-mainnet"
.\manage.ps1 status
.\manage.ps1 logs
.\manage.ps1 doctor
.\manage.ps1 backup
.\manage.ps1 update
.\manage.ps1 access
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

- Docker-Daten liegen im Volume `paritr-mainnet-data`.
- Native Daten liegen im Installationsverzeichnis unter `data/`.
- Eine erneute Installation übernimmt eine bestehende gültige P9-Konfiguration.
- `backup` stoppt die Node kurz für eine konsistente Sicherung.
- `update` lädt den aktuellen Setup-Assistenten samt Prüfsumme und führt die
  Installation erneut idempotent aus.
- Container werden ausschließlich über einen in `release.env` festgelegten
  Image-Digest gestartet.

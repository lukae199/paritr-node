# Paritr Protocol 9: Node installieren und starten

> **Neuer Standardweg:** Für die geführte Ein-Befehl-Installation, automatische
> Docker-Einrichtung und die einheitliche Verwaltung verwenden Sie
> [`INSTALLATION.md`](INSTALLATION.md). Die folgenden Abschnitte dokumentieren
> weiterhin die manuelle Quellcode- und Plattforminstallation.

Diese Anleitung verwendet den Rust-Node `4.0.1-rc.1` und das neue Netzwerk
`paritr-mainnet`. Protocol 9 ist ein vollständiger Neustart. Alte
Protocol-8-Daten dürfen nicht als P9-Chainstate verwendet werden; die Installer
sichern erkannte P8-Daten automatisch.

## Vor dem Start

- Für einen reinen Full Node ist keine Wallet und keine Mining-Adresse nötig.
- Mining benötigt eine gültige Paritr-Adresse (`P...`). Der Node speichert
  keinen Wallet-Private-Key; die Adresse muss aus der Wallet beziehungsweise
  dem Portal stammen.
- Port `5050/TCP` ist die öffentliche RPC- und P2P-Schnittstelle.
- Port `5051/TCP` ist die Admin-Schnittstelle und muss ausschließlich auf
  Loopback (`127.0.0.1`) bleiben.
- `config.json` enthält das Admin-Secret, den Node-Identitätsschlüssel und
  gegebenenfalls das Portal-Token. Diese Datei niemals veröffentlichen.
- Solange noch kein öffentlicher P9-Seed läuft, ist `connected_peers: 0` normal.

## Welche Dateien müssen auf den Linux-Rechner?

Für einen nativen Quellcode-Build einschließlich Tests genügt dieses Paket:

```text
Cargo.toml
Cargo.lock
rust-toolchain.toml
rustfmt.toml
src/
tests/
spec/
install.sh
manage.sh
README.md
BUILDING.md
MIGRATION.md
SECURITY.md
NODE-START.md
```

Nicht benötigt werden `target/`, `.toolchains/`, `__pycache__/` sowie auf einem
reinen Linux-Ziel die Windows-Skripte. Die früheren Python-, PHP- und Logo-Dateien
sind bereits aus der P9-Auslieferung entfernt. `target/` darf nie
zwischen verschiedenen Betriebssystemen oder Architekturen kopiert werden.

Das bereits mit neutralen Unix-Verzeichnisrechten erzeugte Übertragungspaket
heißt:

```text
paritr-node-4.0.1-rc.1-source.tar.gz
paritr-node-4.0.1-rc.1-source.tar.gz.sha256
```

Das Archiv anschließend beispielsweise mit WinSCP oder `scp` auf den
Linux-Rechner übertragen und dort entpacken:

```bash
cd ~
sha256sum -c paritr-node-4.0.1-rc.1-source.tar.gz.sha256
mkdir -p ~/paritr-mainnet-source
tar -xzf ~/paritr-node-4.0.1-rc.1-source.tar.gz -C ~/paritr-mainnet-source --strip-components=1
chmod +x ~/paritr-mainnet-source/install.sh ~/paritr-mainnet-source/manage.sh
cd ~/paritr-mainnet-source
```

Das ältere Archiv ohne `-v2` wurde direkt aus einem OneDrive-Verzeichnis
erzeugt und kann schreibgeschützte Windows-Verzeichnisattribute enthalten. Es
darf auf Linux nicht mehr verwendet werden.

Für den Docker-Weg werden nur `Cargo.toml`, `Cargo.lock`,
`rust-toolchain.toml`, `rustfmt.toml`, `src/`, `Dockerfile`,
`docker-compose.yml` und `.dockerignore` benötigt.

## Windows 10/11 x64

### 1. Voraussetzungen installieren

Benötigt werden:

1. Rust über `rustup`;
2. Git;
3. CMake;
4. Visual Studio 2022 Build Tools mit **Desktopentwicklung mit C++** und dem
   aktuellen Windows SDK.

Danach ein neues PowerShell-Fenster öffnen und prüfen:

```powershell
rustc --version
cargo --version
git --version
cmake --version
```

Die Datei `rust-toolchain.toml` sorgt dafür, dass Rust `1.98.1` verwendet wird.

### 2. Validierenden Node installieren

```powershell
Set-Location 'C:\Users\lukae\OneDrive\Dateien\Development\blockchain\node4'
Set-ExecutionPolicy -Scope Process Bypass
.\install.ps1 -AssumeYes -Light
```

Die Standardinstallation liegt danach in:

```text
%LOCALAPPDATA%\Paritr\node-mainnet
```

Der Installer kompiliert den Rust-Node, baut die festgeschriebene
RandomX-v1.2.3-Bibliothek, erzeugt eine private Konfiguration, führt den
Selbsttest aus und startet die Node über die Windows-Aufgabenplanung.

### 3. Alternativ: Mining aktivieren

Nur nach Festlegung des gemeinsamen P9-Netzwerkstarts ausführen:

```powershell
.\install.ps1 `
  -Address 'P_DEINE_PARITR_ADRESSE' `
  -Fast `
  -Cores 0 `
  -Intensity 80 `
  -AssumeYes
```

`-Cores 0` verwendet die automatisch ermittelte Threadzahl. Fast Mode benötigt
ungefähr 2,1 GiB zusätzlichen RAM. Für einen sparsamen Validator `-Light`
verwenden. `-Fast` und `-Light` dürfen nicht gemeinsam angegeben werden.

### 4. Status und Logs prüfen

```powershell
Set-Location "$env:LOCALAPPDATA\Paritr\node-mainnet"
.\manage.ps1 check
.\manage.ps1 status
Invoke-RestMethod http://127.0.0.1:5050/health
Invoke-RestMethod http://127.0.0.1:5050/status
```

Erwartet werden Protocol `9`, Netzwerk `paritr-mainnet` und bei einer ganz
frischen Chain Höhe `0` mit Genesis-ID:

```text
44f076c3b96c8e7cb49605d13d04177cadb1f2e44faf9f5f2c249b5da32b320f
```

Logs verfolgen und Betrieb steuern:

```powershell
.\manage.ps1 logs
.\manage.ps1 stop
.\manage.ps1 start
.\manage.ps1 restart
```

Die Logansicht wird mit `Ctrl+C` verlassen.

### 5. Eingehende Verbindungen erlauben

Der normale Installer ändert die Firewall nicht. Optional kann der Installlauf
in einer PowerShell mit den nötigen Rechten um `-OpenFirewall` ergänzt werden.
Bei Betrieb hinter einem Router muss außerdem `5050/TCP` auf den Node-Rechner
weitergeleitet werden.

Für einen öffentlich beworbenen Node sollte TLS über einen Reverse Proxy
bereitgestellt und beispielsweise Folgendes beim Installieren angegeben werden:

```powershell
-PublicUrl 'https://node.example.org'
```

Der P2P-Endpunkt lautet dann `wss://node.example.org/p2p/v9`. Der Admin-Port
`5051` darf weder in der Firewall noch im Reverse Proxy veröffentlicht werden.

## Linux

### 1. Voraussetzungen (Debian/Ubuntu)

```bash
sudo apt update
sudo apt install -y build-essential cmake git curl ca-certificates
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
  | sh -s -- -y --profile minimal --default-toolchain 1.98.1
. "$HOME/.cargo/env"
rustup component add --toolchain 1.98.1 clippy rustfmt
```

Auf anderen Distributionen die entsprechenden Pakete für C/C++11, CMake, Git
und Rust installieren. Rust wird offiziell über `rustup` bereitgestellt. Cargo
lädt danach die in `Cargo.lock` festgeschriebenen Rust-Abhängigkeiten selbst.
`install.sh` lädt zusätzlich den festgeschriebenen RandomX-v1.2.3-Quellcode,
prüft dessen vollständige Commit-ID und kompiliert die Bibliothek lokal.

Diese Voraussetzungen sind nur für den Quellcode-Build nötig. Sie müssen nicht
bei jedem Node-Start neu installiert werden.

### 2. Installation

Validierender Light Node:

```bash
cd /pfad/zu/node4
chmod +x install.sh manage.sh
./install.sh --light
```

Damit sind Build, RandomX, Konfiguration, Integritätsprüfung und Systemd-Dienst
in einem Installationslauf vereint. `install.sh` installiert jedoch bewusst
keine Pakete per `apt`, weil Paketmanager, Paketnamen und administrative Regeln
zwischen Linux-Distributionen variieren.

Mining Node:

```bash
./install.sh \
  --address 'P_DEINE_PARITR_ADRESSE' \
  --fast \
  --cores 0 \
  --intensity 80
```

Der Standardpfad ist `~/paritr-node`. Unter Linux richtet der Installer einen
gehärteten Systemd-Dienst ein und fragt für die nötigen Service-Aktionen nach
`sudo`.

### 3. Betrieb prüfen

```bash
~/paritr-node/manage.sh check
~/paritr-node/manage.sh status
curl -fsS http://127.0.0.1:5050/health
curl -fsS http://127.0.0.1:5050/status
```

Weitere Befehle:

```bash
~/paritr-node/manage.sh logs
~/paritr-node/manage.sh stop
~/paritr-node/manage.sh start
~/paritr-node/manage.sh restart
```

Optional öffnet `--open-firewall` Port `5050/TCP` über UFW oder firewalld.
Router-Portweiterleitung und Reverse Proxy bleiben Betreiberaufgaben.

## macOS

Zuerst Xcode Command Line Tools, CMake, Git und Rust installieren. Danach wird
derselbe Installer wie unter Linux verwendet:

```bash
cd /pfad/zu/node4
chmod +x install.sh manage.sh
./install.sh --light
~/paritr-node/manage.sh check
~/paritr-node/manage.sh status
```

Der Installer erstellt einen LaunchAgent. Intel-x86_64 und Apple Silicon werden
nativ unterstützt.

## FreeBSD

Rust, CMake, Git und eine C/C++-Toolchain über `pkg` installieren, danach:

```sh
cd /pfad/zu/node4
chmod +x install.sh manage.sh packaging/paritr-node.freebsd-rc
./install.sh --light
~/paritr-node/manage.sh check
~/paritr-node/manage.sh status
```

Der Installer richtet das rc.d-Skript `paritr_node` ein. Installationspfade mit
Leerzeichen werden für FreeBSD absichtlich abgelehnt.

## Docker / OCI

Der Docker-Weg benötigt auf dem Host weder Rust noch Git, CMake oder einen
C/C++-Compiler. Diese Werkzeuge laufen nur in den isolierten Build-Stufen des
Images. Auf dem Host müssen lediglich Docker Engine, das Compose-Plug-in und
Internetzugang vorhanden sein.

Für einen nicht-minenden Node genügt:

```bash
cd /pfad/zu/node4
docker compose up -d --build
docker compose ps
docker compose logs -f paritr-node
curl -fsS http://127.0.0.1:5050/status
```

Falls `permission denied ... /var/run/docker.sock` erscheint, hat der aktuelle
Benutzer keine Berechtigung für den Docker-Daemon. Für einen einmaligen Test:

```bash
sudo systemctl start docker
sudo docker compose up -d --build
```

Für dauerhaften Zugriff ohne `sudo` kann der Benutzer der Docker-Gruppe
hinzugefügt werden. Diese Gruppe besitzt praktisch Root-Rechte; auf einem
Mehrbenutzersystem vorher das Sicherheitsmodell prüfen:

```bash
sudo usermod -aG docker "$USER"
newgrp docker
docker info
docker compose up -d --build
```

Die Konfiguration und Chain liegen im Volume `paritr-data`. Container und
RandomX werden für die jeweilige Zielarchitektur portabel gebaut.

## Späterer Binär-Release ohne Build-Werkzeuge

Sobald die Release-Pipeline einen signierten und veröffentlichten Build erzeugt
hat, muss auf einem normalen Linux-Zielsystem nur noch `install.sh` vorhanden
sein. Der Installer lädt passend zur Architektur eines dieser Archive samt
SHA-256-Datei:

```text
paritr-node-x86_64-unknown-linux-gnu.tar.gz
paritr-node-aarch64-unknown-linux-gnu.tar.gz
```

Dann sind auf dem Zielsystem kein Rust, Cargo, Git, CMake oder C++-Compiler
nötig; `curl`, `tar` und die normalen Systemwerkzeuge genügen. Dieser Weg ist
für spätere Betreiber empfohlen. Für den jetzigen Entwicklungsstand muss zuerst
ein Release-Tag durch die CI-/Release-Pipeline gebaut und auf dem konfigurierten
Download-Server bereitgestellt werden.

Mining vor dem ersten Start konfigurieren:

```bash
docker compose run --rm paritr-node init \
  --miner-address 'P_DEINE_PARITR_ADRESSE' \
  --enable-mining \
  --randomx-mode fast \
  --mining-threads 0 \
  --mining-intensity 80
docker compose up -d
```

## Weitere Peers eintragen

Die Standardkonfiguration versucht den festgelegten Seed. Für zusätzliche
Nodes zuerst den Dienst stoppen und `config.json` sichern. Anschließend können
HTTPS-/WSS-Adressen ergänzt werden:

```json
{
  "seed_nodes": ["https://node0.oe-net.de"],
  "peers": [
    "https://node1.example.org",
    "wss://node2.example.org/p2p/v9"
  ]
}
```

Eine URL ohne Pfad wird automatisch auf `/p2p/v9` ergänzt. Danach immer
`manage.ps1 check` beziehungsweise `manage.sh check` ausführen und erst dann
den Dienst neu starten.

## Portal koppeln

Windows:

```powershell
.\manage.ps1 pair 'PRTR-DEIN-CODE' 'https://portal.example.org'
```

Linux, macOS oder FreeBSD:

```bash
./manage.sh pair 'PRTR-DEIN-CODE' 'https://portal.example.org'
```

Das Portal ist optional. Der Node baut die Verbindung ausschließlich ausgehend
über HTTPS auf.

## Häufige Probleme

### `RandomX ... library was not found`

Den Installer erneut ausführen und prüfen, ob CMake sowie der C/C++-Compiler
verfügbar sind. In der Installation muss `randomx.dll` beziehungsweise
`lib/librandomx.so` oder `lib/librandomx.dylib` vorhanden sein.

### Node läuft, aber `connected_peers` ist `0`

Beim ersten P9-Node ist das erwartbar. Ansonsten DNS, HTTPS/WSS-Zertifikat,
Portweiterleitung, Firewall und den Endpunkt `/p2p/v9` prüfen. Alle Peers müssen
Protocol 9 und denselben Genesis verwenden.

### Windows-Build findet `cl.exe` oder das Windows SDK nicht

Visual Studio Build Tools erneut öffnen und **Desktopentwicklung mit C++** samt
Windows SDK installieren. Danach PowerShell neu starten.

### Konfiguration ändern

Node zuerst stoppen, `config.json` sichern, Änderung durchführen, `check`
aufrufen und den Node wieder starten. `admin_secret`, `node_private_key` und
`portal_agent_token` niemals in Support-Anfragen oder Logs kopieren.

## Freigabe vor einem öffentlichen Netzwerkstart

Ein lokaler erfolgreicher Start ist noch keine Mainnet-Freigabe. Vor dem
koordinierten Genesis-Start müssen die CI-Matrix, der RandomX-ABI-Selbsttest,
ein Mehrknoten-Testnetz, Restore-/Reorg-Tests und ein unabhängiges
Konsens-/Kryptografie-Audit abgeschlossen sein.

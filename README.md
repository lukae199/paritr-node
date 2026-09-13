# Paritr Protocol 9

Paritr Protocol 9 ist der Rust-Neustart des Paritr-Netzwerks. Er bewahrt das Grundkonzept – CPU-freundliches RandomX-PoW, direkte Zahlungen und eine Workshare-basierte 95/5-Vergütung – und ersetzt die langfristig problematischen Teile des Python-Prototyps durch deterministische Binärformate, eine echte State-Commitment-Struktur, atomare Speicherung und eine kumulative-Work-Forkwahl.

Für Endanwender stehen ein geführter plattformübergreifender Setup-Assistent,
Docker-/native Installation und einheitliche Verwaltungsbefehle bereit. Siehe
[`INSTALLATION.md`](INSTALLATION.md). Die zentrale Release- und Download-
Bereitstellung ist in [`deployment/README.md`](deployment/README.md) beschrieben.
Eine kompakte Dateiliste und die noch notwendigen Betreiberaktionen stehen in
[`DELIVERY.md`](DELIVERY.md).

Aktueller Stand: **4.0.1-rc.1**. Der Code ist vollständig ausführbar und getestet, aber vor einem wirtschaftlich relevanten Mainnet-Start sind ein unabhängiges Konsens-/Kryptografie-Audit und ein öffentlicher Mehrknoten-Test zwingend. „RC“ ist bewusst keine Behauptung, dass externe Prüfung bereits stattgefunden hat.

## Was P9 festlegt

- Chain-ID `paritr-mainnet`, Protocol `9`, fester neuer Genesis;
- RandomX **v1.2.3** mit Start-Selbsttest gegen den offiziellen v1-Testvektor;
- 64-Sekunden-Blöcke, deterministisches Integer-ASERT mit zwei Stunden Halbwertszeit;
- Workshares bei 32-fach leichterem Ziel (erwartet etwa zwei Sekunden), 95 % gleitender Pool / 5 % Finder plus Gebühren;
- 1.440-Blöcke-Vergütungsfenster und 100 Blöcke Reifezeit;
- Konten mit exakten Nonces, secp256k1-Low-S-Signaturen und P8-kompatiblen `P...`-Adressen;
- 256-stufiger Sparse Merkle Tree für Konten und ausstehende Rewards;
- deterministischer Little-Endian-Codec, authentifiziertes P2P, Header-/Block-Synchronisation;
- SQLite `WAL` + `FULL`, atomare Chain-/State-Commits und vollständige Revalidierung beim Start;
- öffentliche API auf 5050, lokale Admin-API auf 5051 und LAN-Verwaltungsseite auf 5052; Secret-geschützte Admin-Routen sind für direkte Nodes zusätzlich über deren HTTPS-Reverse-Proxy erreichbar, Remote-Verwaltung erfolgt bevorzugt über den eingeschränkten ausgehenden Portal-Agenten.

Die verbindlichen Regeln stehen in [spec/PROTOCOL-9.md](spec/PROTOCOL-9.md), maschinenlesbare Konstanten in [spec/test-vectors.json](spec/test-vectors.json). Die ursprüngliche Planung bleibt in [markdown.md](markdown.md), die alte Python-Node ist nur eine eingefrorene Protocol-8-Referenz.

## Schnellstart aus dem Quellcode

Erforderlich sind Rust 1.98.1, ein C/C++-Compiler, CMake, Git und RandomX v1.2.3.

```bash
cargo test --all-targets --locked
cargo build --release --locked
./install.sh --address P... --fast
```

Windows PowerShell:

```powershell
cargo test --all-targets --locked
cargo build --release --locked
.\install.ps1 -Address P... -Fast
```

Ohne Mining-Adresse startet ein validierender Light-Node. Der Installer erzeugt `config.json` mit zufälligem Admin-Secret, dauerhafter Device-ID und Node-Schlüssel, führt den RandomX-Selbsttest sowie die Datenbank-/Chain-Prüfung aus und richtet den nativen Service ein. Nicht kompatible Vorabdaten werden nicht gelöscht, sondern mit Zeitstempel gesichert.

## Betrieb

```bash
./manage.sh status
./manage.sh check
./manage.sh logs
```

Unter Windows entsprechend `manage.ps1`. Die öffentliche API liegt standardmäßig auf Port 5050, die Admin-API auf `127.0.0.1:5051` und die Secret-geschützte lokale Verwaltungsseite auf Port 5052. Port 5050 wird nur mit `--open-firewall` beziehungsweise `-OpenFirewall` freigegeben; 5052 und mDNS werden ausschließlich für private lokale Netze eingerichtet.

Containerbetrieb:

```bash
docker compose up -d --build
```

Weitere Details: [BUILDING.md](BUILDING.md), [MIGRATION.md](MIGRATION.md), [SECURITY.md](SECURITY.md).

## Vor dem Mainnet-Start

1. Genesis und alle Werte in der Spezifikation unveränderlich abzeichnen.
2. Unabhängige Rust- oder C++-Implementierung gegen die Vektoren laufen lassen.
3. Mindestens vier Wochen adversariales Multi-Arch-Testnetz inklusive langer Reorgs, Crash-Recovery, Epoch-Wechseln und Portal-Ausfall durchführen.
4. Externes Audit für Konsens, Signaturen, RandomX-FFI, P2P/DoS und Release-Lieferkette abschließen.
5. Reproduzierbare, signierte Release-Artefakte und Seed-Diversität herstellen.

Nach dem Genesis-Start dürfen Konsenswerte nicht mehr still geändert werden; jede Änderung benötigt ein neues Protokoll und explizite Aktivierung.

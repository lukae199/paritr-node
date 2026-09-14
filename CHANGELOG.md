# Changelog

## 4.0.1-rc.3 — neuer Protocol-9-Testgenesis

- Freigegebener Testketten-Neustart: ASERT ab erstem geminten Block; Anfangs-Difficulty
  rund 64, Zielblockzeit weiterhin 64 Sekunden und Share-Multiplikator 32.
- Separate genesisgebundene Datenbank ohne Löschung der bisherigen Daten.
- Raspberry-Pi-systemd-Pfad und automatische Docker-Compose-Plugin-Installation korrigiert.
- Live-Miningeinstellungen, zuverlässigere CPU-Drosselung und serialisierte RandomX-Initialisierung.
- LAN-Direktlink, Ladeanzeigen und Adresshinweis in der Verwaltung.
- Genesis-Prüfung im Portal; gemeinsames Node-/WebApp-Update erforderlich.

## 4.0.1-rc.2 — Protocol 9

- Windows ARM64: portabler RandomX-Interpreter statt inkompatiblem MSVC-A64-JIT,
  explizite Zielarchitektur, strikte Fließkomma-Rundung und sofortiger Build-Abbruch bei Fehlern.
- Admin-API und lokale Oberfläche teilen Port 5051; alte Standardkonfigurationen
  werden beim Laden von 5052 umgestellt. Angepasste Ports bleiben erhalten.
- Verwaltungsoberfläche nach UI-Entwurf mit Original-ParitrNode-Logo, responsivem
  Layout, geschützten Formulareingaben und sichtbaren Verbindungsabbrüchen.
- Mögliche Sperrverklemmung beim Abruf von Transaktionen behoben.
- Windows-Release prüft das fertige Bundle mit lokalem UI/API-Start und Authentifizierung.
- Keine Änderung an Konsensregeln, Genesis oder Netzwerk-ID.

## 4.0.1-rc.1 — Protocol 9

- Wallet-Portal-Pairing und Agent-Relay auf Protocol 9 vereinheitlicht.
- Lokale, mDNS-erreichbare Verwaltungsoberfläche für Status, Einrichtung und
  persistenten Start/Stop/Restart von P2P und Mining.
- RandomX Light/Fast über Node und WebApp konfigurierbar.
- Workshare-Multiplikator 32 und Chain-ID `paritr-mainnet`.
- Native Windows-ARM64-Artefakte und Linux-`noexecstack`-Prüfung ergänzt.

## 4.0.0-rc.1 — Protocol 9

- vollständiger Rust-Neuaufbau und neuer fester Genesis;
- deterministischer Konsenscodec statt Python-/JSON-Konsens;
- RandomX v1.2.3 mit ABI-/Vektor-Selbsttest und Light/Fast-Modus;
- Integer-ASERT, 64-Sekunden-Blöcke und kumulative-L1-Work-Forkwahl ohne künstliche Reorg-Grenze;
- selbsttragender Workshare-Witness, 16-faches Share-Ziel, exakte 95/5-Verteilung und 100-Blöcke-Reife;
- Konten/Nonces, Low-S-Signaturen, Sparse-Merkle-State und P8-kompatible Adressen;
- authentifiziertes binäres P2P mit Header-Sync und Abhängigkeitsnachforderung für Workshares;
- SQLite-Transaktionen, WAL/FULL, Netzwerk-/Genesis-Identität und Start-Revalidierung;
- getrennte Public/Admin-APIs und eingeschränkter Outbound-Portal-Agent;
- native Installer und Service-Helfer für Linux, macOS, Windows und FreeBSD sowie OCI-Dateien;
- Plattform-CI, feste Testvektoren und Migrations-/Security-Dokumentation.

# Changelog

## 4.0.1-rc.5 — Peer-Erkennung und Langzeitbetrieb

- Authentifizierte Peers tauschen begrenzte Listen öffentlicher Node-Endpunkte
  aus; der ausgehende Manager berücksichtigt neu gelernte Adressen laufend.
- Verbindungs-/Schreib-Timeouts, Frame-Limits, inaktive Verbindungen schließen,
  Wiederverbindung mit zeitlichem Zufallsversatz. Geprüfte DNS-Ergebnisse werden
  beim Socket-Aufbau wiederverwendet, private Gossip-Ziele bleiben ausgeschlossen.
- Block-/Share-Prüfung und häufige aufwendige API-Abfragen aus den asynchronen
  Netzwerk-Tasks ausgelagert; parallele Hintergrundprüfungen begrenzt.
- Keine Chain-Schreibsperre während PoW/SQLite; keine Share-Pool-Sperre während PoW.
- Wiederholte RandomX-Verifikation identischer Seed/Header-Paare begrenzt cachen.
- Normale SQLite-Kettenerweiterungen inkrementell statt kompletter Neuschreibung.
- Genesis, Konsens, Konten und Datenbankformat unverändert.

## 4.0.1-rc.4 — Installation und Reward-Anzeige

- Docker-Abschluss ohne Zugriff auf eine abgelaufene lokale Variable.
- Konfiguration beim Lesen nicht unnötig neu schreiben; eindeutige temporäre
  Dateien verhindern Konflikte zwischen Installer und Dienststart.
- Öffentliche URL, Pairing und Unpairing live übernehmen; Portal-Agent läuft
  auch vor dem ersten Pairing und übernimmt neue Zugangsdaten automatisch.
- Total mined und Wallet-Historie enthalten Finder- und Share-Rewards.
- Aktive Nodes nach authentifizierten Peer-Identitäten statt Reward-Adressen
  zählen (lokale Verbindungssicht, kein globaler Mining-Aktivitätsnachweis).
- Genesis, Reward-Aufteilung, 100-Block-Reifung und bestehende Guthaben unverändert.

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

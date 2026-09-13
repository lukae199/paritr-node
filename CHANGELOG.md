# Changelog

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

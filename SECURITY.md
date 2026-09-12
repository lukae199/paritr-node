# Sicherheit

## Sicherheitsmodell

Konsensdaten werden strikt binär dekodiert, vor Allokationen begrenzt und vollständig revalidiert. Der Node vertraut weder SQLite-Snapshots noch Peers: gespeicherte Blöcke werden beim Start ab Genesis geprüft, State-Roots werden neu berechnet, P2P-Hellos sind signiert und an Chain/Genesis gebunden. Die Forkwahl nutzt ausschließlich kumulative reale L1-Arbeit; Workshares können Rewards beeinflussen, aber keine Chain übernehmen.

Die Admin-API darf ausschließlich an einer Loopback-Adresse binden. Das Portal baut nur ausgehende HTTPS-Verbindungen auf und kann nur eine feste Liste ungefährlicher Leseoperationen, Transaktionsweiterleitung und Sync-Anstoß ausführen. `admin_secret`, `node_private_key` und Portal-Token dürfen nie veröffentlicht werden.

## Betreiberpflichten

- Binärarchiv gegen die veröffentlichte SHA-256-Datei und Release-Signatur prüfen.
- `config.json` nur für den Dienstbenutzer lesbar halten; sie enthält Node-/Admin-Geheimnisse, aber keinen Wallet-Private-Key.
- Admin-Port 5051 niemals weiterleiten, tunneln oder öffentlich freigeben.
- Öffentliche API hinter TCP-/HTTP-Rate-Limits und optional DDoS-Schutz betreiben.
- Uhrzeit per mehreren vertrauenswürdigen NTP-Quellen synchronisieren.
- Backups verschlüsseln und Restore regelmäßig testen.
- Fast-Mode nur bei genügend RAM nutzen; Light bleibt konsensidentisch.
- Seed-Nodes und Release-Hosting organisatorisch sowie geographisch diversifizieren.

## Meldungen

Sicherheitslücken zunächst privat an den Projektbetreiber melden und dabei Version, Plattform, Reproduktionsschritte, erwartetes/tatsächliches Verhalten und mögliche Auswirkungen angeben. Keine Secrets, Wallet-Keys oder fremden personenbezogenen Daten anhängen.

Vor einem Final-Mainnet sind externe Reviews für Konsensarithmetik, RandomX-FFI, secp256k1-Verwendung, P2P-DoS, Portal-Protokoll, Installer und Lieferkette erforderlich. Die vorhandenen Tests reduzieren Fehlerwahrscheinlichkeit, ersetzen aber kein Audit.

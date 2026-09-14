# Migration

## Testnetz-Neustart mit 4.0.1-rc.3

Der freigegebene Neustart korrigiert den ASERT-Zeitanker. Alle Nodes, Seeds und
die Plattform müssen gemeinsam aktualisiert werden; rc.2 ist nicht kompatibel.
Die Node verwendet automatisch `data/chain-<vollständige Genesis-ID>.sqlite`.
Die alte `chain.sqlite` einschließlich etwaiger WAL-Dateien bleibt unverändert.
Konfiguration, Device-ID, Wallet-Adresse, Admin-Secret und Pairing bleiben erhalten.
Alte Test-Guthaben, Rewards und Transaktionen gehören weiterhin nur zur alten Kette.
Vor der Umstellung den Dienst stoppen und Konfiguration sowie komplettes Datenverzeichnis sichern.
Details und Veröffentlichungsreihenfolge: `RELEASE-4.0.1-rc.3.md`.

## Migration von Protocol 8 auf Protocol 9

Protocol 9 ist ein absichtlicher Neustart, keine In-Place-Datenbankmigration. P8 und P9 besitzen verschiedene Chain-IDs, Genesisblöcke, Binärformate, Difficulty-Regeln, State-Commitments und P2P-Protokolle.

## Was erhalten bleibt

- private secp256k1-Schlüssel;
- die aus unkomprimierten SEC1-Public-Keys abgeleiteten Base58Check-`P...`-Adressen (Version 55);
- das Grundmodell RandomX + Workshares + direkte Kontenzahlungen;
- Bedienkonzept, Portal-Pairing und Standardport 5050.

## Was neu startet

- Chainstate und Blockhistorie;
- Nonces und ausstehende Rewards;
- Peer-Identität und Admin-Secret in der neuen Konfiguration;
- Workshare-Historie;
- wirtschaftlicher Supply von Höhe 0.

Der festgeschriebene P9-Genesis enthält einen leeren State. **P8-Guthaben werden deshalb nicht automatisch übernommen.** Falls vor dem finalen Start ein Snapshot/Claim-Verfahren politisch gewünscht ist, muss es vor Genesis als eigener, öffentlich geprüfter Konsensmechanismus spezifiziert werden; ein nachträgliches Editieren von SQLite oder Genesis wäre ein anderer, inkompatibler Fork.

## Sichere Umstellung

1. P8-Node sauber stoppen und Wallet-Private-Keys separat, offline und geprüft sichern.
2. `config.json`, `data/` und gegebenenfalls Portal-Zugangsdaten archivieren.
3. P9-Installer ausführen. Er erkennt eine abweichende Chain-ID und verschiebt die P8-Dateien in einen zeitgestempelten Backup-Ordner; er löscht sie nicht.
4. Die gewünschte bestehende `P...`-Adresse als Mining-Adresse eintragen. Niemals einen Private Key in `config.json` einfügen.
5. `manage.sh check` beziehungsweise `manage.ps1 check` ausführen und Genesis-ID `482443…2867c` kontrollieren.
6. Erst nach öffentlicher Bekanntgabe des Startzeitpunkts Mining, Seeds und Portal für P9 aktivieren.

P8- und P9-Nodes peeren aufgrund von Magic, Chain-ID, Protokollnummer und Genesisprüfung nicht miteinander. Ein Rollback bedeutet, den P9-Dienst zu stoppen und das unveränderte P8-Backup mit der alten Software separat zu starten; P9-Transaktionen existieren dort nicht.

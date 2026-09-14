# Release 4.0.1-rc.3

## Wichtig: neue Test-Blockchain

Dieser ausdrücklich freigegebene Neustart ist inkompatibel mit rc.2. Netzwerkname
`paritr-mainnet` und Protocol 9 bleiben erhalten. Neuer Genesis:

`4824434908d3cc1100e56146b813b95770bf85ada235b55c690404b4ea02867c`

ASERT verwendet den ersten geminten Block als Zeitanker statt des festen
Genesis-Datums. Angezeigte Anfangs-Difficulty: rund 64 (kompakte Targets runden),
Blockziel: durchschnittlich 64 Sekunden, Share-Multiplikator: 32.
64 Sekunden sind kein Mindestabstand; bei wechselnder Hashrate braucht die
Regel mit zwei Stunden Halbwertszeit Zeit zur Anpassung.

Die neue Datenbank heißt `chain-<Genesis-ID>.sqlite` im bisherigen Datenordner.
Alte Daten werden nicht gelöscht. Einstellungen, Wallet-Adresse, Geräteidentität
und Pairing bleiben erhalten; alte Test-Guthaben werden nicht übernommen.

## Korrekturen

- Raspberry Pi: gültiger systemd-WorkingDirectory-Pfad und Unit-Prüfung vor Installation.
- Docker Compose: fehlendes Plugin auf Debian/Ubuntu/Raspberry Pi OS bei Bedarf
  aus dem signierten offiziellen Docker-Repository installieren; vorhandene Engine behalten.
- Mining: Drosselung pro Hash; RandomX-Initialisierung serialisiert, um parallele
  Mehrfachallokationen desselben Datensatzes zu vermeiden; Container-RAM-Limit berücksichtigen.
- Threads, Intensität, Mining-Adresse und Mining ein/aus werden live übernommen.
  Light/Fast und Geräte-/Pairingänderungen benötigen weiterhin einen Neustart.
- UI: Ladeanzeige, Neustartfeedback, Hinweis auf fehlende Mining-Adresse.
  Validierung und Synchronisation laufen auch ohne Mining-Adresse.
- Installer zeigen LAN-IP mit Port 5051 und einen Direkt-Anmeldelink.
  Das Secret bleibt technisch nötig: LAN ist keine Authentifizierung und die
  Portal-Admin-Routen existieren auch auf Port 5050. Der vertrauliche Direktlink
  übernimmt das Secret automatisch aus dem URL-Fragment in den Sitzungsspeicher.
  5051 nicht öffentlich freigeben; HTTP-Direktlinks nur im vertrauenswürdigen LAN nutzen.
- Portal prüft den Genesis bei Verbindung, Pairing und Agent-Synchronisation,
  damit alte und neue Testketten nicht versehentlich vermischt werden.

## Veröffentlichung

1. Alle bisherigen Nodes einschließlich Plattform-/Seed-Nodes stoppen. Einstellungen
   und vollständige Datenordner sichern, auch die WebApp-Datenbank.
2. Dieses Repository committen/pushen und den neuen Tag `v4.0.1-rc.3` auf diesen
   Commit setzen. Alle GitHub-CI- und Release-Jobs müssen erfolgreich abschließen.
   Der Release-Publish wartet jetzt zusätzlich auf Formatierung, Clippy und
   einen gezielten Konsenstestlauf; ein fehlgeschlagener Check blockiert das Release.
3. Alle Assets dieses Releases herunterladen. Die lokalen `dist/*source*`-Archive
   sind Quellcode-Übergaben, keine fertigen plattformübergreifenden Binärpakete.
4. Mit `scripts/prepare-download-root.ps1` bzw. `.sh` den Download-Baum vorbereiten;
   Parameter siehe `deployment/README.md`. Alle erzeugten Dateien und Unterordner
   unverändert nach `https://paritr.highactive.de/downloads` hochladen.
   Versionsarchive bleiben gepackt, Skripte/Prüfsummen/STABLE liegen einzeln vor.
   `release.env` muss aus dem neuen Image-Digest erzeugt werden, nicht aus rc.2 kopieren.
5. `api.php` aus `paritr-webapp-4.0.1-rc.3-update.zip` in das WebApp-Root hochladen
   (vorher alte Datei sichern). Bei aktivem OPcache PHP-FPM/Cache aktualisieren.
   Der Response-Header `X-Paritr-Portal-API` lautet danach `4.0.1-rc.3-agent-relay-v5`.
6. Seed-/Plattform-Nodes und danach weitere Geräte auf rc.3 aktualisieren/starten.
   Docker-Volumes behalten; niemals `down -v` verwenden. Native Geräte können den
   neuen Installer erneut ausführen; er behält die Konfiguration und repariert die Unit.
7. Kurz auf einem Gerät prüfen: Version/Genesis, Port 5051, Pairing/Portal-Status,
   Live-Intensitätswechsel ohne Prozessneustart, anschließend Blockfortschritt.
   Auf Raspberry Pi zusätzlich `systemctl status paritr-node` bzw. `docker compose ps`.

## Prüfumfang und Grenzen

Gezielte lokale Format-/Syntaxprüfungen und Konsensvektor-Abgleich; zusätzliche
Regressionstests für ASERT-Startverzögerung und extreme Zeitabstände im bestehenden
Rust-Testmodul. Kein vollständiger Plattform-Testlauf auf dieser Windows-Maschine:
Der benötigte MSVC-Linker `link.exe` fehlt. GitHub-Builds, echte Pi-Installation
und Live-Portaltest bleiben vor Veröffentlichung erforderlich.
Der gemeldete sporadische Mining-Ausfall ist ohne Geräte-Logs nicht abschließend
diagnostiziert; die gefundenen Ressourcenprobleme sind korrigiert, eine allgemeine
Garantie gegen jeden Ausfall wäre nicht belastbar.

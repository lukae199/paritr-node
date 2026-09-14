# Release 4.0.1-rc.2 bereitstellen

Dieser Stand korrigiert den Windows-ARM64-Build und vereinheitlicht die lokale
Verwaltung auf Port 5051. Konsens, Genesis und `paritr-mainnet` bleiben unverändert.

## GitHub und Download-Host

1. Die Änderungen einschließlich `src/admin/logo.svg` und
   `scripts/smoke-management.ps1` committen und nach GitHub pushen.
2. Den neuen Tag `v4.0.1-rc.2` auf diesen Commit setzen und pushen.
   Einen erneuten Lauf des alten Tags zu starten würde dessen alten Code bauen.
3. Den erfolgreichen Abschluss aller Release-Jobs abwarten. Die Windows-Jobs
   prüfen den RandomX-Hashvektor und starten das fertige Bundle für einen kurzen
   lokalen API-/UI-/Authentifizierungstest. ARM64 verwendet den langsameren
   portablen Interpreter, x64 weiterhin den JIT. Beide bieten Light und Fast.
4. Alle Assets des neuen GitHub-Releases herunterladen. Die Quellarchive in
   `dist/` sind nur zur Übergabe des Repositorys gedacht, keine fertigen Binärpakete.
5. `./scripts/prepare-download-root.ps1 -Version 4.0.1-rc.2 -ArtifactDirectory ./release-assets -OutputDirectory ./download-root`
   beziehungsweise das entsprechende Shell-Skript ausführen; die exakten
   Parameter stehen in `deployment/README.md`.
6. Den Inhalt des vorbereiteten Download-Verzeichnisses nach
   `https://paritr.highactive.de/downloads` hochladen. Versionierte Archive bleiben
   gepackt. Installer, Prüfsummen, `release.env` und `STABLE` liegen einzeln an den
   vom Vorbereitungsskript erzeugten Stellen. `release.env` entsteht aus dem
   tatsächlich gebauten Image-Digest und darf nicht aus dem alten Release stammen.

## Installation und bestehende Geräte

Die Bootstrap-Befehle bleiben unverändert. Neue Installationen verwenden
`http://127.0.0.1:5051` beziehungsweise `http://pnode-<id>.local:5051`.
Der öffentliche Node-Port bleibt 5050. 5051 nur im vertrauenswürdigen LAN freigeben.

Beim Laden wird die alte Standardkombination aus Management-Port 5052 und
Loopback-Admin-Port 5051 auf einen gemeinsamen Listener umgestellt. Individuelle
Portkonfigurationen bleiben erhalten. Bestehende Docker-Installationen brauchen
die aktualisierte Compose-Datei samt Portmapping `5051:5051` und müssen neu
erstellt werden. Die Daten-Volumes dabei behalten. Eigene Firewall-Regeln anpassen.

Die bestehende Protocol-9-WebApp akzeptiert auch `4.0.1-rc.2`; für diese Korrekturen
ist kein weiteres WebApp-Dateiupdate erforderlich. Pairing nutzt wie bisher
ausgehendes HTTPS und den internen Admin-Zugriff auf 127.0.0.1:5051.

## Prüfumfang dieses lokalen Standes

PowerShell- und JavaScript-Syntax sowie Shell-Syntax geprüft, Layout und Logo im
Browser angesehen, Git-Diff auf Formatfehler geprüft. Der Rust-Prüflauf wurde
lokal durch Windows-Anwendungssteuerung blockiert. Die neuen GitHub-Builds und der
reale Pairing-/Installationslauf auf Zielgeräten sind noch nicht durchgeführt.
Eine erfolgreiche Live-Installation oder Veröffentlichung wird damit nicht behauptet.

# Lieferumfang: plattformübergreifende Paritr-Installation

## Endanwender

- `bootstrap.sh`, `bootstrap.ps1`: stabile Ein-Befehl-Einstiege;
- `setup.sh`, `setup.ps1`: geführte Erkennung, Abfragen und Installation;
- `install.sh`, `install.ps1`: geprüfte native Installation;
- `manage.sh`, `manage.ps1`: einheitliche Verwaltung für Docker und native Nodes;
- `docker-compose.release.yml`: abgesicherter Betrieb des digest-festgelegten
  Release-Images;
- zu jedem direkt ausgelieferten Bootstrap-, Setup-, Installations- und
  Verwaltungsskript sowie zur Release-Compose-Datei gehört eine `.sha256`-Datei;
- `INSTALLATION.md`: vollständige Benutzeranleitung.

## Build und Release

- `.github/workflows/ci.yml`: Format-, Lint-, Plattform- und RandomX-Tests;
- `.github/workflows/release.yml`: native Pakete, Multi-Arch-OCI-Image,
  SBOM/Provenance, Cosign-Signaturen, Quellarchiv und Prüfsummen;
- `scripts/prepare-download-root.sh` und `.ps1`: validieren ein Release und
  erstellen den Inhalt des öffentlichen Download-Verzeichnisses.

## Serverbereitstellung

- `deployment/paritr-downloads.nginx.conf`: statische HTTPS-Auslieferung mit
  unveränderlichem Cache für versionierte Dateien;
- `deployment/release.env.example`: Format des automatisch erzeugten,
  digest-festgelegten Image-Verweises;
- `deployment/README.md`: Tagging, Veröffentlichung, Hosting, Signaturprüfung
  und Aktualisierungsablauf.

## Noch vom Betreiber auszuführen

Die Dateien enthalten absichtlich keine Zugangsdaten, privaten Signierschlüssel
oder erfundenen Registry-Digests. Für eine öffentliche Installation sind daher
einmalig erforderlich:

1. Repository auf GitHub bereitstellen und Actions/Packages aktivieren.
2. Einen zur Cargo-Version passenden signierten Release-Tag pushen.
3. Das erzeugte GHCR-Paket öffentlich lesbar schalten.
4. GitHub-Release-Assets herunterladen und mit dem Vorbereitungsskript prüfen.
5. Das erzeugte Download-Verzeichnis auf `paritr.highactive.de` veröffentlichen.
6. Erst danach die Bootstrap-Befehle öffentlich bekannt geben.

Ohne diese Schritte würde ein Installer zwar korrekt arbeiten, könnte aber die
noch nicht veröffentlichten Binärpakete und Container-Digests nicht beziehen.

Ein Git-Repository ist für den reinen Node-Betrieb nicht notwendig. Es wird nur
für die mitgelieferte GitHub-Actions-Releaseautomation vorausgesetzt. Das
Kommandozeilenprogramm `git` wird unabhängig davon bei Tier-2-Quellinstallationen
zum geprüften Abruf von RandomX benötigt.

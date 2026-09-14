# Paritr-Installationen bereitstellen

Dieses Verzeichnis beschreibt die zentrale Bereitstellung der nativen Pakete,
des Multi-Arch-Container-Images und der Ein-Befehl-Installer. Der Download-Host
liefert ausschließlich statische, geprüfte Dateien über HTTPS aus.

## Git und Repository

Für Installation und Betrieb einer Node ist kein eigenes Git-Repository nötig.
Die hier beschriebene vollautomatische öffentliche Release-Lieferkette verwendet
GitHub allerdings als Build-, Signatur-, Paket- und Release-Plattform. Wer diese
Automatisierung nicht nutzt, muss dieselben Builds, Tests, Prüfsummen,
Signaturen und Uploads kontrolliert auf eigenen Build-Systemen ausführen. Das
Programm `git` bleibt bei einer lokalen Quellkompilierung erforderlich, weil der
Installer damit die festgelegte RandomX-Version abruft und deren Commit prüft.

## 1. Release erzeugen

1. Repository nach GitHub übertragen und GitHub Actions sowie Packages erlauben.
2. Einen signierten Tag passend zur Version in `Cargo.toml` erstellen, zum
   Beispiel `v4.0.1-rc.3`.
3. Den Tag pushen. `.github/workflows/release.yml` baut und testet:
   - Linux x86-64 und ARM64;
   - macOS Intel und Apple Silicon;
   - Windows x64 und Windows ARM64 nativ;
   - das OCI-Image für `linux/amd64` und `linux/arm64`;
   - das Unix-Quellarchiv mit echten Unix-Rechten.
4. Das Workflow signiert das Image schlüssellos mit Sigstore/Cosign und hängt
   alle Pakete, Installer, Prüfsummen und `release.env` an das GitHub Release.
5. Das GHCR-Paket öffentlich lesbar schalten, damit Nodes kein Registry-Login
   benötigen.

Der Image-Verweis in `release.env` enthält immer einen unveränderlichen
`sha256`-Digest. Ein Tag wie `latest` ist kein Installationsanker.

## 2. Download-Verzeichnis vorbereiten

Alle Assets eines GitHub Releases in ein lokales Verzeichnis herunterladen und
anschließend auf einem Linux-Rechner ausführen:

```bash
chmod +x scripts/prepare-download-root.sh
./scripts/prepare-download-root.sh 4.0.1-rc.3 ./release-assets ./download-root
```

Das Skript verweigert unvollständige Releases und prüft zuerst `SHA256SUMS`.
Danach enthält `download-root`:

```text
bootstrap.sh / bootstrap.ps1       stabile Einstiegspunkte
setup.sh / setup.ps1               aktuelle geprüfte Assistenten
v4.0.1-rc.3/                       unveränderliche Release-Dateien
STABLE                              aktuell freigegebene Version
```

Die äquivalente Vorbereitung unter Windows lautet:

```powershell
.\scripts\prepare-download-root.ps1 `
  -Version 4.0.1-rc.3 `
  -ArtifactDirectory .\release-assets `
  -OutputDirectory .\download-root
```

Den Inhalt anschließend beispielsweise mit `rsync` nach
`/srv/paritr-downloads/` übertragen. Ein vorhandenes Versionsverzeichnis darf
nicht überschrieben werden; erst nach erfolgreichem Upload werden die vier
stabilen Bootstrap-/Setup-Dateien ausgetauscht.

## 3. HTTPS-Host konfigurieren

`paritr-downloads.nginx.conf` wird in den bereits TLS-gesicherten Serverblock
von `paritr.highactive.de` eingebunden. Das Zertifikat, HSTS und TLS-Versionen werden
auf Ebene des Haupt-Serverblocks verwaltet. Danach:

```bash
sudo nginx -t
sudo systemctl reload nginx
```

Diese URLs müssen anschließend mit HTTP 200 erreichbar sein:

```text
https://paritr.highactive.de/downloads/setup.sh
https://paritr.highactive.de/downloads/setup.sh.sha256
https://paritr.highactive.de/downloads/setup.ps1
https://paritr.highactive.de/downloads/v4.0.1-rc.3/release.env
https://paritr.highactive.de/downloads/v4.0.1-rc.3/release.env.sha256
```

## 4. Öffentliche Installationsbefehle

Linux, macOS und FreeBSD:

```bash
curl -fsSL https://paritr.highactive.de/downloads/bootstrap.sh | sh
```

Windows PowerShell:

```powershell
irm https://paritr.highactive.de/downloads/bootstrap.ps1 | iex
```

Der Bootstrap lädt den eigentlichen Setup-Assistenten und prüft ihn vor der
Ausführung. Für eine Prüfung unabhängig vom Download-Host sollten Betreiber
zusätzlich `SHA256SUMS` aus dem GitHub Release vergleichen und die Cosign-Signatur
des in `release.env` genannten Image-Digests kontrollieren.

Insbesondere müssen `setup.sh.sha256` und `setup.ps1.sha256` unmittelbar neben
den jeweiligen Setup-Dateien liegen. Ohne diese beiden Dateien brechen die
Bootstrap-Skripte absichtlich vor der Ausführung ab.

## 5. Aktualisierungen und Rollback

`manage.sh update` beziehungsweise `manage.ps1 update` lädt immer zuerst den
aktuellen stabilen Setup-Assistenten. Bestehende Protocol-9-Konfigurationen und
Daten werden beibehalten. Vor einer Aktualisierung wird empfohlen:

```text
manage.sh backup
manage.sh update
manage.sh doctor
```

Ein fehlerhaftes Release darf nicht durch Austausch bestehender Dateien
"repariert" werden. Stattdessen wird eine neue Versionsnummer veröffentlicht
und erst nach bestandenen Tests zum stabilen Einstiegspunkt gemacht.

## Plattformgrenzen

- Linux x86-64/ARM64 verwendet standardmäßig die native Installation. Dadurch
  funktioniert die mDNS-Adresse ohne Docker-Netzwerk-Sonderfälle. Docker bleibt
  mit `--deployment docker` vollständig unterstützt.
- Windows 10/11 kann Docker Desktop mit WSL2 verwenden; ein Neustart kann nötig
  sein. Windows Server verwendet die native Node.
- macOS verwendet standardmäßig die native LaunchAgent-Installation; Docker
  Desktop bleibt optional.
- FreeBSD verwendet nativ rc.d, da Docker Engine dort kein gepflegter Zielpfad
  dieser Distribution ist.
- ARMv7, RISC-V, ppc64le und FreeBSD bleiben Tier-2-Ziele, bis reale Hardware-CI
  und Langzeittests bestanden wurden.

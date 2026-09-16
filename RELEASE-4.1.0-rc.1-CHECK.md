# Prüfung des aktuellen Protocol-10-Stands

Der fehlgeschlagene Test mischte P9-/ältere P10-Testvektoren mit dem neuen
Konsenscode. Die inzwischen lokal geänderten P10-Hash-Domänen verändern zusätzlich
Genesis-, State- und Workshare-Wurzeln. Diese Benutzeränderungen wurden beibehalten.

Aktuelle Genesis-ID:
`979eaa36eae4041b0c49281f64ed16bd963a646d9ee4807965c46f252240d216`

## Korrigiert

- Vollständiger Genesis-Vektor einschließlich aller Wurzeln, ID und Base64.
  Der bestehende Integrationstest prüft jetzt zusätzlich Genesis-Nachricht,
  Timestamp, Target, Transaktionswurzel, Wire-Magic und DAA-Fenster.
- Release-Publish wartet auch auf diesen Integrationstest; Protokoll-Metadaten 10.
- Die neue gewichtete 16-Block-DAA bleibt erhalten. Sie überspringt jetzt die
  Wartezeit vom festen Genesis-Datum bis zum ersten geminten Block. Die ersten
  beiden Blöcke verwenden das Starttarget, danach zählen echte Mining-Intervalle.
  Zeit-/Target-Gewichte und Begrenzungen bleiben ansonsten wie im Benutzerstand.
  Die Begrenzung gilt relativ zum gewichteten Target-Mittel, nicht zwingend zum
  letzten Block; die Regel beseitigt keine zufälligen Poisson-Schwankungen.
- redb prüft beim Laden zusätzlich fortlaufende Höhen und Block-Hash-Zuordnung;
  Snapshots oberhalb einer kürzeren Ersatzkette werden in derselben Transaktion entfernt.
- redb-Dateien nicht in Git/Docker-Kontext aufnehmen. Update-Installer stoppen
  vorhandene Dienste/Container vor exklusivem Datenbankzugriff. Ein fehlgeschlagenes
  natives Update kann den Dienst gestoppt lassen; nach Fehlerbehebung neu starten.
- `manage check` verlangt eine gestoppte Node; `doctor` nutzt bei laufender Node
  die API, statt eine zweite redb-Instanz zu öffnen. Bestehende Backup-Befehle stoppen
  den Dienst bereits während des Kopierens und bleiben dafür geeignet.

## Vor Veröffentlichung beachten

Dies ist kein kompatibles P9-Update und keine automatische SQLite-zu-redb-Migration.
Die neuen Genesis-/DAA-/Signaturregeln erfordern einen gemeinsamen P10-Neustart.
Alte Daten bleiben separat erhalten, Guthaben werden nicht automatisch übertragen.
Auch bereits gestartete P10-Testnodes mit anderen Genesis-Domänen dürfen nicht
mit diesem Stand vermischt werden. Die Version wurde nicht nochmals umbenannt.

Die benachbarte WebApp ist noch auf Protocol 9, Genesis `482443…2867c` und die
4.0.1-Versionserkennung eingestellt. Ihre Pairing-/Transaktionssignierung ist
damit nicht für diesen P10-Stand freigegeben. Nicht einfach nur die Protokollprüfung
entfernen: Portal-Prüfungen und Wallet-Signaturformat müssen gemeinsam auf P10
angepasst werden. Die WebApp wurde in dieser gezielten Node-Korrektur nicht verändert.
Reverse-Proxys müssen jetzt auch `/p2p/v10` weiterleiten.

Alle lokalen Änderungen zusammen committen und den Release-Tag auf diesen Commit
setzen. Ein erneuter Lauf eines alten Tags verwendet weiterhin dessen alten Code.
Alte `spec/PROTOCOL-9.md`-Texte sind keine Spezifikation der neuen P10-Regeln.

Die Genesis-Wurzeln und Bytes wurden unabhängig anhand der aktuellen Encoder- und
Hash-Regeln berechnet; der Integrationstest bleibt als Abgleich mit Rust bestehen.
Das ist keine vollständige kryptografische Prüfung oder ein Plattform-Langzeittest.
Formatierung, Skriptsyntax und unabhängiger Genesis-Abgleich waren erfolgreich.
Der einzelne lokale Rust-Integrationstest konnte nicht starten, weil redb im
Offline-Cargo-Cache fehlt. Ein erfolgreicher Rust-Testlauf wird damit nicht behauptet;
der GitHub-Lauf mit verfügbaren Dependencies muss ihn bestätigen.

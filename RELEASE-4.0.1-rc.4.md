# 4.0.1-rc.4: gezieltes Korrektur-Release

Kein Hard Fork, kein Datenbank-Neustart: Genesis und Konsens bleiben exakt wie rc.3.
Quellcode committen/pushen, Tag `v4.0.1-rc.4` auf diesen Commit setzen und den
erfolgreichen GitHub-Release abwarten. Den Download-Baum mit dem bestehenden
Vorbereitungsskript für Version `4.0.1-rc.4` aus den neuen Release-Assets erzeugen
und hochladen; keine alten Prüfsummen oder Image-Digests wiederverwenden.
Bestehende Geräte aktualisieren, Datenverzeichnisse und Docker-Volumes behalten.

## Behoben

- Docker: Abschlussfunktion verwendet den dauerhaften Compose-Pfad.
- Nativ: konkurrierende Konfigurationszugriffe verwenden keine gemeinsame
  `config.json.tmp` mehr. Ein reiner Lesezugriff schreibt nicht mehr unnötig.
- Öffentliche Adresse und Portal-Pairing/-Unpairing benötigen keinen Neustart.
  Neue URL gilt sofort für APIs/Portal und für nachfolgende P2P-Handshakes.
  Portal-Übernahme beim nächsten Poll; nach Verbindungsfehlern höchstens nach
  dem bestehenden Retry-Backoff. Gerätename und Light/Fast benötigen weiterhin Neustart.
- `total_mined` enthält alle bestätigten Finder- und Share-Rewards, einschließlich
  noch unreifer Rewards, unabhängig davon, welche Node den Block gefunden hat.
- Wallet-Historie zeigt auch Share-Auszahlungen an Nicht-Blockfinder; pro Block
  werden Finder- und Share-Anteil der Adresse zusammengefasst.
- Node-Zählung verwendet lokale authentifizierte Verbindungen plus eigene aktive
  Node, nicht die Zahl unterschiedlicher Wallet-Adressen. Eine Node kann ohne
  globale Topologieinformationen nicht alle indirekt verbundenen Geräte zählen.

## Rewards richtig verstehen

5 % der Subvention plus Gebühren gehen an den Finder. 95 % werden weiterhin
proportional zur bestätigten Share-Arbeit im Fenster der vorherigen 1.440 Blöcke
verteilt. Der Finder erhält auch seinen eigenen Share-Anteil, falls vorhanden;
Nodes ohne anrechenbare Shares erhalten keinen Pool-Anteil. Das ist unveränderter
Konsens, keine neue Auszahlungsregel. Die Historie und Total-Anzeige waren unvollständig.

Rewards aus Höhe H werden erst mit Block H+100 verfügbar. Daher darf available
balance anfangs 0 sein, während total mined und pending bereits steigen. Kein
zusätzlicher Wallet-Transfer ist nötig. Bleibt available nach der Reifung 0,
zuerst Höhe/Tip der Plattform-Node mit den Mining-Nodes vergleichen.

## Gezielte Prüfung

Format-/Syntaxkontrolle und ein zusätzlicher Regressionstest für die Historie
eines reinen Share-Empfängers. Der bestehende Konsenstest prüft die tatsächliche
Gutschrift nach 100 Blöcken. Vollständige Geräte-/GitHub-Läufe sind lokal mangels
MSVC-Linker nicht nachgewiesen. Alte dist-Archive enthalten diese Korrekturen nicht.

# 4.0.1-rc.5: Peer-Erkennung und Stabilität

Kein Hard Fork und kein Datenreset. rc.3/rc.4-Daten und Guthaben bleiben erhalten.
Den korrigierten Commit pushen und als `v4.0.1-rc.5` taggen. Nach erfolgreichem
GitHub-Build alle neuen Assets samt Prüfsummen über das vorhandene
prepare-download-root-Skript bereitstellen. Alle drei Nodes aktualisieren;
Docker-Volumes und native Datenordner behalten. Alte lokale dist-Archive wurden
nicht neu gepackt. Ein WebApp-Dateiupdate ist für diese Änderungen nicht erforderlich.

## Automatisches Peering

Zuvor wurde die vorhandene Peers-Nachricht ignoriert und der Verbindungsmanager
verwendete nur seine feste Startliste. Jetzt werden die im authentifizierten
Handshake bekanntgegebenen öffentlichen Adressen gelernt und alle 30 Sekunden
ausgetauscht. Neue Adressen werden ohne Node-Neustart berücksichtigt. Adressliste,
Parallelverbindungen, Retry-Rate und Nachrichtengrößen bleiben begrenzt.

Damit Node 2 und 3 direkt verbunden werden können, müssen beide eine öffentliche,
erreichbare HTTPS-Node-Adresse in der Verwaltung hinterlegen. Der Reverse Proxy
muss `/p2p/v9` als WebSocket zur öffentlichen Node-API (standardmäßig 5050) leiten.
5051 bleibt ausschließlich Verwaltung; diesen Port nicht öffentlich freigeben.
Das Verfahren öffnet keine Firewall-Ports, richtet keine Routerfreigaben ein und
implementiert kein NAT-Hole-Punching. Ohne eingehende Erreichbarkeit bleibt die
Verbindung über Node 1 korrekt und ausreichend zur Synchronisation. Bestehendes
mDNS für die Verwaltungsoberfläche ist kein LAN-P2P-Verbindungsmechanismus.

Die Peer-Limits gelten weiter: Kein unbegrenztes Vollnetz bei vielen Teilnehmern.
Bei drei erreichbaren Nodes und freien Peer-Slots sollten nach dem Austausch/
Retry alle je zwei Verbindungen erreichen. Seltene gleichzeitige Doppeldials
werden durch zeitversetzte Wiederholungen aufgelöst.

## Gefundene Lastursachen

- Teure P2P-Prüfungen liefen direkt auf Tokio-Netzwerk-Threads.
- Blockvalidierung und SQLite hielten die exklusive Chain-Sperre; wiederholte
  Share-Prüfungen hielten den Share-Pool gesperrt.
- Bereits geprüfte RandomX-Header wurden bei jedem verlängerten Share-Präfix
  erneut gehasht. Ein auf 8.192 Einträge begrenzter Cache vermeidet das; sämtliche
  anderen Konsensprüfungen sowie Target-Vergleiche bleiben erhalten.
- Die gesamte aktive Kette wurde bei jedem Block erneut in SQLite geschrieben.
  Normale Erweiterungen schreiben jetzt nur den neuen Suffix. Reorganisationen
  und Chainstate-Snapshots bleiben transaktional abgesichert.

Verbindungsaufbau und Schreibzugriffe haben jetzt Zeitlimits; inaktive Peers
werden beendet und neu verbunden. Aufwendige HTTP-Abfragen laufen begrenzt im
Hintergrund und können bei Überlast gezielt HTTP 503 statt endlosem Warten liefern.
Es wurden keine Proxy-Timeouts erhöht oder Konsensprüfungen abgeschaltet.

## Prüfumfang und kurzer Nachtest

Rust-Formatierung/Diff geprüft. Zwei gezielte bestehende Testbereiche ergänzen
Schutz gegen private Gossip-Ziele und gegen Neuschreiben unveränderter DB-Präfixe.
Der lokale Clippy-/Build-Versuch endet am fehlenden MSVC-Linker `link.exe`, bevor
der Projektcode vollständig geprüft werden kann. GitHub und reale Geräte müssen
die Änderungen daher noch bestätigen; ein fehlerfreier Nachtlauf ist nicht behauptet.

Nach dem Update einmal Version, Blockfortschritt und Peers prüfen. Falls weitere
504 auftreten, zur gleichen Uhrzeit Node-Logs, Container-/systemd-Restarts sowie
Reverse-Proxy-Errorlog sichern. Ohne diese Laufzeitdaten ist nicht bewiesen, dass
alle bisherigen 504 ausschließlich aus der Node und nicht aus Hosting/Proxy/RAM stammen.

> **Status 2026-09-12:** Dieses Dokument ist das ausgewertete Design-Memorandum, nicht mehr die normative Spezifikation. Die umgesetzten Regeln stehen in `spec/PROTOCOL-9.md`; feste Werte in `spec/test-vectors.json`. Die finale Implementierungsentscheidung ist ein sauberer, einzelner P9-Neustart. Die frühere Python-P8-Node ist aus der aktiven Quelle und der Auslieferung entfernt; der Installer kann vorhandene Altinstallationen weiterhin erkennen und sichern.

Ja. Gerade weil ein Hard Fork **jetzt noch realistisch möglich ist**, würde ich die Rust-Version **nicht einfach 1:1 als endgültige Architektur** aus der Python-Node übersetzen. Ich würde v3.2.3-rc1 als Referenz für Protocol 8 einfrieren, daraus Testvektoren erzeugen und dann gezielt einen **Protocol-9-Fork** entwerfen, der das Grundprinzip beibehält, aber einige langfristige Schwachstellen beseitigt.

Das Grundmodell würde ich ausdrücklich erhalten: RandomX als vollständige L1-PoW, Fork Choice ausschließlich nach echter kumulativer Blockarbeit, Workshares nur als Reward-Layer und niemals als L1-Gewicht, 64-Sekunden-Zielzeit, Account-/Nonce-Modell, 95/5-Aufteilung, rollierendes Reward-Fenster, Tail Emission und die Trennung von Consensus und Portal/Admin. Dass Workshares die L1-Fork-Choice nicht beeinflussen, ist eine der stärksten Eigenschaften der aktuellen Architektur. 

## Die Punkte, die ich vor einer langfristigen Rust-Version ändern würde

| BereichAktuell v3.2.3Langfristige EmpfehlungHard Fork |                                                         |                                                                       |                         |
| ----------------------------------------------------- | ------------------------------------------------------- | --------------------------------------------------------------------- | ----------------------- |
| Consensus-Serialisierung                              | Canonical JSON für TXIDs, State, Templates, Settlements | Explizites binäres Protocol-9-Format                                  | **Ja, sehr empfohlen**  |
| State Root                                            | nur alle 360 Blöcke, commitet Pre-Block-State           | Post-State-Root **in jedem Block**                                    | **Ja**                  |
| State Tree                                            | eigener, vollständig neu berechneter Baum               | inkrementeller, spezifizierter Sparse-Merkle-/Merkleized Account Tree | **Ja**                  |
| Reward Credits                                        | max. 128 Auszahlungen, größte Guthaben zuerst           | automatische maturity-basierte State Credits                          | **Ja, sehr empfohlen**  |
| Reorgs                                                | >100 Blöcke werden grundsätzlich abgelehnt              | schwerste gültige L1-Kette gewinnt unabhängig von Tiefe               | **Ja, sehr empfohlen**  |
| Workshare Templates                                   | historische Validierung braucht Sidecar-Templates       | selbstenthaltener, begrenzter Workshare-Witness                       | **Ja, sehr empfohlen**  |
| Workshare-Frequenz                                    | Multiplikator 64 ≈ 1 Share/s                            | langsamerer, gemessener Zielwert für globale Propagation              | **Ja, prüfen**          |
| Difficulty                                            | LWMA + zusätzliche Stall-Relief-Regel                   | eine einzige integerbasierte DAA, z. B. ASERT-artig                   | **Ja, nach Simulation** |
| Fee / Dust                                            | Mindestfee und Dust sind Consensus                      | Mindestfee/Dust primär Relay-/Mempool-Policy                          | **Ja**                  |
| Block Header                                          | 32-Bit Timestamp, Commitments indirekt via Coinbase     | Version-3-Header mit Height, u64-Zeit und direkten Roots              | **Ja**                  |
| Signaturen                                            | ECDSA secp256k1                                         | beibehalten, aber `libsecp256k1` verwenden                            | Nein                    |
| RandomX                                               | 1.x-Verhalten                                           | exakt RandomX **v1.2.3 pinnen**                                       | Nein                    |
| P2P                                                   | HTTP + WebSocket + JSON                                 | binäres P2P, persistente Peer-DB, mehrere Seeds                       | Nein                    |
| Storage                                               | SQLite + JSON-Blobs + Sidecar-DB                        | SQLite weiter möglich, aber binäre Records + atomare Migrationen      | Nein                    |

### 1. Canonical JSON würde ich aus dem Consensus entfernen

Das ist für die Rust-Portierung einer der wichtigsten Punkte.

Momentan werden beispielsweise Transaktionssignaturen, TXIDs und Teile des State über sortiertes Python-JSON definiert.  Der State-Leaf wird ebenfalls über `canonical_json()` gehasht. 

Das funktioniert innerhalb einer Python-Implementierung gut. Bei mehreren Implementierungen entstehen aber unangenehme Detailfragen:

```
```

```
Unicode escaping
integer representation
object ordering
JSON string escaping
invalid UTF-8
optional fields
parser behaviour
```

Ein einziges unterschiedlich serialisiertes Byte kann einen anderen:

```
```

```
txid
template_id
state_root
workshare_commitment
```

erzeugen.

RFC 8949 definiert zwar beispielsweise deterministische CBOR-Encodingregeln.  Für Paritr würde ich allerdings noch expliziter sein und ein **kleines eigenes binäres Consensus-Codec** definieren.

Etwa:

```
```

```
u8/u16/u32/u64       feste Little-Endian-Zahlen
[32]byte             Hash
varint + bytes       variable Daten
u64                  Amount in base units
```

Keine Floats. Keine JSON-Maps. Keine impliziten Defaults.

REST und Webportal dürfen weiterhin bequem JSON verwenden. Nur **Consensus und P2P-Wireformat** werden binär.

Das würde Rust, C++, Python und zukünftige Clients erheblich sicherer gegeneinander machen.

---

## 2. State Root würde ich grundlegend verbessern

Hier ist die aktuelle Implementierung zwar konsistent, aber für einen langfristigen Account-Chain-State unnötig kompliziert.

Aktuell gilt:

```
```

```
checkpoint = is_state_checkpoint(height)
expected_state_root = state_root(
    balances,
    nonces,
    reward_credits
) if checkpoint else None
```

und erst danach werden die Transaktionen angewendet. 

Damit commitet Block 360 beispielsweise effektiv den **State vor Block 360**, und nur alle 360 Blöcke existiert überhaupt ein State Commitment. 

Ich würde Protocol 9 wesentlich klarer machen:

```
```

```
Block H
   ↓
Parent State
   ↓
Transactions
   ↓
Rewards
   ↓
Post-State H
   ↓
state_root(H)
```

und **jeder einzelne Block** commitet seinen Post-State.

Dann bekommt man:

```
```

```
Block #10000
state_root = ABC...
```

und ein Snapshot zu #10000 lässt sich unmittelbar gegen genau diesen Block prüfen.

Das ist außerdem die Grundlage für sehr schnellen Node-Bootstrap. Bitcoin Core verfolgt mit AssumeUTXO ein verwandtes Modell: Ein Snapshot kann schnell geladen werden, während die vollständige Historie weiterhin im Hintergrund validiert wird. 

Bei Paritr wäre das dank Account-State sogar sehr natürlich.

### Dazu würde ich den State Tree inkrementell machen

Momentan wird der Root aus allen Accounts neu aufgebaut. 

Langfristig:

```
```

```
Balance Luka geändert
Nonce Luka geändert
Reward Alice geändert
```

sollte nur deren Merkle-Pfade aktualisieren.

Dann wächst die Berechnungszeit nicht linear mit der Anzahl aller existierenden Wallets.

---

## 3. Das aktuelle Reward-Credit-Payout-System würde ich ersetzen

Das ist wahrscheinlich die wichtigste ökonomische Verbesserung.

Heute werden Credits akkumuliert und dann so ausgewählt:

```
```

```
eligible = sorted(
    ...,
    key=lambda item: (-item[1], item[0]),
)[:MAX_POOL_PAYOUTS]
```

Damit werden maximal **128 Reward-Credit-Empfänger pro Block** ausgezahlt und die größten Credits zuerst.

Das ist deterministisch, aber langfristig kann ein kleiner Miner theoretisch immer wieder von größeren Guthaben verdrängt werden.

Ich würde das Account-Modell konsequenter nutzen.

Protocol 9 könnte Entitlements beispielsweise direkt als Consensus-State führen:

```
```

```
Block H
95%-Workshare Rewards
        ↓
pending_rewards[H + 100]
        ↓
100 Blöcke Reifezeit
        ↓
automatisch zu Balance
```

Damit gibt es:

```
```

```
keinen MIN_SHARE_PAYOUT
keine 128-Payout-Grenze
keine Payout-Starvation
keine bis zu 129 Coinbase-Transaktionen
```

Die Coinbase könnte dann wieder **genau eine** Finder-Transaktion sein:

```
```

```
5 % Subsidy
+ Fees
```

Die 95 % Workshare-Rewards werden als deterministische State-Transition behandelt.

Das passt meiner Einschätzung nach wesentlich besser zu deinem bestehenden Account-Modell als die heutige Simulation vieler Coinbase-Auszahlungen.

---

# 4. Die 100-Block-Reorg-Grenze würde ich aus dem Consensus entfernen

Das halte ich langfristig für wichtig.

Momentan:

```
```

```
if reorg_depth > MAX_REORG_DEPTH:
    raise ConsensusError(...)
```

Dadurch kann passieren:

```
```

```
Netz A                    Netz B
1000 Blöcke               1101 Blöcke
weniger Work              mehr Work
      \                   /
       Netzwerk verbindet sich
```

aber Netz A sagt:

```
```

```
Reorg >100
→ lehne schwerere Chain ab
```

Damit ist die Nakamoto-Konvergenz aufgehoben.

Ich würde weiterhin Schutzmechanismen einbauen:

```
```

```
"WARNING: deep reorg of 347 blocks"
extra validation
UI warning
rate limiting
disk safeguards
```

aber Consensus sollte letztlich sagen:

> Die gültige L1-Kette mit der höchsten kumulativen vollständigen RandomX-Arbeit gewinnt.

Das entspricht ohnehin bereits deinem eigentlichen Grundprinzip.

---

# 5. Workshare Data Availability muss gelöst werden

Das ist ein Punkt, den ich vor der Rust-Hauptversion unbedingt lösen würde.

Die Workshares selbst stehen im Settlement, aber zur vollständigen Validierung werden die referenzierten Templates benötigt.

Der Code hat deshalb einen eigenen:

```
```

```
workshares.sqlite
```

Store und sagt ausdrücklich:

> Archive mode is the default until a fully trustless snapshot/bootstrap protocol ships.

Langfristig bedeutet das:

```
```

```
Block vorhanden
Workshares vorhanden
Template fehlt
        ↓
neue Full Node kann historischen Reward-Beweis
nicht vollständig selbst validieren
```

Das ist für eine langfristige Blockchain nicht ideal.

## Meine bevorzugte Lösung: Workshare Witness

Block Protocol 9 könnte einen separaten:

```
```

```
workshare_witness
```

besitzen.

Darin liegen:

```
```

```
Workshares
+
alle tatsächlich referenzierten Templates
```

dedupliziert und binär komprimiert.

Der Header enthält nur:

```
```

```
workshare_root
```

Damit ist ein Block vollständig verifizierbar:

```
```

```
Header
Transactions
Workshare witness
```

Ein Pruned Node darf alte Witnesses später löschen.

Ein Archival Node behält sie.

Aber **jede historische Blockvalidierung kann grundsätzlich aus Blockchain-Daten erfolgen** und hängt nicht von einer separat erhalten gebliebenen Sidecar-Datenbank ab.

Dafür würde ich außerdem einen eigenen Witness-Byte-Budget definieren.

---

# 6. Den Workshare-Multiplikator 64 würde ich nochmals untersuchen

Deine aktuelle Version hat:

```
```

```
WORKSHARE_TARGET_MULTIPLIER = 32
```

Bei:

```
```

```
64 Sekunden Blockzeit
64 Workshares / Block
```

ergibt das im Erwartungswert:

```
```

```
1 Workshare pro Sekunde
```

Dein Workshare-System ist aber selbst eine **lineare Heaviest-Work-Sharechain**. Die Spitze wird nach kumulativer Share-Arbeit gewählt. 

Eine Sekunde ist für ein weltweit verteiltes P2P-Netz ziemlich aggressiv.

Wenn beispielsweise Share A in Deutschland gefunden wird und fast gleichzeitig Share B in den USA:

```
```

```
        Share A
       /
Parent
       \
        Share B
```

entsteht bereits eine Workshare-Fork.

Je kürzer die Sharezeit im Verhältnis zur Netzwerklatenz, desto höher der Orphan-Anteil.

Ich würde **nicht einfach wieder 32 einsetzen**, sondern mit echten Messungen bestimmen:

```
```

```
P50 propagation
P95 propagation
P99 propagation
```

und die Workshare-Zielzeit danach festlegen.

Wahrscheinlich würde ich für eine lineare Sharechain eher in Richtung **mehrere Sekunden** statt \~1 Sekunde gehen.

Die Reward-Glättung bleibt trotzdem enorm stark: Selbst bei nur 16 erwarteten Shares pro 64-Sekunden-Block wären das bei Zielzeit bereits über 20.000 Shares pro Tag.

Ein zusätzlicher Hinweis: Im aktuellen Code ist der Wert bereits `64`, aber einige Kommentare sprechen noch von „32x“.  Das verändert nicht die ausgeführte Logik, ist aber genau die Art von **Spec Drift**, die bei einer zweiten Implementierung gefährlich wird.

Vor Rust sollten Code, Spezifikation und Testvektoren deshalb aus **einer kanonischen Protokollspezifikation** erzeugt werden.

---

# 7. Die Difficulty würde ich ebenfalls jetzt überprüfen

Aktuell kombiniert Paritr:

```
```

```
LWMA-20
+
25–400 % Änderungsgrenze
+
Emergency Stall Relief nach 640 s
```

Das funktioniert und ist für ein junges Netzwerk nachvollziehbar.

Aber die zusätzliche Stall-Regel verwendet den Timestamp des neuen Blocks. Blocktimestamps sind innerhalb der erlaubten Grenzen Miner-kontrolliert.

Das würde ich langfristig lieber durch **eine einzige wohldefinierte DAA** ersetzen.

ASERT wäre ein ernsthafter Kandidat. Es arbeitet mit einem Anchor und einem kontinuierlichen exponentiellen Ausgleich. Die veröffentlichte Spezifikation betont insbesondere deterministische Integer-Arithmetik, um Cross-Platform-Abweichungen zu vermeiden. 

Ich würde allerdings **nicht blind BCH-Parameter übernehmen**.

Für Paritr müsste man simulieren:

```
```

```
64 s Blockzeit
Hashrate +90 %
Hashrate -90 %
Miner fällt plötzlich aus
Netzwerk startet mit sehr wenig Hashrate
24h stabile Hashrate
Timestamp-Angriffe
```

und danach eine passende Half-Life bestimmen.

Mein Ziel wäre:

```
```

```
eine DAA
keine Sonder-Notfallregel
nur Integer-Arithmetik
gleiche Ergebnisse auf Rust/C++/Python
```

---

# 8. Mindestfee und Dust sollten keine Consensus-Konstanten sein

Momentan lehnt ein Block Transaktionen ab, wenn:

```
```

```
amount < DUST_LIMIT
fee < MIN_RELAY_FEE
```

Damit ist beispielsweise:

```
```

```
MIN_RELAY_FEE = 0.001 PRTR
```

nicht nur eine Relay-Policy, sondern eine **Consensus-Regel**.

Wenn PRTR langfristig einmal einen völlig anderen wirtschaftlichen Wert besitzt, brauchst du einen Hard Fork, nur um die Mindestfee zu ändern.

Bei einem Account-Modell ist klassisches UTXO-Dust außerdem weniger problematisch.

Ich würde unterscheiden:

```
```

```
CONSENSUS:
amount > 0
fee >= 0
amount/fee <= MAX_MONEY

NODE POLICY:
minimum relay fee
minimum mempool fee
spam limits
RBF policy
```

Dann kann die Gebührenpolitik später geändert werden, ohne Blockchain-Fork.

---

# 9. Wenn wir sowieso einen neuen Header bauen, würde ich ihn zukunftsfester machen

Aktuell entspricht der PoW-Header im Wesentlichen dem klassischen 80-Byte-Bitcoin-Header:

```
```

```
version     u32
prev_hash   32 B
merkle_root 32 B
timestamp   u32
bits        u32
nonce       u32
```

Das ist simpel und gut.

Aber ein Protocol-9-Fork bietet die Chance, einen saubereren Header zu definieren:

```
```

```
version             u32
height              u64
previous_block      [32]
transactions_root   [32]
state_root           [32]
workshare_root       [32]
timestamp            u64
bits                 u32
nonce                u32
```

Das hätte mehrere Vorteile.

`height` wird direkt committed.

`state_root` ist direkt committed.

`workshare_root` ist direkt committed.

Und `timestamp` hat kein Jahr-2106-Problem mehr.

RandomX kann problemlos einen längeren Header hashen; sein Input ist nicht auf 80 Bytes beschränkt.

SHA256d kann weiterhin die Block-ID erzeugen und RandomX weiterhin die PoW prüfen.

Ich würde **SHA256d nicht ersetzen**. Dafür gibt es hier keinen überzeugenden Grund.

---

# 10. ECDSA würde ich ebenfalls nicht unnötig ändern

Deine derzeitigen Transaktionen verwenden secp256k1 und erzwingen bereits Low-S-Signaturen. Das ist grundsätzlich gut.

Für Rust würde ich die etablierte `rust-secp256k1`-Anbindung an `libsecp256k1` verwenden. Die zugrunde liegende Bibliothek ist speziell auf secp256k1 ausgelegt, konstantzeitoptimiert und sehr umfangreich getestet. 

Also:

```
```

```
ECDSA behalten
Adressen behalten
Private Keys behalten
bestehende Wallets behalten
```

Optional könnte Protocol 9 komprimierte 33-Byte-Public-Keys in Transaktionen erlauben, während aus dem dekomprimierten Schlüssel weiterhin dieselbe bestehende `P...`-Adresse berechnet wird.

Das spart Platz, ohne die Wallet-Adressen zu zerstören.

Schnorr würde ich **nicht nur deshalb einführen, weil wir ohnehin forken**.

---

# 11. RandomX würde ich behalten – aber exakt pinnen

Das ist für deine Multi-Architektur-Pläne wichtig.

RandomX v1.2.2 hat einen Fehler korrigiert, bei dem auf ARM/RISC-V extrem selten falsche Hashes erzeugt werden konnten – ungefähr 1 Fall in 268 Millionen. Die aktuelle 1.x-Version ist v1.2.3. 

Für Paritr würde ich deshalb explizit definieren:

```
```

```
RandomX consensus line = 1.2.3
exact upstream tag/commit pinned
```

für:

```
```

```
Windows x86_64
Windows ARM64
Linux x86_64
Linux ARM64
Linux RISC-V
macOS x86_64
macOS ARM64
```

Kein automatisches Wechseln auf RandomX 2.x.

Ein neuer RandomX-Major wäre eine **bewusste zukünftige PoW-Forkentscheidung**.

---

# Was keinen Hard Fork benötigt

Sehr viel können wir unabhängig vom Consensus verbessern.

Die Rust-Node würde ich intern mit Tokio und streng begrenzten Workqueues aufbauen. P2P sollte langfristig ein binäres, längenbegrenztes Protokoll mit Backpressure bekommen; REST bleibt nur für APIs. Node-IDs sollten durch langlebige Node-Schlüssel authentifiziert werden. Seed-Discovery sollte mindestens mehrere unabhängig betriebene Seeds besitzen. Peer-Adressen und Reputation sollten persistent sein. Admin bleibt ausschließlich lokal bzw. explizit freigegeben.

SQLite würde ich nicht vorschnell durch RocksDB ersetzen. SQLite ist plattformübergreifend, ausgereift und für deine Datenmenge gut geeignet. Ich würde aber statt JSON-Blobs versionierte binäre Blockrecords verwenden und Block, Index und Chainstate atomarer behandeln.

---

# Mein konkreter Zielzustand für **Protocol 9**

Ich würde damit folgende Regeln festschreiben:

1. **L1 bleibt Nakamoto RandomX PoW.** Nur vollständige Blockarbeit bestimmt die Hauptkette. 
2. **Native Workshare-Sharechain bleibt erhalten**, aber Zielintervall und Data Availability werden verbessert. 
3. **Versioniertes binäres Consensus-Format**, JSON nur noch für RPC/API. 
4. **Post-State-Root in jedem Block** mit inkrementellem Merkle-State und verifizierbaren Snapshots. 
5. **95/5 bleibt**, aber Workshare-Rewards laufen über maturity-basierte Consensus-State-Credits statt einer 128er Coinbase-Auszahlungsqueue. 
6. **Keine Consensus-Reorg-Grenze von 100 Blöcken**; höchste gültige kumulative L1-Arbeit gewinnt. 
7. **Eine einzige deterministische DAA**, wahrscheinlich ASERT-artig nach Simulation, ohne separate timestampbasierte Emergency-Regel. 
8. **Self-contained Workshare Witness**, plus Fee/Dust als Node-Policy und ein versionierter Header mit direkten State-/Workshare-Commitments. 

Das wäre aus meiner Sicht **deutlich besser als eine reine 1:1-Rust-Portierung von v3.2.3**.

Und wichtig: Dafür brauchst du **keine neue Blockchain und keinen neuen Genesis-Block**.

Wir können definieren:

```
```

```
Block 0 … H-1
Protocol 8
bestehendes Format

Block H
Protocol 9 Activation

Block H … ∞
Protocol 9
```

Die Rust-Node beherrscht dann **beide Validatoren**:

```
```

```
height < activation
→ validate_protocol8()

height >= activation
→ validate_protocol9()
```

Damit bleiben sämtliche bisher geschürften PRTR, Wallets und Historie bestehen.

Der jetzige Zeitpunkt ist dafür günstig: Wenn wir erst eine 1:1-Rust-Implementierung veröffentlichen und danach die fundamentalen Consensus-Datenstrukturen ändern, müssen wir praktisch zweimal dieselbe Arbeit machen. Deshalb ist **v3.2.3-rc1 als Protocol-8-Referenz eingefroren und Protocol 9 als eigenständiger Rust-Neustart umgesetzt**. Ein Dual-Mode-Client wurde bewusst verworfen: Er hätte alten Konsenscode und zusätzliche Angriffsfläche in den langfristigen Produktionsprozess getragen. P8-Schlüssel/Adressen bleiben kompatibel, Chainstate und Netzwerk starten klar getrennt neu.

# Build- und Plattformmatrix

## Voraussetzungen

- Rust **1.98.1** (durch `rust-toolchain.toml` fixiert; deklarierte MSRV: 1.98)
- C/C++11-Toolchain, CMake und Git für RandomX
- RandomX **v1.2.3**, vollständiger Tag-Commit `12f2c2ffe2108d6cf54c391fee33c8bc3646cdab`
- mindestens etwa 256 MiB zusätzlich für RandomX Light; Fast benötigt ungefähr 2,1 GiB Dataset-Speicher

```bash
git clone --depth 1 --branch v1.2.3 https://github.com/tevador/RandomX.git
test "$(git -C RandomX rev-parse HEAD)" = 12f2c2ffe2108d6cf54c391fee33c8bc3646cdab
cmake -S RandomX -B RandomX/build -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=ON -DARCH=native
cmake --build RandomX/build --config Release --parallel
cargo build --release --locked
```

Die Bibliothek kommt neben das Programm oder in `lib/`. Alternativ setzt `PARITR_RANDOMX_LIBRARY` einen absoluten Pfad. Beim Start prüft der Node die geladene ABI mit dem RandomX-v1-Referenzvektor und verweigert bei v2 oder einer beschädigten Bibliothek den Betrieb.

## Unterstützte Ziele

| System | Architektur | Status | Installation |
|---|---:|---|---|
| Linux (glibc) | x86_64, AArch64 | Release/Tier 1 | `install.sh`, Systemd oder Container |
| macOS | Intel x86_64, Apple Silicon | Release/Tier 1 | `install.sh`, LaunchAgent |
| Windows 10/11 | x86_64 | Release/Tier 1 | `install.ps1`, Task Scheduler |
| Windows 11 ARM | ARM64 | x64-Bundle unter Windows-Emulation | `install.ps1` |
| Linux | ARMv7 hard-float, RISC-V 64, ppc64le | Source/Tier 2 | lokaler Source-Build mit `install.sh`; Light empfohlen |
| FreeBSD | x86_64, AArch64 | Source/Tier 2 | `install.sh`, rc.d |
| OCI/Docker | linux/amd64, linux/arm64 | Release/Tier 1 | `docker buildx` / Compose |

Tier 1 wird nativ im CI getestet und als Release gebündelt. Tier 2 besitzt vollständige Build-/Servicepfade, benötigt vor einem Mainnet-Rollout aber Hardware-CI und Langzeittests. 32-Bit-Windows, iOS/Android, WebAssembly und Big-Endian-Konsens-Clients sind keine Full-Node-Ziele. Das Binärformat selbst ist architekturunabhängig; alle Zahlen sind explizit little-endian.

Der Windows-ARM-Weg ist absichtlich x64-emuliert: Der Rust-Kern lässt sich nativ für ARM64 prüfen, aber die gepflegte RandomX-v1-Windows-Bibliothek stellt dort keinen verlässlichen nativen Produktionspfad bereit. Linux/macOS AArch64 laufen nativ.

## Qualitätsprüfungen

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked
```

`paritr-node --config config.json check` lädt danach die echte RandomX-Bibliothek, führt ihren Selbsttest aus, öffnet SQLite mit Integritätsprüfung und validiert die gespeicherte aktive Chain vollständig bis zum festen Genesis.

## Release-Regeln

- ausschließlich `Cargo.lock` mit `--locked` verwenden;
- RandomX nie automatisch auf v2 aktualisieren;
- jedes Archiv mit separater SHA-256-Datei veröffentlichen und zusätzlich signieren;
- Release auf nativer Zielhardware mit `check` prüfen;
- die festen Vektoren und Genesis-ID vor Veröffentlichung vergleichen;
- keine RC-Binaries als auditiertes Final-Release bezeichnen.

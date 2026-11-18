# igs-iec61850-mms

An IEC 61850 client and server over MMS, in Rust: the application layer of IEC 61850 (ACSI, data model,
reports, controls, configuration by SCL) on the MMS stack of [igs-iso9506-mms](https://github.com/aklira/igs-iso9506-mms).

![Architecture of the crates and modules](docs/diagram.png)

The diagram shows the modules by layer: the SCL configuration (parser, model builder, validation), the IEC data model
(discovery, server model, ACSI types, standard catalogue), the MMS server (served model, data sets, value updates), the control
and report services (unbuffered and buffered reports, controls), and the client access (typed client, report, control and
buffered-report APIs).

## Status

Version 0.1.0 is the first release. Its API may change before 1.0.0.

What the library does (see [`docs/capabilities.tsv`](docs/capabilities.tsv) for the requirement rows and their tests, and
[`docs/statements.md`](docs/statements.md) for the generated statements):

- **Client**: typed discovery of devices, logical nodes and data objects; reads and writes; data sets (static and dynamic);
  unbuffered and buffered reports, with their configuration; controls (direct and select-before-operate, normal and enhanced
  security); the journal of a log (ReadJournal) and the files of a server.
- **Server**: serves a model built in code or from an SCL file (ICD/CID subset), with consistent value updates per node; serves
  data sets, unbuffered and buffered reports, controls, arrays, setting groups (SGCB), logs (journals fed by their log control
  blocks, GetLCBValues and SetLCBValues), and the files of a directory.
- **Time**: the host clock with its declared quality, or the embedded SNTP client (accuracy from the root distance; leap second
  announced by the server).
- **Security**: TLS 1.2 and 1.3 with mutual authentication and a revocation list (IEC 62351-3, transport only; see the gaps below).
- **Configuration**: SCL import with diagnostics (file, path, line), against the frozen 2007B schema; no external entities;
  atomic activation.
- **Gateway**: one runtime with clients and a server, explicit mirrors, quality invalid when a downstream association is lost,
  forwarded controls, and the replay of a downstream journal (called by the application after a reconnection).

What it does not do yet (details in [`docs/capabilities.md`](docs/capabilities.md)):

- buffered reports: the owner (`Owner`) is not served, and a general interrogation restarted while being sent is not restarted;
- Edition 1 interoperability is not tested against a peer;
- PTP is not implemented;
- no GOOSE and no sampled values; SetFile is refused;
- the TLS gaps G17 to G19, and IEC 62351-4 and -6 are not held;
- the library is not released for production use yet: see [`docs/deployment.md`](docs/deployment.md) for the configuration, and
  the production phase of [`IMPLEMENTATION.md`](IMPLEMENTATION.md).

Conformance drafts (ACSI statement, MICS, PIXIT) are in [`docs/conformance`](docs/conformance). They are drafts for review,
not a conformance claim.

## Using the crates

```toml
[dependencies]
igs-iec61850-mms = "0.1.0"
```

The client, the server and the gateway are examples in [`crates/igs-iec61850-mms/examples`](crates/igs-iec61850-mms/examples):

```sh
cargo run -p igs-iec61850-mms --example server -- 127.0.0.1:11102
cargo run -p igs-iec61850-mms --example client -- 127.0.0.1:11102
cargo run -p igs-iec61850-mms --example gateway
```

Two command line tools are included:

- `igs61850-client`: `domains`, `nodes`, `names`, `groups`, `definition`, `read`, `write`, `datasets`, `report` and `buffered`.
- `igs61850-server`: serves the demonstration model, or an IED of an SCL file with `--scl <file> --ied <name>` (built with the
  `scl` feature), and changes a status periodically with `--toggle-ms`.

```sh
cargo run -p igs-iec61850-mms --features scl --bin igs61850-server -- --listen 127.0.0.1:11102
```

## Building and checking

The repository builds without the licensed standards, which are not in it. The quality gate:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo test --workspace --all-features
cargo deny check
cargo run -p xtask -- statements --check
```

The minimum supported Rust version is 1.85. The sibling repository `igs-iso9506-mms` is a dependency through its published
crates; the CI clones it next to the workspace for the path dependencies of the development layout.

## Interoperability

[`docs/interop.md`](docs/interop.md) lists the rows of the interoperability bench, which runs the example programs of
libiec61850 as black boxes, in containers (`bench/`). Current rows: reports both ways, buffered integrity reports, and the
example clients against the demonstration server. The gaps are listed there too.

## Clean-room

The implementation is written from the IEC 61850 and ISO 9506 texts and the public examples of their behaviour. The source of
libiec61850 is not read or copied; the bench uses its example binaries as black boxes only.

## Licence

Apache-2.0 (see [`LICENSE`](LICENSE)), except the files generated from IEC Code Components, which keep the IEC copyright notice
and the `IEC-Code-Components-EULA` identifier in their headers. [`NOTICE`](NOTICE) lists them and their attribution.

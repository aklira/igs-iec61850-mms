# Deployment guide

This guide describes how to configure and run a station, a client or a gateway with `igs-iec61850-mms`, and what the library does when
the network or a peer fails. It states what is not done. It is a draft for review: the library is not yet released for production use
(the production phase of `IMPLEMENTATION.md`, X11, is open, and the legal review R1 blocks the publication). A reader who did not write
the library should be able to follow it; the maintainer checks that before the release.

## 1. What is in the box

| Component | Where | Use |
| --- | --- | --- |
| `igs61850-server` | `crates/igs-iec61850-mms/src/bin` | Serves the demonstration model, or an IED of an SCL file, over MMS |
| `igs61850-client` | same | Discovers, reads, writes, configures and receives reports |
| examples `server`, `client`, `gateway`, `measure`, `contention`, `contention_reports` | `examples/` | Starting points; built in CI |
| `igsmms-client` | `igs-iso9506-mms` (sibling repository) | The MMS client of the profile, with the logs and the files |

Build with the SCL import (needed for a configuration file):

```bash
cargo build --release -p igs-iec61850-mms --features scl --bin igs61850-server
```

## 2. Configuration file (SCL)

A station is configured from an SCL file (IEC 61850-6, the frozen 2007B schema). A logical device is named on the communication as its
`ldName`, or else the IED name followed by the device's `inst` (for the IED `IED1` and the instance `LD1`, the domain is `IED1LD1`; D-058).
Clients that name a domain must use these names. The import reads the IED named with `--ied` (default
`IED1`). It is checked as a whole: a refused element, a dangling reference or an unknown IED stops the activation, and the diagnostic gives
the file, the path and the line. Nothing is activated partially.

| Element | Served | Note |
| --- | --- | --- |
| Logical devices, nodes, data objects, attributes | yes | the namespace of the standard; mandatory data objects are checked |
| `DataSet` | yes | static data sets; a client cannot delete them |
| `ReportControl` (unbuffered and buffered) | yes | the owner and the reservation are as in the state machines (`docs/capabilities.md`) |
| `LogControl` and `Log` | yes | a journal of 512 entries per `Log`; the block records into it (`logEna`) |
| `SettingControl` | yes | the setting groups of the device (SG and SE values); the group control block is served |
| `GSEControl`, `SampledValueControl`, `Inputs` | no | refused with a diagnostic, not dropped (GOOSE and sampled values are not implemented) |

The `logName` of a `LogControl` must name a `Log` of the same logical node; otherwise the diagnostic is `SCL-003`.

Start a station:

```bash
igs61850-server --listen 0.0.0.0:102 --scl /etc/station/station.cid --ied IED1
```

Port 102 is the port of TCP/RFC 1006 used by MMS; binding it needs the privileges that the platform requires for a port below 1024.

Options of the server (`igs61850-server --help`): `--listen` (default `127.0.0.1:11102`), `--scl`, `--ied`, `--toggle-ms` (a demonstration
that changes a status periodically), and `--files-dir` (section 5).

## 3. Time

The timestamps of the data carry a quality (7-2 TimeQuality). The source of the time is a choice of the installation, to be written in the
PIXIT (`docs/conformance/PIXIT.md`):

- **The host clock** with the quality the operator declares for it (`HostClock::synchronised(leap_second_known, accuracy)`). The default is
  not synchronised. The library cannot check the synchronisation of the host: the declaration is the operator's statement.
- **The embedded SNTP client** (`igs_iec61850_mms::sntp::SntpClock`, run with `SntpClock::run(server_address)` on the UDP port 123). It sends a
  request every 64 seconds, checks each reply (mode, origin, kiss-o'-death, leap indicator, stratum), and derives the accuracy from the root
  distance of the reply (8-1 §6.4.2.2). A reply older than 1024 seconds no longer keeps the clock synchronised: the stamps are then not
  synchronised, not extrapolated.
- **Leap seconds.** A server that announces an insertion (leap indicator 1) makes the midnight stamp read as the leap second, with
  `LeapSecondsKnown` FALSE, as 7-2 Annex F.3 gives it. A deletion is not signalled, since 7-2 has no value for it.
- **PTP** (IEC/IEEE 61850-9-3) is not implemented.

The accuracy is a conservative bound, not a measurement (`docs/ai-log/O5.md`, D-052): a station whose clock needs a tighter statement must
measure it with its own means.

## 4. Security

The transport can run over TLS 1.2 or 1.3 with mutual authentication (IEC 62351-3), built from PEM material with
`igs_iec61850_mms::secure::secure_station` (certificate and key of the station, the trust anchors, the revocation list) and served with
`secured_server_config`. A client uses `SecureConnector` of `igs-iso9506-mms`.

Known gaps, which the statement of conformance says (`docs/conformance/PIXIT.md`, D-051):

- the mandatory TLS cipher suites of the profile are not all available in the backend (G17), the renegotiation of the TLS 1.2 certificates
  is not done (G18), and the certificate size is not checked (G19);
- IEC 62351-4 (the application association, the user) and IEC 62351-6 (GOOSE and sampled values) are not held.

A station that must conform to these points must not be deployed on this library until the maintainer decides them (X5).

## 5. Files

`igs61850-server --files-dir DIR` serves the regular files of `DIR` through the file services: their names are listed (FileDirectory),
read (GetFile) and deleted (FileDelete) by a client. Subdirectories and links are not followed. A file above 16 MiB stops the server at
its start. The store is in memory: a file deleted by a client is gone from the store, not from the disk, and it comes back at the next start.

SetFile (ObtainFile) is refused. The directory is loaded once at the start; the server does not watch it.

## 6. Logs

Each `Log` of the file is a journal of its node (`LN$name`, 512 entries; the oldest is overwritten). Each `LogControl` records the changes
of its data set into its journal while `logEna` is true: the transition of `LogEna` is an entry, and each change of a member is an entry with
the reference of the member, its value and its reason code. A client reads the journal with ReadJournal, from the start or from an entry
(QueryLogAfter), and recovers the entries after the last one it had after a lost association.

The log control block is read with GetLCBValues (its structure of 8-1 Table 75) and written with SetLCBValues (`LogEna`, `LogRef`, `DatSet`,
`TrgOps`, `IntgPd`). A reference cannot change while the block records.

Entry identifiers are eight octets, from one, increasing. The entries of a gateway replay keep the identifiers of the downstream journal: a
journal that also records its own changes must not be fed by a replay (see the gateway below).

## 7. Gateway

A gateway runs clients towards its downstream servers and a server towards its upstream clients, in one runtime
(`examples/gateway.rs`). Two mechanisms are explicit:

- **Mirrors**: each downstream variable is mapped to one upstream variable; nothing else is copied. The quality of the mirrored values is
  set invalid when the downstream association is lost.
- **Journal replay** (`igs_iec61850_mms::gateway::replay_journal`): the application calls it, after a reconnection, and it reads the
  downstream journal after the last entry replayed, and appends the missed entries to the upstream journal. Nothing is replayed by itself. It keeps the last identifier, so that a replay does not repeat an entry.
  The upstream journal should be fed by the replay only, or its identifiers must not collide with the downstream ones.

Controls received by the gateway are forwarded, and their termination is the outcome of the downstream.

## 8. What happens when a peer or the network fails

| Event | What the library does |
| --- | --- |
| A client's association is closed in order | Its reservations, its selections and its setting-group editing are released |
| A client's association is aborted or lost (no word) | The server finds the loss when it reads the end of the connection. The reservation is released then; a client that reconnects at once may find the block still reserved for a few milliseconds and must retry. The soak test of `tests/soak_reports.rs` measures this |
| A buffered report block loses its owner | The block is disabled; entries are kept in its bounded history. The client resynchronises from its last entry identifier |
| A downstream association of a gateway is lost | The mirrored quality is invalid until the association is back. The application then calls `replay_journal` to recover the missed journal entries (section 7); nothing is replayed by itself |
| The time source loses its synchronisation | The stamps carry not synchronised after the last reply's validity (1024 s for SNTP) |
| A peer sends hostile or truncated frames | The frame is refused and the connection closed; other clients are served (`tests/hostile_peer.rs`) |

## 9. Limits and defaults

- Names: 64 characters, by the IEC 61850 profile.
- Arrays: the number of elements is the size attribute of the instance, at most 4096.
- Reservations of a setting group: 60 seconds after the last selection, without confirmation.
- Journals: 512 entries. Buffered reports: a bounded history (`docs/capabilities.md`).
- Files: 16 MiB each.
- The server polls for changes every 50 ms by default. A report reaches its client about one poll after the change: the latency measured
  in `docs/robustness.md` is about 2 ms with a 1 ms poll and about 26 ms with the default.

## 10. Checks before running a station

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo run -q -p xtask --offline -- statements --check
```

The bench (`bench/`, podman-compose) runs the interoperability rows against the reference peer, as black boxes. Its results are in
`docs/interop.md`; the rows that are not passing are recorded there with their reason.

## 11. Troubleshooting

| Symptom | Cause and action |
| --- | --- |
| The server does not start with a file diagnostic `SCL-003` | A `LogControl` names a `Log` that its node does not declare: fix `logName` or add the `Log` |
| The server does not start with a file diagnostic `SCOPE-002` | An element is refused (GOOSE, sampled values, inputs): remove it from the file for this server |
| A write answers `temporarily-unavailable` | Another client holds the reservation or the editing; retry after it releases |
| A write answers `object-access-denied` on `LogRef` or `DatSet` | The block records; disable it (`LogEna` FALSE) first |
| A file is not listed | It is not a regular file of the directory, or it is larger than 16 MiB (the server did not start) |
| Stamps are marked not synchronised | The host declaration or the SNTP replies are not current; see section 3 |
| A report arrives late | The poll interval is the default 50 ms; see section 9 |

# Changelog

All notable changes to this project are recorded in this file. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). The capabilities and their tests are listed in
`docs/statements.md` (generated from `docs/capabilities.tsv`); the gaps are in `docs/capabilities.md`.

## [Unreleased]

### Fixed

- The published packages of `igs-iec61850-mms` and `igs-iec61850-mms-core` include `LICENSE` and `NOTICE`, with the copyright
  notice, the conditions and the disclaimer of the IEC Code Components licence that covers the generated files.
- The double-point status `Dbpos` is served as a two-bit coded enumeration (7-2 Table 24). A logical node with a
  breaker position (`XCBR.Pos`) is no longer refused for that type; an SCL file with a breaker is not yet checked end to end.
- The step command `Tcmd` is resolved as a two-bit coded enumeration (7-2 Table 23); its control follows with the control task.
- The trigger conditions, the report and the sampled value options (bit strings of their minimum size), the entry identifier and
  the entry time are served (8-1 §8.1.3.5, §8.1.3.8, §8.1.3.9, §8.1.3.13, §8.1.3.15). The tracking objects of the reports, logs and
  setting groups are served; the GOOSE and sampled value tracking, which holds a destination address, stays refused.
- Buffered reports: the general interrogation (`GI`, figure 30) sends the buffered entries first, then every member once; the
  reservation (`ResvTms`, 7-2 §17.2.2.17) keeps other clients out while it holds, and for its time after the loss of its holder.
  Only the loss of the owner disables a buffered block now.
- Logs: the log (a bounded buffer with entry identifiers) and its control block (recording of the changes of a data set, of the
  transitions of LogEna and of the integrity period) are state machines in the core, with the queries by time and after an entry
  and the status (7-2 §17.3). The MMS journal services are not served yet.
- Classes with setting-group variants are served: a device with a `SettingControl` has the setting values as SG and SE, a device
  without one as SP; the declaration of the setting groups is kept (7-3 §7.6.1; D-046).
- Arrays are served as one MMS array per attribute, with the number of elements of the instance (`maxPts`); the element access is not
  served (D-047).
- The general interrogation of a buffered block is an entry of its history, with its identifier, as the reference peer keeps it (D-044).

### Added

- Embedded SNTP client (TIME-002, TIME-003): `sntp` in the core builds the requests (mode 3), checks each reply (mode 4, origin,
  kiss-o'-death, leap indicator, stratum) and computes the offset and the delay of RFC 5905. The accuracy of the clock is its root
  distance, unadjusted and rounded up to bits of the fraction (8-1 §6.4.2.2); the clock is not synchronised without a reply and
  1024 s after the last one. `igs_iec61850_mms::sntp::SntpClock` is the UDP loop and the clock source (D-052).
- The secured profile of the station: `secure::secure_station` builds the TLS of a station from PEM material (mutual authentication and
  the revocation list), `secure::secured_server_config` serves the model over it. The profile's cipher suites and renegotiation are not
  conform yet (inherited gaps, D-051); IEC 62351-4 and -6 are not held.
- The setting group control block (SGCB) of a logical device, served as `LLN0$SP$SGCB`: its structure is read, SelectActiveSG,
  SelectEditSG and ConfirmEditSGValues are writes of its attributes, and SetEditSGValue is a write of the values with FC SE by the holder of the
  editing; the values of each group are kept, and the editing is released when its association is lost (IEC 61850-7-2 §16; D-050).
- Server: logical devices, nodes and data objects served as MMS variables, read and write of the served attributes,
  directory and definitions, application updates per node with one revision (`ServedModel`, `Updater`).
- Configuration: import of a declared subset of an SCL file (ICD, CID): model, data sets, addresses; the structure and
  semantic checks report the file, the path and the line. The frozen IEC 61850-6 2007B schema is checked, not an
  evolving one. Documents are bounded, external entities are refused, and a configuration is activated atomically.
- Data sets: static data sets (defined by the configuration) and dynamic ones, persistent and association-specific,
  with the client operations to define, read and delete them.
- Unbuffered reports: report control blocks as state machines (reservation, triggers, buffer time, integrity, general
  interrogation), the layout of table 74, delivery to the owning association, and the client API to configure and
  receive them.
- Controls: direct and select-before-operate control with normal and enhanced security, the typed client API
  (`select`, `select_with_value`, `operate`, `cancel`), deferred outcomes reported as command terminations, and the
  checks of the application.
- Gateway: one runtime with clients and servers, mirrors that set the upstream model from the downstream values
  (explicit), the quality set invalid when a downstream association is lost, and forwarded controls whose termination is
  the downstream outcome.
- Editions: detection of the edition from the namespace of the device, and the additional causes 26 and 27 read as the
  first edition exchanges them.
- Buffered reports: the buffered block as a state machine with a bounded history, its purge, its overflow flag and the
  resynchronisation to an entry after a lost association; the server serves the blocks declared in SCL, with the values
  of each entry kept with it, the wire layout of 8-1 table 74, delivery to the owner, and segmentation; the client reads,
  writes and decodes them (`client_buffered`).
- Command line client (`igs61850-client`, with `report` and `buffered`) and demonstration server (`igs61850-server`);
  examples for the client, the server and the gateway.

### Not implemented

- Buffered reports: the general interrogation and the reservation of a buffered block; the reverse direction against the
  reference client, and the resumption against the reference server (see `docs/interop.md`).
- Time-activated operate, and the security matrix (optional groups): refused or not provided.
- PTP (IEC/IEEE 61850-9-3): not implemented. The time comes from the host clock with its declared quality, or from the embedded SNTP
  client.
- Edition 1 interoperability is not tested against a peer yet.
- Writing the PICS, MICS and PIXIT drafts of the capability statements.

### Changed

- A data set of a logical node is named `LN$name` in MMS (for example `LLN0$Events`), and the `DatSet` attribute of a block
  is the object reference `device/LN$name`; writes take either form (D-040).
- A read of a control block (`node$RP$name`, `node$BR$name`) returns its attributes as one structure.

- Visible and Unicode string types are variable lengths with a maximum, as the standard has them (a fix of the
  string sizes, which refused any other length).

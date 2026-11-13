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

### Added

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
- Embedded SNTP and PTP: the host clock with its declared quality is used.
- Edition 1 interoperability is not tested against a peer yet.
- Writing the PICS, MICS and PIXIT drafts of the capability statements.

### Changed

- A data set of a logical node is named `LN$name` in MMS (for example `LLN0$Events`), and the `DatSet` attribute of a block
  is the object reference `device/LN$name`; writes take either form (D-040).
- A read of a control block (`node$RP$name`, `node$BR$name`) returns its attributes as one structure.

- Visible and Unicode string types are variable lengths with a maximum, as the standard has them (a fix of the
  string sizes, which refused any other length).

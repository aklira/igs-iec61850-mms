# Changelog

All notable changes to this project are recorded in this file. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). The capabilities and their tests are listed in
`docs/statements.md` (generated from `docs/capabilities.tsv`); the gaps are in `docs/capabilities.md`.

## [Unreleased]

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
- Command line client (`igs61850-client`) and demonstration server (`igs61850-server`); examples for the client, the
  server and the gateway.

### Not implemented

- Buffered report control blocks (history, purge, overflow, resynchronisation): refused.
- Time-activated operate, and the security matrix (optional groups): refused or not provided.
- Embedded SNTP and PTP: the host clock with its declared quality is used.
- Edition 1 interoperability is not tested against a peer yet.
- Writing the PICS, MICS and PIXIT drafts of the capability statements.

### Changed

- Visible and Unicode string types are variable lengths with a maximum, as the standard has them (a fix of the
  string sizes, which refused any other length).

# igs-iec61850-mms

An IEC 61850 client and server over MMS, in Rust: the application layer of IEC 61850 (ACSI, data model, reports, controls,
configuration by SCL) on the MMS stack of [igs-iso9506-mms](https://github.com/aklira/igs-iso9506-mms).

- **Client**: typed discovery, reads and writes, data sets, unbuffered and buffered reports, controls.
- **Server**: serves a model built in code or from an SCL file, with consistent value updates, data sets, reports and controls.
- **Gateway**: one runtime with clients and a server, explicit mirrors, quality invalid when a downstream association is lost.

The command line tools `igs61850-client` and `igs61850-server` are in this crate; `igs61850-server --scl <file> --features scl`
serves an IED of an SCL file.

Version 0.1.x is a first release: its API may change before 1.0.0. The capabilities, their tests and the gaps are documented in
the [repository](https://github.com/aklira/igs-iec61850-mms). Licensed under Apache-2.0.

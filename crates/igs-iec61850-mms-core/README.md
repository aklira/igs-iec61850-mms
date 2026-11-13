# igs-iec61850-mms-core

The sans-I/O core of [igs-iec61850-mms](https://crates.io/crates/igs-iec61850-mms): object references, ACSI types and their MMS
mapping, the server model and its layout, the service state machines (controls, unbuffered and buffered reports), and, with the
`scl` feature, the parser and the validator of IEC 61850-6 configuration files.

The state machines take an event and the current time, and return actions; they never read a clock or wait. The crate
`igs-iec61850-mms` runs them over the network.

Version 0.1.x is a first release: its API may change before 1.0.0. Licensed under Apache-2.0; the files generated from IEC Code
Components keep the IEC notice (see the repository's NOTICE).

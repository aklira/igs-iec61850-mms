# PIXIT (draft): the values of the implementation

Draft of the PIXIT of `igs-iec61850-mms`: the values that the implementation chooses or that the application sets, and the behaviour that
the standard leaves to the implementation. Each row names its evidence (`path::function`, a decision `D-nnn`, or a file), and
`cargo xtask statements --check` verifies it. A draft, to be completed by the integrator for each installation (X10).

| Item | Value or behaviour | Evidence |
| --- | --- | --- |
| Transport | TCP (RFC 1006) on the address the application gives the server; the default of the demonstration binaries is 127.0.0.1:11102 | docs/interop.md |
| Identifiers | up to 64 characters, by the IEC 61850 profile of the VMD | D-006 |
| Time source | the host clock with its declared quality (default: not synchronised), or the embedded SNTP client (the choice is the integrator's) | D-014, D-052 |
| SNTP client | requests every 64 s to the server the application gives (UDP 123); the last reply keeps the clock synchronised for 1024 s at most | D-052 |
| Time accuracy | the unadjusted root distance of the last reply, rounded up to bits of the fraction; above one second, not synchronised | D-052, crates/igs-iec61850-mms-core/tests/sntp.rs::the_accuracy_is_the_bits_of_the_fraction_the_distance_covers |
| Leap second | an announced insertion (leap indicator 1) marks the midnight stamp as not known (7-2 Annex F.3); an announced deletion (leap indicator 2) has no value in 7-2 and is not signalled | D-055 |
| PTP (IEC/IEEE 61850-9-3) | not implemented | D-014 |
| Setting groups | the number of groups and the active one come from the SCL file; the reservation of the editing lasts 60 s | D-050 |
| Setting group owner of a buffered block | the owner (BRCB Owner) is an empty octet string; open (X4) | D-053 |
| File store | the directory given to igs61850-server with --files-dir; regular files only, 16 MiB per file at most | D-054 |
| SetFile | refused | D-054 |
| Security | TLS 1.2 and 1.3 with mutual authentication and a revocation list (IEC 62351-3, transport only); cipher suites and renegotiation are not conformant (G17 to G19); IEC 62351-4 and -6 not held | D-051 |
| Edition | the edition is detected from the namespace; the causes 26 and 27 are read as the first edition gives them | crates/igs-iec61850-mms/tests/edition.rs::the_causes_26_and_27_are_read_in_the_first_edition_as_its_tissues_give_them |
| Reports | the buffered history and the reservation are bounded; the general interrogation is an entry of the history | D-044 |
| Logs | each Log of the file is a journal of 512 entries (the oldest is overwritten); the entries carry the reference, the value and the reason code of each change; the LCB over MMS is not served yet (X2) | D-045, D-056 |
| Measurements | recorded in docs/measurements.md and docs/robustness.md, for one machine and one workload | docs/robustness.md |

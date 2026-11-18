# Robustness on peer input (task X8)

This file records what was checked for input from a peer (RUN-002) and for load (RUN-004), with the workload and the hardware. The numbers
are for one machine and one workload; they are not an application performance claim.

## Hardware and software

- Processor: AMD RYZEN AI MAX+ PRO 395 w/ Radeon 8060S, 16 hardware threads, Linux (WSL2 kernel 6.18).
- Toolchains: stable for the tests and the examples; nightly with cargo-fuzz for the fuzz targets.
- Date of the runs: 2026-11-18.

## Review of the panics reachable from peer input

Method: the library code of the two crates (`igs-iec61850-mms-core`, `igs-iec61850-mms`) and the MMS crates of `igs-iso9506-mms` (codec,
server, link), outside their test modules, searched for `panic!`, `unreachable!`, `todo!`, `unimplemented!`, `unwrap`, `expect`, `assert!`
and `debug_assert!`.

- Result: no panic macro, no `unwrap` and no `expect` (`Option` or `Result`) in the library code of the IEC crates. The two `expect` hits of the
  sibling codec are a method of its own reader that returns a `Result`.
- The workspace lints enforce this on every build: `unwrap_used`, `expect_used`, `indexing_slicing` and `arithmetic_side_effects` are `deny`
  in `[workspace.lints.clippy]`, so a panic path of this kind fails the gate. The lints cover only the constructs they name: the review checked
  the other forms by search.
- Allocations sized from peer data: the one that reads the count of a buffered report (`client_buffered.rs`) is checked against the length of
  the bytes received before it allocates. The TPKT frame allocation is bounded by the 16-bit length of the frame.

## Hostile peer (`crates/igs-iec61850-mms/tests/hostile_peer.rs`)

A raw TCP peer sends, one connection each: 4096 pseudo-random octets, 17 octets of noise, a TPKT header with the length 0xFFFF and no body, a
TPKT header of length 4, a frame cut in the middle, a connection closed at once, and 65 536 octets of noise. After each frame a well-behaved
client connects and gets its Identify answered.

- Result: pass. The server closes the hostile connections and serves each client that follows.

## Soak (`crates/igs-iec61850-mms/tests/soak.rs`)

Each cycle opens an association, identifies, and ends it in one of three ways: an orderly close, an abort, or a drop without a word. Every
25 cycles a well-behaved client is checked.

| Cycles | Checks | Failures | Duration |
| ---: | ---: | ---: | ---: |
| 300 (default test) | 12 | 0 | 0.80 s |
| 5 000 (`IGS_SOAK_CYCLES=5000`) | 200 | 0 | 13.19 s |

Not measured: the memory of the server over a campaign of hours, and the associations with reports enabled over the cycles. A longer campaign
with those is for the maintainer.

## Fuzzing (`fuzz/`, cargo-fuzz, nightly)

Seven targets, each run for 60 seconds from its corpus. The first five are those of V3; `sntp_reply` and `file_store` are new.

| Target | Executions | Coverage (edges) | Result |
| --- | ---: | ---: | --- |
| scl_parse | 2 038 443 | 2300 | no crash, no sanitizer report |
| scl_build | 2 361 933 | 2014 | no crash, no sanitizer report |
| report_codec | 4 596 061 | 259 | no crash, no sanitizer report |
| urcb_events | 884 773 | 403 | no crash, no sanitizer report |
| control_events | 928 045 | 474 | no crash, no sanitizer report |
| sntp_reply | 19 970 428 | 132 | no crash, no sanitizer report |
| file_store | 745 978 | 452 | no crash, no sanitizer report |

- `sntp_reply`: the reply datagram, the poll and the instant of the reply are taken from the input. Two invariants: the stamp's fraction is in
  its range, and a clock that is not synchronised at the instant gives a not-synchronised quality.
- `file_store`: the services of the file store (add, open, read, close, delete, list, file) in sequences from the input. Two invariants: an added
  file is stored, and a read gives at most its limit.
- The artifact directories are empty: no input made a target fail.

Not done: longer campaigns (the 60 seconds are a check, not a campaign, as in V3), and a target for the journal of the core (`logs`), which is
fed by the application rather than by a peer.

## Contention of the shared model (RUN-004)

`crates/igs-iec61850-mms/examples/contention.rs`: N associations read one item in a loop on the demonstration model, while the application
calls `Updater::update` for the same item, 1 000 times, about 1 ms apart. Run: `cargo run --release --example contention -- <readers> 1000`.

| Readers | Updates | Reads completed | Update call p50 | p90 | p99 | max | Read p50 | p90 | p99 | max |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 0 | 1 000 | 0 | 0.003 ms | 0.005 ms | 0.006 ms | 0.036 ms | - | - | - | - |
| 4 | 1 000 | 24 034 | 0.003 ms | 0.003 ms | 0.005 ms | 0.046 ms | 0.385 ms | 0.446 ms | 0.514 ms | 1.006 ms |
| 16 | 1 000 | 98 446 | 0.003 ms | 0.004 ms | 0.029 ms | 0.075 ms | 0.372 ms | 0.509 ms | 0.599 ms | 1.017 ms |

- Reading: under read load the update call stays at about 3 µs at the median, and at 16 readers its 99th percentile rises from 5 µs to 29 µs.
  The model's lock is held for short sections; the reads do not delay an update by more than a few tens of microseconds on this machine.
- What this does not measure: the delivery of reports. The reports of a block are computed in the same serial model task, and their cost with
  many subscribers is not measured here: the workload has no report blocks, and the SCL fixture of the reporting tests is not in the example.
  RUN-004 asks for that measurement; it remains open (docs/measurements.md has the read measurements of V3).

## Open

- The reporting contention under multi-client load (above).
- The bench rows of the file services against the reference peer (X3): the reference example's directory convention is not known.

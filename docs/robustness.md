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

## Soak with reports enabled (`crates/igs-iec61850-mms/tests/soak_reports.rs`)

Clients reserve and enable the unbuffered block RPT1 of an SCL model, change the status, and end their association in one of three ways (an
orderly close, an abort, a drop without a word). Every 10 cycles a new client takes the block over and must receive a report.

| Cycles | Checks | Failures | Reservations retried after a drop | Duration |
| ---: | ---: | ---: | ---: | ---: |
| 150 (default test) | 15 | 0 | 0 | 1.5 s |
| 2 000 (`IGS_SOAK_REPORT_CYCLES=2000`) | 200 | 0 | 10 | 20 s |

- A first version of the test, without the retry, failed 5 times in 2 000 cycles. Every failure followed a drop: the next association met the
  reservation before the server had found the loss (D-059). The retry (up to one second) removes the failures; the count of retries is printed,
  so that the latency is seen.
- The orderly close does not show the race: 600 close-and-reconnect cycles, with and without a server change that was tried and not kept.

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
- What this does not measure: the reports. They are measured in the next section.

### Report delivery under multi-client load (RUN-004; `examples/contention_reports.rs`)

N clients each enable their own unbuffered block (RPT1 to RPTN) on a served SCL model, with the data-change trigger, no buffer time and no
integrity period. The driver changes the status `Ind1.stVal` 1 000 times, every 5 ms, and every change produces one report at every client.
The latency of a report is the time from the update call to its arrival at the client. The server polls for the changes every 1 ms in these
runs (`ServerConfig::poll_interval`). Run: `cargo run --release --example contention_reports --features scl -- <clients> <updates> <poll ms> <pace ms>`.

| Clients | Changes | Reports received | Out of sequence | Update call p50 | p99 | Report latency p50 | p90 | p99 | max |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 1 000 | 1 000 of 1 000 | 0 | 0.004 ms | 0.010 ms | 1.802 ms | 2.128 ms | 2.354 ms | 2.638 ms |
| 4 | 1 000 | 4 000 of 4 000 | 0 | 0.004 ms | 0.007 ms | 1.882 ms | 2.241 ms | 2.460 ms | 2.680 ms |
| 16 | 1 000 | 16 000 of 16 000 | 0 | 0.004 ms | 0.010 ms | 2.000 ms | 2.292 ms | 2.594 ms | 6.617 ms |

With the default poll interval of the server (50 ms), four clients and 200 changes every 60 ms (one change per poll):

| Clients | Changes | Reports received | Update call p50 | Report latency p50 | p90 | p99 | max |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 4 | 200 | 800 of 800 | 0.007 ms | 26.485 ms | 46.785 ms | 51.549 ms | 51.703 ms |

- Reading: the update call stays at a few microseconds with 16 clients, and every report arrives. The report latency is set by the poll
  interval of the server: about 2 ms with a 1 ms poll, and about the poll interval (26 ms median, 52 ms maximum) with the default of 50 ms.
  The latency grows by about 0.5 ms from 1 to 16 clients at the median on this machine.
- Honest limits: one change gives one report only when the pace is above the poll interval; a faster pace coalesces changes (a first run at
  1 ms pace, before the poll was lowered, received 688 reports of 1 000 changes). The count of reports is printed, so the case shows up.
  The workload has one data set of two members and one value changing; the reports of more members, of the integrity period and of the
  buffered blocks are not in it.

## Open

- The reverse row of the log example reads its journal with no entry: the example asks after a time with no change (docs/interop.md).

- The memory of the server over a long campaign (the soaks count the outcomes, not the memory).
- The bench rows of the file services against the reference peer (X3): the reference example's directory convention is not known.

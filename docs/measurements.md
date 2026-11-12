# Measurements (task V3, SPECIFICATIONS §12.4)

These are the first measurements of the library. They describe one machine and one workload, and they do not establish an application
performance class: §12.4 asks for targets from the intended deployment, which are not set here.

## Hardware and software

- Processor: AMD RYZEN AI MAX+ PRO 395 w/ Radeon 8060S, 16 hardware threads, Linux (WSL2 kernel 6.18).
- Build: `cargo build --release`, the example `measure` of `igs-iec61850-mms`, with the crate as it is at this commit.
- Date of the run: 2026-11-12.

## Workload

- One server in the process: the demonstration model, served on `127.0.0.1` (ephemeral port).
- N associations, all opened at once, each reads `IED1LD1/GGIO1$ST$Ind1$stVal` 500 times, one request at a time, one item per request.
- No reports, no secured transport, no reconnection, no large PDUs. Client and server share the process and the processor.
- Command: `./target/release/examples/measure <associations> 500`, one run per line below.

## Timing boundaries

- Latency: the time of one `read` call of the client, from the request to the decoded answer, on the client's task. It excludes the connection.
- Throughput: the reads completed divided by the time from the start of the first association to the end of the last one. It includes the
  connections of all the associations.
- Percentiles: nearest rank over all the reads of the run.

## Results (one run each)

| Associations | Reads | Failed | Duration | Reads/s | p50 | p90 | p99 | max |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 500 | 0 | 0.174 s | 2 877 | 0.329 ms | 0.399 ms | 0.472 ms | 0.563 ms |
| 8 | 4 000 | 0 | 0.208 s | 19 269 | 0.406 ms | 0.478 ms | 0.556 ms | 1.617 ms |
| 32 | 16 000 | 0 | 0.240 s | 66 575 | 0.405 ms | 0.593 ms | 0.800 ms | 1.076 ms |
| 64 | 32 000 | 0 | 0.368 s | 86 940 | 0.606 ms | 0.865 ms | 1.159 ms | 1.606 ms |

The runs are short (a fraction of a second each), so they show the order of magnitude, not a steady state. Each load was run once; the
variation between runs was not measured.

## Not measured

The following items of §12.4 are not measured in this release, and no number is given for them:

- Reports per second, event-to-enqueue delay, and enqueue-to-local-write delay (the reports of the server are not under load here).
- Buffer occupancy, overflow and loss diagnostics, and memory use under slow clients (buffered reports are not implemented, task B1).
- Secured transport, large PDUs, and reconnection bursts.
- Model size: the number of data sets and of nodes is that of the demonstration model only.
- The reference 50-client MMS benchmark of `igs-iso9506-mms` is a loopback baseline of another crate; it was not run here.

## Fuzzing

The fuzzing results are in `docs/ai-log/V3.md`, with the targets in `fuzz/fuzz_targets/`.

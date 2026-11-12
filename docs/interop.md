# Interoperability matrix (task A7)

The bench (`bench/`) runs our stack against the example programs of libiec61850 at tag `v1.6.2.1` (commit `c28c27c`), as
black boxes in containers. Every row captures its traffic and decodes it in tshark; a row fails on a decode error other
than the known Wireshark artefact of a zero-byte MMS PDU, which the F4 scenario already filters.

Run it with `BENCH_COMPOSE=podman-compose .venv/bin/pytest tests/test_a7_data_access.py tests/test_a7_reverse.py` in
`bench/`. The last run passed 13 of 13 rows.

## Matrix A: our client against the example servers

The client is `igs61850-client` with `--lenient-parameter-cbbs`: the reference server negotiates no parameter CBB (gap G8).
Each row runs the same script, which takes its node and its status object from the discovery: the domains, the nodes, the
functional groups of the node, one definition, a read of a status value, and a write of the same status, which a client must
not change (8-1 Table 27: the write is refused with `AccessViolation`).

| Row (example server) | Domain | Nodes | Status groups seen | Result | Features the row does not exercise |
| --- | --- | --- | --- | --- | --- |
| `server_example_basic_io` | `simpleIOGenericIO` | GGIO1, LLN0, LPHD1 | MX, ST, CO, CF, DC | pass | controls (K), reports, data sets |
| `server_example_substitution` | `IED1LD1` | GGIO1, LLN0, LPHD1, MMDC1 | MX, ST, SV, BL | pass | substitution writes (A4 covers them on our model) |
| `server_example_control` | `simpleIOGenericIO` | GGIO1, LLN0, LPHD1 | MX, ST, CO, CF, DC, OR | pass | operate and select of controls (K) |
| `server_example_dynamic` | `testmodelSENSORS` | GGIO1, LLN0, LPHD1 | ST, SP, RP, GO | pass | reports `events01`, `events02` and GOOSE `gse01` (B, D); data-set creation |
| `server_example_sg` | `DEMOPROT` | LLN0, LPHD1, PTOC1 | ST, CO, CF, DC, SP, EX | pass | setting groups (`SGCB`): select and edit (O3) |
| `server_example_files` | `simpleIOGenericIO` | GGIO1, LLN0, LPHD1 | MX, ST, CO, CF, DC | pass | file services (O1) |
| `server_example_service_tracking` | `simpleIOGenericIO` | GGIO1, LLN0, LPHD1, LTRK1 | MX, ST, CO, CF, DC | pass | service tracking (SR), not in the plan (see below) |

## Matrix B: example clients against our server

Our server is `igs61850-server`, serving the demonstration model (`IED1LD1`, with `GGIO1` and `MMXU1234567890123456`) on the IEC 61850
profile. The example client runs in the network namespace of our server and connects to port 102 on `localhost`. The row
checks the protocol: the association is accepted, and every confirmed request of the example is answered by a response or
an error PDU. The capture decodes cleanly. The functional part of each example is a gap, listed here.

| Row (example client) | Requests answered | What the example needs and our model does not give | Gap |
| --- | --- | --- | --- |
| `iec61850_client_example1` | 3 of 3 | the domain `simpleIOGenericIO` and the values of its `GGIO1`; our domain is `IED1LD1` | functional: the example reports read failures |
| `iec61850_client_example_array` | 5 of 5 | an array | arrays (A3, N4) |
| `iec61850_client_example_reporting` | 3 of 3 | data sets and control blocks | reports (B), data sets (D) |
| `client_example_control` | 5 of 5 | a control operation | controls (K) |
| `iec61850_client_example_log` | 3 of 3 | a log and its control block | logs (O2) |
| `iec61850_client_file_async` | conclude only | a file to transfer; the example sends no service request without one | file services (O1) |

## Matrix C: reports, our client against the reference server (task D6)

`bench/tests/test_d6_reports.py`: our client configures the unbuffered block `EventsRCB01` of the example `server_example_basic_io`
(`LLN0$RP$EventsRCB01`, data set `LLN0$Events`), enables it, and decodes the reports the reference server sends. The capture of each row
must decode without an error in tshark.

| Row | Result |
| --- | --- |
| General interrogation: one report of every member, reason general-interrogation | Passes: four entries, the reason decoded, the sequence number from zero |
| Integrity period of 1 s: periodic reports of every member, reason integrity | Passes: three reports with sequence numbers 0, 1, 2, four entries each |

The attributes of the block have the names of IEC 61850-8-1 table 73 in the reference server (`RptID`, `RptEna`, `Resv`, `DatSet`, `ConfRev`,
`OptFlds`, `BufTm`, `SqNum`, `TrgOps`, `IntgPd`, `GI`): the flattened form of D-028 reads them without change.

Not covered: the reverse direction (the reference example client receives the reports of our server). Our server serves the demonstration model,
which has no report control block; serving a configuration read from SCL is the step that makes this row possible (a gap of the `igs61850-server`
binary, see the table below). The buffered blocks (`EventsBRCB01` of the reference server) are in matrix C2.

## Matrix C2: buffered reports, our client against the reference server (task B2)

`bench/tests/test_b2_buffered.py`: our client configures the buffered block `EventsBRCB01` of the example `server_example_basic_io`
(data set `LLN0$Events`, the options of 8-1 table 74 with the entry identifier and the overflow flag), enables it, and decodes the buffered
reports the reference server sends for its integrity period. The capture must decode without an error in tshark.

| Row | Result |
| --- | --- |
| Integrity period of 1 s: buffered reports with an entry identifier and a sequence number | Passes: entries in order, each once, sequence numbers consecutive, reason integrity |

The peer's reports follow the wire order of 8-1 table 74 (the time of entry before the data set, the inclusion bit string in every report), which
the decoder reads (D-038). The first attempt failed on that order: the decoder now follows the table of the wire.

Not covered: the reverse direction (the reference example client receives the buffered reports of our server), and the resumption of the peer
after a lost association (our client sets an entry identifier of the peer and enables the block). Both are in the gap table below.

## Findings of the bench, and the fixes

- **The order of a listing.** The first version of the client refused a page of names that was not sorted. The reference
  server gives its names in its own order (`GGIO1$CO$SPCSO1$Oper$T` came after a later name), and the client refused its whole
  listing. A listing is now checked for repeats only, and no order is assumed (D-016, amended).
- **Negative sizes.** The reference server describes its quality and its strings with negative sizes (`BitString(-13)`,
  `VisibleString(-255)`). These are variable lengths with the absolute value as maximum (ISO 9506-2 §14.2.2.4.2). The client
  refused the definition of every status object before the fix (D-016, amended).
- **A server that does not describe `stVal`.** The control example describes its `Mod` status without a `stVal`. The client
  reports what the server describes; the script takes a status object that has a `stVal`.
- **A slow conclude.** The reference server answers the conclude late; the command-line client waited for it. The close is now
  bounded to two seconds.
- **Localhost frames.** The example clients connect to `localhost`, so their frames carry `127.0.0.1`, not the address of the
  container. The rows find the association by its PDU, not by its address.
- **The zero-byte PDU artefact.** Wireshark reports an internal error on a zero-byte MMS PDU in the reference traffic. The
  filter of F4 excludes it, and the same filter applies to every row.
- **The strength of the handshake.** The reference server refuses an association that does not negotiate the `str2` parameter CBB,
  which our client does not offer by default. The client's `--lenient-parameter-cbbs` option (A7) is needed for the peer, as for the
  data-access rows.

## Known gaps, with the task that closes each

| Gap | Task |
| --- | --- |
| Controls: select, operate, cancel | K |
| Reports: the reverse direction (the reference client receives our reports), which needs a configuration read from SCL in the server binary | D6 follow-up (the server binary serves an SCL configuration) |
| Buffered reports: the reverse direction, and the resumption from an entry identifier against the reference server (`EventsBRCB01`) | B2 follow-up (the server binary serves an SCL configuration, as for the unbuffered reports) |
| GOOSE | Out of scope of the plan |
| Data sets (dynamic creation and reading) | D |
| Setting groups (select and edit) | O3 |
| File services | O1 |
| Log services | O2 |
| Service tracking (SR) | Not in the plan: its scope is to be decided with the maintainer |
| Arrays in the served model; the client example reads one | A3 and N4 |
| Functional match of the example model (names and values of `simpleIOGenericIO`) | a model that mirrors the example, which is a new fixture |

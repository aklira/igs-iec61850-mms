# Conformance drafts: ACSI statement, PICS, MICS, PIXIT (task V1)

**Status: draft, for review.** This is not a conformance claim. The statements below restate the capability source
(`docs/capabilities.tsv`, the requirements named in brackets) and the values that the code gives. The review of these texts is the
maintainer's, and it belongs to the release gate (R1). They are written by hand in this release; the generation by `cargo xtask
statements` is open (task V1 stays unticked for that reason).

## 1. ACSI conformance statement (draft)

| Service area (IEC 61850-7-2) | State | Requirement |
| --- | --- | --- |
| Server directory, logical devices, logical nodes, named variables, definitions | Implemented (client side, typed; served by the model) | ACSI-001 |
| Data values: read and write of served attributes | Implemented (writes to controllable objects go through the control state machines); the attribute rows are in the register `docs/capabilities.md`, not yet in the source | MODEL-006 (updates); register rows for reads and writes |
| Data sets: static and dynamic, persistent and association-specific | Implemented | DS-001, DS-002 |
| Unbuffered reports (URCB): reservation, triggers, buffer time, integrity, general interrogation, layout | Implemented | RPT-001, RPT-002, RPT-003 |
| Buffered reports (BRCB): history, purge, overflow, resynchronisation, delivery, segmentation | Implemented; no general interrogation and no reservation for buffered blocks | BRCB-001, BRCB-002, BRCB-003 |
| Controls: direct and select-before-operate, normal and enhanced security | Implemented; TimeActivatedOperate refused | CTL-001, CTL-002, CTL-003, CTL-004 |
| Gateway (one runtime, mirrors, forwarded controls) | Implemented; no reconnection | GW-001, GW-003, GW-004 |
| Editions: detection from the namespace; the causes of the first edition | Implemented; the other causes of the first edition are not known | ED1-001, ED1-004 |
| Time: quality flags, clock loss, recovery | Implemented at the level of the clock source; no SNTP | TIME-001 |
| Logs, file services, setting groups, GOOSE, SV | Not implemented (no row claims them) | SCOPE-002 (refused when declared) |
| Security (optional group) | Not implemented | SEC-001 |

Declarations in SCL of a service the server does not execute are refused, not dropped (SCOPE-002); the SCL subset is in SCL-001 and
SCL-002. Checks are made against the frozen 2007B schema (SCL-002); the order of sequences is not checked.

## 2. PICS (draft)

The PICS lists the protocol features that the library supports on the IEC 61850 profile of IEC 61850-8-1, by the rows above.

- MMS services used by the server and the client: Read, Write, GetNameList, GetVariableAccessAttributes, DefineNamedVariableList,
  GetNamedVariableListAttributes, DeleteNamedVariableList, and the InformationReport of a report (RPT-002, BRCB-002). The other MMS
  services of the base stack are the stack's statement (igs-iso9506-mms), not restated here.
- Profile: the IEC 61850 profile with logical nodes (`Profile::iec61850`), data sets and arrays not declared in the model profile (`profile()`
  in `model.rs`). Arrays are refused by the model (D-008).
- Application context: the current one of the stack (the first edition is accepted through the edition detection, ED1-001).

## 3. MICS (draft)

The MICS names the logical node classes that the library can instantiate from a configuration, and the ones of the demonstration model.

- From SCL (`ServedModel::from_configuration`): any logical node class of the catalogue of IEC 61850-7-4 (2007B) whose data objects the
  document declares, checked against the namespace (SCL-002). Presence conditions other than mandatory are not evaluated: the declared member
  is instantiated (D-007).
- The demonstration model (`igs61850-server`): `GGIO` (the status `Ind1`, the analogue `AnIn1`, `Health`), `MMXU` (`TotW`), and the logical node
  `LLN0` with the mode of the device, as `demo.rs` builds them.

## 4. PIXIT (draft)

Values the implementation gives. They are the defaults of this release; a deployment chooses its own where the configuration allows.

| Item | Value | Where |
| --- | --- | --- |
| Transport: calling and called TSAP, SSAP, PSAP, AP title | none set by default | `LinkConfig::default` |
| Transport: TPDU size | 8192 octets (negotiated down to the peer's) | `LinkConfig::default` |
| Connect timeout / request timeout | 10 s / 30 s | `LinkConfig::default` |
| Outstanding requests, calling and called side | 8 / 8 | `LinkConfig::default` |
| Server identity (vendor, model, revision) | `igs`, `igs61850-demo`, the crate version | `server_identity` |
| Server poll interval | 50 ms | `DEFAULT_POLL_INTERVAL` (the MMS server) |
| Notices kept between two polls before the oldest is dropped (and counted) | 65 536 | `NOTICE_LIMIT` |
| Entries kept by a buffered block | 256 | `HISTORY_CAPACITY` |
| Segment budget of a buffered report | 4096 estimated octets | `SEGMENT_OCTETS` |
| Names | up to 64 characters (the profile's limit), device names as the SCL gives them | D-006 |
| Demonstration server listen address | `127.0.0.1:11102` | `igs61850-server --listen` |
| Model nesting | the profile's (`LOGICAL_NODE_NESTING_LEVEL`) | `server_config` |

## 5. Not claimed

- Any conformance class, certification or interoperability beyond the rows of the bench (`docs/interop.md`).
- The reverse direction of the reports against the reference client, and the first edition against a peer of the first edition (E3).
- Performance classes: the measurements of `docs/measurements.md` describe one machine and one workload, and set no class (§12.4).

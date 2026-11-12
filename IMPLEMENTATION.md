# Implementation plan

This plan drives the implementation of `igs-iec61850-mms` up to milestone M3 of
`SPECIFICATIONS.md`. It is written for an AI coding agent working task by task, with the method of
`igs-iso9506-mms`: **the agent decides and records; it does not stop to ask** (rule 10).

Product uses: a SCADA client facing an installed base of mixed editions, an IED-side server, and a
gateway/proxy combining both. The library builds on the MMS crates of `igs-iso9506-mms`; the MMS
extensions that IEC 61850 needs are made in that repository (phase N), under its own rules.

Requirement IDs (`MMS-001`, `RPT-001`, …) refer to `SPECIFICATIONS.md` version 0.2. That file is
kept local (gitignored); this plan stands on its own for the order of work.

---

## 1. Rules for the agent (read before every task)

1. **Clean room.** Never open, quote, reconstruct or describe the source code of libiec61850,
   IEC61850bean/OpenIEC61850, any other IEC 61850 or MMS implementation, or the MMS and IEC 61850
   dissectors of Wireshark. Reference implementations are only ever run as separate processes in the
   interop bench. Their user documentation (command lines, configuration options) may be read.
2. **No standard text in this repository.** IEC and ISO publications are licensed. Never paste text
   from `docs/spec/` into code, comments, docs, tests or commit messages. Never commit a file of
   `docs/spec/`, an NSD or XSD file whose licence forbids it, or an `.asn` file. Cite the clause
   (`// IEC 61850-8-1 §8.1.3.2`) and write in your own words. Facts such as tags, sizes, bit
   positions, service lists and enumeration values may be used as data.
3. **English only** for everything committed.
4. **Every protocol behaviour needs a source.** Take values from the data files of section 3, not
   from memory. If the standard is silent or ambiguous, choose the reading best supported by the
   clauses, record it as a `D-nnn` entry of `PROVENANCE.md` with its confidence and a test, and go
   on.
5. **One task at a time.** Read only the inputs listed for the task. Keep each change small (aim for
   under 400 changed lines per commit). Do not start a task whose dependencies are not checked.
6. **Done means green.** A task is done only when all its done criteria pass, plus the standard gate:
   ```
   cargo fmt --all --check
   cargo clippy --workspace --all-targets --all-features -- -D warnings
   cargo test --workspace --all-features
   cargo deny check
   ```
   Tasks of phase N also pass the gate of `igs-iso9506-mms` and the test suite of `igs-iec-tase2`.
7. **Commit per task, never push.** Nothing leaves this machine before task R1. Message:
   `<crate or area>: <what>` then a blank line and `Task: <ID>`. Tick the task's box in section 6
   in the same commit. Phase N commits go to `igs-iso9506-mms` with the same format.
8. **Safety.** `#![forbid(unsafe_code)]` in every crate. No `unwrap`, `expect`, indexing that can
   panic, unbounded recursion or unchecked arithmetic on data from the network or from SCL files:
   return `Result`. Bound every length, nesting depth and model size read from outside.
9. **Dependencies** must be MIT, Apache-2.0, BSD or ISC and pass `cargo deny`. Prefer the ones
   already used by `igs-iso9506-mms`. A new dependency is recorded in `PROVENANCE.md` with its
   reason.
10. **Decide and record.** The only human gate in the task list is R1 (⛔). Choices with written
    criteria are made by the agent and recorded in `PROVENANCE.md` and the ai-log. A failing
    criterion is fixed, or recorded as a known gap in `docs/capabilities.md`; it does not stop the
    plan. What stays with the user is listed in section 5: spending (buying publications), access
    to RTE equipment, legal questions, publication.
11. **Log prompts.** Archive the prompt and a short summary of each task in
    `docs/ai-log/<task-id>.md` (no standard text).
12. **Reading the licensed text.** The agent may read `docs/spec/` and the PDFs, local or online,
    under the authorization recorded in `~/Dev/norm_preparation/.claude/CONTEXT.md` §10 (IEC 61850:
    written authorization of RTE's legal department, same conditions as IEC 60870). It reads only
    what the task needs. Rule 2 still holds. A newly acquired publication has its licence notice
    checked before any processing.

---

## 2. Architecture (summary)

Cargo workspace:

| Crate | Role | I/O | async |
| --- | --- | --- | --- |
| `igs-iec61850-mms-core` | Sans-I/O: object references and MMS names, ACSI types and their MMS mapping, model tree and catalogue, datasets, report, control and (later) log and setting-group state machines, SCL import (feature `scl`), capability source | none | no |
| `igs-iec61850-mms` | Public API: `Client`, `Server`, gateway helpers, clock source; binds the core to the `Client` and `Model` of `igs-iso9506-mms`; binaries | TCP/TLS through the MMS crates | yes |
| `xtask` | Catalogue generation from the NSD files, capability-statement generation (not published) | files | no |

Why two crates rather than the single crate that SPECIFICATIONS §5 allows first: the service state
machines must be deterministic and testable with controlled time (RUN-004), and keeping them free of
tokio enforces it, as the link crate does in `igs-iso9506-mms`. Recorded as D-001 in F1.

Design rules:

- **Sans-I/O core**: state machines take events (MMS service indications, application updates,
  current instant) and return actions (responses, reports to send with their association, next
  wake-up, application notifications).
- **The MMS layers stay generic** (ARCH-001): no IEC 61850 semantics in `igs-iso9506-mms`. What
  phase N adds there is an MMS profile (limits, types, CBBs), not IEC 61850 behaviour.
- **Profiles are explicit** (ARCH-002): the TASE.2 profile keeps its limits; the IEC 61850 profile is
  chosen by configuration; the Edition 1 client profile is a variant of it.
- **One model, one mapping**: reads, writes, type discovery and reports all resolve against the same
  model tree (MODEL-003).
- **Stateful objects own their attributes** (MODEL-005): an MMS Write to a control or control-block
  attribute is routed to its state machine.
- **Strong types**: object references, FCs, MMS names and timestamps are newtypes; an invalid
  reference cannot be built.

Dependency on the MMS crates: the published versions in `Cargo.toml`, overridden by a local,
untracked `[patch.crates-io]` to `~/Dev/igs-iso9506-mms` while phase N changes are unreleased. CI
uses the released versions; a task that needs unreleased MMS changes keeps CI on the last release
and records it in the ai-log.

---

## 3. Sources of truth

`docs/spec/` is a local symlink to `~/Dev/norm_preparation/docs/spec/`; it is ignored by git and
not available in CI. Read `docs/spec/<part>/INDEX.md` first, then only the section you need.

**Data files (use these for any value that becomes code):**

| File | Content | Task |
| --- | --- | --- |
| `docs/spec/8-1/data/conformance_81.yaml` | PICS of 8-1 clause 24: profiles, Initiate parameters, MMS services, parameter CBBs, per-service conformance, each with base, client and server status and conditions | P3 |
| `docs/spec/8-1/data/mapping_81.yaml` | ACSI to MMS service relation, basic type mapping, Annex F extensions (tags, sizes, error codes) | P3 |
| `docs/spec/7-2/data/acsi_conformance.yaml` | ACSI conformance statement of 7-2 Annex A | P4 |
| `docs/spec/9506-2/data/asn1/variante_61850/*.asn` | MMS modules with the 8-1 Annex F extensions applied | P6 |
| `docs/spec/iec61850-code-components/` | NSD and SCL XSD releases, with their licence and checksums | P5 |
| `docs/spec/tissues/tissues.yaml` | Selected TISSUEs: number, part, status, batch, decision | P7 |
| `PROVENANCE.md` | Decisions on ambiguities | all |

**Sections by topic:**

| Topic | Sections |
| --- | --- |
| Communication profiles, A-profile, T-profile | 8-1 §6.2 |
| Time synchronisation | 8-1 §6.4, RFC 5905 |
| Object references and MMS names | 7-2 (ObjectReference), 8-1 §7.3, §8.1.3.2 |
| Basic and common types | 8-1 §8.1, 7-2, 7-3 |
| Server, association, LD, LN, data | 8-1 §9 to §13 |
| Datasets | 8-1 §14 |
| Service tracking | 8-1 §15 |
| Setting groups | 8-1 §16 |
| Reporting, logging | 8-1 §17, 7-2 reporting and log models |
| Controls | 8-1 §20, 7-2 control model |
| File transfer | 8-1 §23 |
| PICS | 8-1 §24 |
| SCL addressing, SCSM namespace | 8-1 §25, 61850-6 |
| MMS type extensions | 8-1 Annex F |
| Revision compatibility | 7-1 Annex K, 7-2 Annex F, 8-1 Annex K |
| Use of quality | 7-2 Annex D, 7-3 |
| RCB reservation | 7-2 Annex E |
| Namespaces and their versions | 7-1 (namespace rules), 8-1 §1.2 |
| Proxies and gateways | 7-1 §8.2.3, 7-4 LPHD |
| Conformance testing | 61850-10 |
| MMS itself | `igs-iso9506-mms` §3 |

Figures are missing from the prepared text. **The PDF is the authority** when text and data
disagree.

---

## 4. Phases and tasks

Each task lists: **Depends**, **Read**, **Produce**, **Done when**. Milestones of the specification:
M0 = phases P, F, N1–N4; M1 = phases A, S and D1; M2 = N5, D2–D6; M3 = phases B, K, E, G and V.

### Phase P — Inputs (done in `~/Dev/norm_preparation`, committed there)

**P1 — Prepared texts of 8-1 and 7-2**
- Produce: `prepare_norm.py` on 8-1 and 7-2; output in `docs/spec/8-1/`, `docs/spec/7-2/`.
- Done when: each `REPORT.md` is read and its points noted in `CONTEXT.md`.

**P2 — Prepared texts of 7-1 and 61850-6**
- Produce: the same for 7-1 and 61850-6. 7-3 and 7-4 are not prepared: their data comes from the
  NSD files (P5), and the PDF is read directly when a task needs a rule.
- Done when: as P1.

**P3 — 8-1 conformance and mapping data**
- Read: 8-1 clause 24, §8.1, Table 1, Annex F, in the PDF.
- Produce: `extract_conformance_81.py`, on the model of `extract_conformance_702.py`, writing
  `conformance_81.yaml` and `mapping_81.yaml` from the PDF (not from the prepared sections).
- Done when: the line-by-line comparison of TEMPLATE §6.1 matches the counts of short cells; the
  conditions (c1, c2, …) are kept with their text paraphrased.

**P4 — ACSI conformance data**
- Read: 7-2 Annex A.
- Produce: `acsi_conformance.yaml` by script.
- Done when: as P3.

**P5 — IEC code components**
- Produce: the NSD files of 7-2, 7-3, 7-4 and 8-1 and the SCL XSD release matching the selected
  publications, downloaded from the IEC code-components site with their licence read first and
  recorded in `CONTEXT.md`; checksums in a manifest.
- Done when: the licence decision is recorded (what may be committed in the public repository, if
  anything) and the files parse.

**P6 — MMS modules with the 8-1 extensions**
- Read: 8-1 Annex F; the `variante_if` modules of 9506.
- Produce: a script applying the four Annex F extensions (utc-time, MMSString in Data, maxIdentifier
  64, `pdu-size`) to a copy of the `variante_if` modules, with a diff report.
- Done when: the modules parse and the diff contains exactly the four extensions.

**P7 — TISSUE selection**
- Produce: `tissues.yaml` from the public TISSUE database for 7-2, 7-3, 7-4 and 8-1 edition 2.x:
  status, batch, affected clause, and the project's decision (applied, not applicable, open).
- Done when: every TISSUE in a published interoperability batch for these parts has a decision.

### Phase 0 — Foundations

**F1 — Workspace skeleton**
- Depends: none.
- Produce: root `Cargo.toml`, `rust-toolchain.toml` (same MSRV as `igs-iso9506-mms`), the three
  crates, workspace lints, `deny.toml`, SPDX headers, `AGENTS.md`, `PROVENANCE.md` with D-001
  (crate split), `docs/spec` ignored symlink, `.gitignore` for `.claude/`.
- Done when: the standard gate passes on the empty crates.

**F2 — Continuous integration and ai-log**
- Depends: F1.
- Produce: `.github/workflows/ci.yml` as in `igs-iso9506-mms` (no `docs/spec/` needed),
  `docs/ai-log/README.md`.
- Done when: the gate passes locally.

**F3 — Catalogue generation**
- Depends: F1, P5.
- Produce: `cargo xtask catalogue` reading the NSD files and writing
  `crates/igs-iec61850-mms-core/src/generated/`: CDCs, constructed attributes, LN classes, FCs,
  enumerations, presence conditions, with the namespace name and version. Generated files carry a
  "do not edit" banner citing their sources; `--check` fails when stale. The NSD files never enter
  this repository. Under the IEC Code Components EULA (decision of P5), every generated file keeps
  the IEC copyright notice, the licence conditions, the disclaimer and the attribution to its part
  and namespace release, and a `NOTICE` file repeats them.
- Done when: counts of classes and enumerations match the NSD files; a test checks a sample.

**F4 — Interop bench skeleton**
- Depends: F1.
- Produce: `bench/` reusing the approach of `igs-iso9506-mms`: libiec61850 at a fixed tag, built in
  a container, its example IEC 61850 servers and clients run as black boxes; capture and tshark
  helpers. Licence and version in `PROVENANCE.md`. Record whether its documented configuration
  offers an Edition 1 server mode (for E3).
- Done when: one scenario connects the MMS client of `igs-iso9506-mms` to an example IEC 61850
  server under the IEC 61850 profile and the capture decodes without error.

### Phase N — MMS extensions (in `igs-iso9506-mms`, under its rules)

**N1 — Generator input and codec union**
- Depends: P6.
- Produce: the codec generated from the union: identifiers up to 64, `utc-time`, the UTF-8 MMSString
  alternative, `pdu-size`. A PROVENANCE decision there records the union (MMS-001).
- Done when: codegen check passes; TASE.2 tests are green; generated tests cover the new variants.

**N2 — IEC 61850 MMS profile**
- Depends: N1, P3.
- Produce: a link profile: identifier limit 64 (32 for TASE.2), nesting at least 6, parameter CBBs
  and services from `conformance_81.yaml` by declared support, version number 1; the profile check
  enforces names, string forms and CBBs per profile. Revisit the lenient-CBB option (G8) against
  Table 128 (MMS-006).
- Done when: AT-02 and AT-04: each profile accepts its longest names and refuses longer ones; the
  bench scenario of F4 passes without the lenient option, or the remaining discrepancy is recorded.

**N3 — Typed UtcTime, MMSString and TypeDescription**
- Depends: N1.
- Produce: typed values with length and UTF-8 checks, the `utc-time` type description, the
  VMD/server paths that carry them (MMS-002, MMS-003, MMS-012, MMS-013 client side).
- Done when: AT-03 vectors built independently from the standard's octet layout pass.

**N4 — Alternate access towards the model**
- Depends: N2.
- Produce: Read and Write with alternate access passed to `Model::dispatch` instead of being refused
  by the generic layer; the generic VMD keeps refusing it (MMS-007).
- Done when: unit tests of the dispatch path; generic VMD behaviour unchanged.

**N5 — Observable outbound delivery**
- Depends: N2.
- Produce: per-association bounded queues replacing the shared broadcast; admission, refusal,
  saturation, loss and termination outcomes with a local message identifier (MMS-009 to 011).
- Done when: AT-06: forced saturation is observable, order is kept per association, no cross-client
  delivery, existing targeted-report test still green. Release 1.2.0 prepared locally (no push).

### Phase A — Model and data access

**A1 — Object references and MMS names**
- Depends: F1, P1.
- Read: 7-2 ObjectReference, 8-1 §7.3, §8.1.3.2.
- Produce: parsing and building of object references, FC placement, MMS domain and item names, the
  control-block address forms (`@`, `domain/item`, `/item`), with the three limits of MMS-001.
- Done when: property tests round-trip references and names; boundary tests at 64 and 129.

**A2 — ACSI types and their MMS mapping**
- Depends: A1, N3, P3.
- Read: 8-1 §8.1, 7-2 basic types, 7-3 Quality and TimeStamp.
- Produce: ACSI basic and common types to MMS Data and TypeDescription, bit-string rules, enumeration
  rules (negative values private, out-of-range values tolerated), Quality, TimeStamp with
  TimeQuality.
- Done when: vectors per row of the type table; malformed values give typed errors.

**A3 — Server model**
- Depends: A2, F3.
- Produce: LD/LN/DO/DA tree with FC views built from the catalogue, MMS type descriptions
  synthesised per LN and FC, presence checks (MODEL-004), the `Model` implementation for
  `igs-iso9506-mms`.
- Done when: a model built in code gives the expected GetNameList and GetVariableAccessAttributes
  answers.

**A4 — Server data access**
- Depends: A3, N4.
- Read: 8-1 §9 to §13.
- Produce: directory and definition services through GetNameList and GetVariableAccessAttributes,
  GetDataValues and SetDataValues through Read and Write, per-item errors (ACSI-002), the stateful
  write guard (MODEL-005).
- Done when: AT-07 against the in-process client of A6.

**A5 — Application event path and clock source**
- Depends: A3.
- Produce: a non-blocking handle to update values, quality and timestamps from application tasks,
  with the snapshot rule of MODEL-006; the clock-source trait of TIME-001 and a host-clock
  implementation (TIME-002).
- Done when: updates from another task are seen consistently by reads; AT-17 timestamp cases pass.

**A6 — Client discovery and data access**
- Depends: A2.
- Produce: typed client API: server, LD, LN and data directories with `continueAfter` paging, data
  definitions rebuilt from type descriptions, reads and writes with per-item results; unknown classes
  kept as generic data (MODEL-002).
- Done when: AT-07 client side against the server of A4.

**A7 — Data-access interop**
- Depends: A4, A6, F4.
- Produce: bench matrix: our client against the example servers, our server against the example
  clients, captures checked in tshark.
- Done when: every row passes or is recorded as a known gap.

### Phase S — SCL import

**S1 — Parser and bounds**
- Depends: F1, P5.
- Produce: ICD/CID parsing (feature `scl`) with external entities disabled, no network access, and
  bounds on size, depth and instance count (SCL-004).
- Done when: malicious XML cases are refused; fuzz target added.

**S2 — Semantic checks and templates**
- Depends: S1, F3.
- Produce: data-type templates resolved and instantiated, references, namespaces, dataset members and
  service declarations checked; diagnostics with file, path and line (SCL-002, SCL-003).
- Done when: AT-12 invalid cases each give their diagnostic.

**S3 — Model and communication parameters from SCL**
- Depends: S2, A3.
- Read: 8-1 §25, 61850-6.
- Produce: an atomic build of the server model, its datasets, its control blocks and its address
  parameters; declared services checked against what the server can execute (SCOPE-002).
- Done when: a sample CID builds a server that A7's clients discover.

### Phase D — Datasets and unbuffered reporting

**D1 — Static datasets**
- Depends: A4, S3.
- Read: 8-1 §14.
- Produce: configured datasets as named variable lists, GetDataSetValues, SetDataSetValues,
  GetDataSetDirectory, stable member order (DS-002).
- Done when: AT-08 static cases. **M1 is reached** when A1–A7, S1–S3, D1 and the M0 tasks are done.

**D2 — Dynamic datasets**
- Depends: D1.
- Produce: CreateDataSet and DeleteDataSet, persistent and association-specific, cleanup on release,
  refusal while referenced (DS-001, DS-002).
- Done when: AT-08 dynamic cases with two associations.

**D3 — URCB state machine**
- Depends: D1.
- Read: 8-1 §17.1.3, 7-2 reporting model and Annex E.
- Produce: sans-I/O URCB: reservation, configuration writes, enable, triggers, buffer time,
  integrity period, general interrogation (RPT-001).
- Done when: deterministic tests with controlled time for each trigger.

**D4 — Report encoding and delivery**
- Depends: D3, N5.
- Read: 8-1 §17.2.1.
- Produce: report PDUs with optional fields, inclusion bit string and reasons, sent through the
  observable delivery of N5 to the owning association only (RPT-002, RPT-003).
- Done when: vectors built from the standard's layout; saturation produces a diagnostic.

**D5 — Client reporting**
- Depends: D4, A6.
- Produce: client API to configure and enable URCBs and to receive decoded reports.
- Done when: AT-09 between our client and server.

**D6 — Reporting interop**
- Depends: D5, F4.
- Produce: bench rows for reports in both directions.
- Done when: AT-09 with the peer. **M2 is reached** with N5 and D2–D6.

### Phase B — Buffered reporting

**B1 — BRCB state machine and history**
- Depends: D3.
- Read: 8-1 §17.1.2, 7-2 buffered reporting.
- Produce: bounded event history, EntryID, purge, overflow, resynchronisation after association loss
  in the same process (BRCB-001 to 003).
- Done when: deterministic tests of disconnect, reconnect, resume and overflow.

**B2 — BRCB delivery, segmentation and client side**
- Depends: B1, D4, D5.
- Produce: segmentation where a report exceeds the PDU size, history kept until the standard's entry
  lifecycle allows deletion, client API for BRCBs.
- Done when: AT-10 between our client and server, and with the peer.

### Phase K — Controls

**K1 — Control state machines**
- Depends: A4.
- Read: 7-2 control model, 8-1 §20.
- Produce: sans-I/O direct and select-before-operate, normal and enhanced security, with selection,
  timeout, ownership, origin, ctlNum, test and check (CTL-001, CTL-002).
- Done when: deterministic tests of every transition and refusal.

**K2 — MMS mapping of controls**
- Depends: K1, D4.
- Produce: Select, SelectWithValue, Cancel and Operate through writes, LastApplError and AddCause
  (§20.11), CommandTermination (§20.9). TimeActivatedOperate declared unsupported.
- Done when: vectors and in-process tests per control model.

**K3 — Deferred outcomes and client control API**
- Depends: K2, A5.
- Produce: non-blocking application callbacks with deferred results (CTL-003); typed client API for
  the four control models.
- Done when: AT-11 between our client and server, and with the peer (CTL-004).

### Phase E — Edition 1 client profile

**E1 — Inventory of Edition 1 differences**
- Depends: P1, P2, P7.
- Read: 7-1 Annex K, 7-2 Annex F, 8-1 Annex K, the 7-2 notes on Edition 1, the TISSUEs of P7.
- Produce: a list of the differences a client meets (names, control-block attributes, reporting,
  controls), each as a PROVENANCE decision with its confidence (ED1-002, ED1-003).
- Done when: every difference has a source and a planned test.

**E2 — Detection and profile**
- Depends: E1, A6, D5, K3.
- Produce: edition detection from `LLN0.NamPlt.ldNs`, the per-connection profile, and its effect on
  names, control blocks and controls (ED1-001, ED1-004).
- Done when: unit tests; no check relaxed for an Edition 2.1 server.

**E3 — Edition 1 interop**
- Depends: E2, F4.
- Produce: bench rows against an Edition 1 peer if F4 found one; otherwise captures provided by the
  user (section 5). Untested behaviours are declared "untested".
- Done when: AT-15 for what can be tested; the rest recorded.

### Phase G — Gateway building blocks

**G1 — One runtime, clients and server**
- Depends: A5, A6.
- Produce: composition helpers and an example running two downstream clients and one upstream server
  (GW-001).
- Done when: the example builds and runs in an integration test.

**G2 — Proxy model, quality and controls**
- Depends: G1, D5, K3, S3.
- Read: 7-1 §8.2.3, 7-4 LPHD, 7-2 Annex D.
- Produce: upstream proxy LDs built from discovery or SCL with `LPHD.Proxy`, quality change on loss
  of a downstream association, control forwarding with deferred outcomes, upstream reports from the
  gateway's own control blocks (GW-002 to 005).
- Done when: AT-16 with two example servers downstream and an example client upstream.

### Phase O — Optional groups (selected after M3, each with its own service matrix)

- **O1 — File transfer**: MMS file services in phase N style, then GetFile, SetFile, DeleteFile,
  GetFileAttributeValues (8-1 §23). First optional group to select.
- **O2 — Logs**: log control blocks and readJournal (8-1 §17.3).
- **O3 — Setting groups**: SGCB and its services (8-1 §16).
- **O4 — Secured profile**: TLS of `igs-iso9506-mms` under IEC 62351-3; 62351-4 and 62351-6 are not
  held (SEC-001, SEC-002).
- **O5 — Embedded SNTP client** (TIME-002, TIME-003).

### Phase V — Verification, documentation, release

**V1 — Capability statements**
- Depends: the feature tasks of M3.
- Produce: one machine-readable capability source; `cargo xtask statements` writing the ACSI
  conformance statement, the PICS, a MICS and a PIXIT draft, and `docs/capabilities.md`
  (SCOPE-001 to 003).
- Done when: AT-14: every declared row names an existing test.

**V2 — Documentation, examples, binaries**
- Produce: rustdoc on every public item, examples for client, server and gateway, a command line
  client, `CHANGELOG.md`.
- Done when: `cargo doc --no-deps` has no warnings; examples build in CI.

**V3 — Robustness and performance evidence**
- Produce: fuzz targets for the core decoders and SCL; the measurements of SPECIFICATIONS §12.4
  with their workload and hardware.
- Done when: fuzzing runs clean for the recorded duration; results recorded, no target invented.

**R1 — Legal review and release** ⛔
- Depends: all previous tasks of M3.
- Gate: legal review of publishing code derived from the standards and code components (including
  whether code generated from the NSD files may be published under Apache-2.0: the IEC Code
  Components EULA is not OSI-approved and is granted to the purchaser of the standard), similarity
  check against libiec61850, crate names, git push and tag, for this repository and the phase N
  release of `igs-iso9506-mms`.
- Done when: a maintainer approves and publishes.

---

## 5. Decisions that stay with the user

The agent does not wait for these; it records the impact and continues with what it can do.

| Decision | Impact while open |
| --- | --- |
| Buy 7-2:2003 and 8-1:2004 (Edition 1) | E1 decisions keep a medium or low confidence |
| Buy 61850-6 AMD1/AMD2 and 61850-10 AMD1 | SCL features of later revisions unsupported; statements cite edition 2.0 of part 10 |
| Provide captures or access to RTE Edition 1 field devices | E3 limited to the bench peer |
| Fill in the reference of the legal authorization in `CONTEXT.md` §10 | None for the work; needed for R1 |
| Legal review, push and publication (R1) | Nothing leaves the machine |

---

## 6. Progress

| Task | Status |
| --- | --- |
| P1 Prepared texts of 8-1 and 7-2 | [ ] |
| P2 Prepared texts of 7-1 and 61850-6 | [ ] |
| P3 8-1 conformance and mapping data | [x] |
| P4 ACSI conformance data | [x] |
| P5 IEC code components | [x] |
| P6 MMS modules with the 8-1 extensions | [x] |
| P7 TISSUE selection | [x] |
| F1 Workspace skeleton | [x] |
| F2 Continuous integration and ai-log | [x] |
| F3 Catalogue generation | [x] |
| F4 Interop bench skeleton | [x] |
| N1 Generator input and codec union | [x] |
| N2 IEC 61850 MMS profile | [ ] |
| N3 Typed UtcTime, MMSString and TypeDescription | [ ] |
| N4 Alternate access towards the model | [ ] |
| N5 Observable outbound delivery | [ ] |
| A1 Object references and MMS names | [ ] |
| A2 ACSI types and their MMS mapping | [ ] |
| A3 Server model | [ ] |
| A4 Server data access | [ ] |
| A5 Application event path and clock source | [ ] |
| A6 Client discovery and data access | [ ] |
| A7 Data-access interop | [ ] |
| S1 Parser and bounds | [ ] |
| S2 Semantic checks and templates | [ ] |
| S3 Model and communication parameters from SCL | [ ] |
| D1 Static datasets | [ ] |
| D2 Dynamic datasets | [ ] |
| D3 URCB state machine | [ ] |
| D4 Report encoding and delivery | [ ] |
| D5 Client reporting | [ ] |
| D6 Reporting interop | [ ] |
| B1 BRCB state machine and history | [ ] |
| B2 BRCB delivery, segmentation and client side | [ ] |
| K1 Control state machines | [ ] |
| K2 MMS mapping of controls | [ ] |
| K3 Deferred outcomes and client control API | [ ] |
| E1 Inventory of Edition 1 differences | [ ] |
| E2 Detection and profile | [ ] |
| E3 Edition 1 interop | [ ] |
| G1 One runtime, clients and server | [ ] |
| G2 Proxy model, quality and controls | [ ] |
| V1 Capability statements | [ ] |
| V2 Documentation, examples, binaries | [ ] |
| V3 Robustness and performance evidence | [ ] |
| R1 Legal review and release ⛔ | [ ] |

Suggested order: P1, P3, P6, P5, F1, F2, F4, N1–N4 (M0), P2, P4, F3, A1–A7, S1–S3, D1 (M1), N5,
D2–D6 (M2), P7, B1, B2, K1–K3, E1–E3, G1, G2, V1–V3 (M3), R1. Fuzz targets are added as each
decoder lands.

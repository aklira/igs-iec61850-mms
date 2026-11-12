# Provenance

This file links every non-trivial behaviour of `igs-iec61850-mms` to its source: a clause of a standard,
a TISSUE, an observation test, or third-party code under a permissive licence. The project is a
clean-room implementation: no protocol behaviour without a source recorded here or in a
`// IEC 61850-8-1 §8.1.3.2` comment.

The IEC and ISO standards are licensed and are not distributed with this repository. The references
below point to clauses; they do not reproduce the text of the standards. The IEC code components (NSD
and SCL schema files) are not in this repository either.

## Reference standards

| Standard | Edition | Use |
| --- | --- | --- |
| IEC 61850-7-1 | 2011 + AMD1:2020 (edition 2.1) | Modelling principles, namespaces, revision compatibility |
| IEC 61850-7-2 | 2010 + AMD1:2020 (edition 2.1) | ACSI services and state machines, ACSI conformance statement |
| IEC 61850-7-3 | 2010 + AMD1:2020 (edition 2.1) | Common data classes |
| IEC 61850-7-4 | 2010 + AMD1:2020 (edition 2.1) | Logical-node classes |
| IEC 61850-8-1 | 2011 + AMD1:2020 (edition 2.1) | Mapping to MMS, MMS type extensions (Annex F), PICS |
| IEC 61850-6 | 2009 (edition 2.0) | SCL |
| IEC 61850-10 | 2012 (edition 2.0) | Conformance testing |
| IEC code components | NSD 7-2, 7-3, 7-4 2007B6 (light); SNSD 8-1 2003A4; SCL 2007B and 2007B4 | Catalogue and SCL schema, under the IEC Code Components EULA |
| IEC 61850 TISSUE database | Batches 1 to 3 (closed 2021-11-11, 2024-04-01, 2026-09-01) | Corrections applied, with a decision per TISSUE |
| ISO 9506-1, ISO 9506-2 | 2003 | MMS, through `igs-iso9506-mms` |
| RFC 5905 | | SNTP subset, time synchronisation profile |

## Decisions on ambiguities in the standard

Confidence: **high** when the sources agree; **medium** when they contradict each other or are
silent, to be confirmed by an observation test against other implementations.

### D-001 — Two library crates: a sans-I/O core and the async API

- **Decision**: the workspace has `igs-iec61850-mms-core` (object references and MMS names, ACSI types
  and their MMS mapping, model and catalogue, datasets, report, control, log and setting-group state
  machines, SCL import behind a feature), `igs-iec61850-mms` (async client, server, gateway helpers and
  clock source on the MMS crates of `igs-iso9506-mms`), and `xtask` (not published).
- **Why**: SPECIFICATIONS §5 allows one crate at first and a split when it is justified. The service
  state machines must be deterministic and testable with a controlled clock (RUN-004); a crate without
  tokio enforces it, as the link crate does in `igs-iso9506-mms`. The core can also serve another
  runtime or a test harness without the network layers.
- **Sources**: SPECIFICATIONS.md §5 and RUN-004; the architecture of `igs-iso9506-mms` (IMPLEMENTATION.md
  §2 of that repository).
- **Confidence**: high (a structural choice, no protocol behaviour).
- **Test**: the standard gate on the empty crates (task F1).

### D-002 — The catalogue is generated from the latest IEC code components, with the NSD defaults

- **Decision**: `cargo xtask catalogue` reads the NSD files of 7-2, 7-3 and 7-4 at release 2007B6 and
  the SNSD and AppNS of 8-1 at release 2003A4, and writes one data module per namespace under
  `crates/igs-iec61850-mms-core/src/catalogue/generated/`: basic types, functional constraints, presence
  conditions, enumerations, constructed attributes, CDCs, logical-node classes, and the 8-1 service
  additions. Order is the order of the files. The reading is strict (an unknown element or value stops
  the generation) and every count is checked against an independent count of the XML elements.
  Attributes absent from a file take the default of the schema delivered with it (`NSD.xsd`):
  `typeKind` BASIC, `presCond` and `dsPresCond` M, booleans false. Descriptions are not read (the light
  code components carry none).
- **Why the latest release**: each part asks the user to take the code component with the highest
  release, which supersedes the published text. Releases B4 to B6 (2022 to 2025) and A3, A4 carry the
  corrections of the TISSUE batches: all ten TISSUE decided "catalogue" in the project's selection are
  in them, so no correction is applied on top (`norm_preparation`, tissues decisions).
- **Licence**: the files fall under the IEC Code Components EULA. Each generated file keeps the IEC
  copyright notice of its code component and the attribution note, and carries the SPDX identifier
  `IEC-Code-Components-EULA`; `NOTICE` gives the conditions and disclaimer. The NSD files themselves
  are not in this repository. Publishing these files under the Apache-2.0 crate is part of the legal
  review of task R1.
- **Sources**: IEC 61850-7-2, 7-3, 7-4 §1.3 and 8-1 §1.3 (code component distribution); IEC TS 61850-7-7
  (NSD format, through `NSD.xsd`); TISSUE batches 1 to 3.
- **Confidence**: high for the content (strict reading, counts checked, sample checked against the
  standards in `tests/catalogue.rs`); medium for the defaults until the model tasks use them.
- **Test**: `crates/igs-iec61850-mms-core/tests/catalogue.rs`; `cargo xtask catalogue --check`.

## Third-party code and sources

| Component | Licence | Use | Notes |
| --- | --- | --- | --- |
| `igs-iso9506-mms` crates | Apache-2.0 | MMS stack | Same maintainers; added with the first task that needs it |
| roxmltree 0.21 | MIT OR Apache-2.0 | XML reading in `xtask` (task F3) | No dependency; DTDs refused |
| IEC code components (NSD 2007B6, SNSD 2003A4) | IEC Code Components EULA | Generated catalogue (task F3) | See NOTICE and D-002; files not in this repository |
| libiec61850 | GPL-3.0 | Reference peer of the bench, tag `v1.6.2.1`, commit `c28c27c`, black box only; image built locally, not distributed | Source never read; user documentation only |
| Wireshark (tshark) in `nicolaka/netshoot` | GPL-2.0 | Independent decoder of the bench captures | Run as a tool, not linked |

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

### D-003 — Names and object references: three limits, three forms (task A1)

- **Decision**: `names` holds the names and their MMS addresses; `reference` holds the object references and their
  three forms.
  - **Names**: an IEC 61850 name (`Name`) is an ObjectName of 1 to 64 characters, starting with a letter, then
    letters, digits and underscores. An MMS identifier (`MmsIdentifier`) is 1 to 64 characters of letters, digits,
    `$` and `_`, starting with a letter. A scope-qualified MMS name (`MmsName`) is `domain/item`, `/item` or `@item`,
    and a reference is at most 129 octets. The three limits are kept apart, as MMS-001 asks; the string values of the
    application have their own limit, in the data model (A2).
  - **Forms**: the ACSI reference `LD/LN.DO.DA(n).SubDA` has no functional constraint; the MMS item
    `LN$FC$DO$DA` in the domain `LD` has it after the logical node; the data reference of a report or a log
    `LD/LN$FC$DO$DA(n)` keeps the array elements. `ObjectReference::to_mms` refuses an array element: MMS reaches it by
    an alternate access. `from_mms` and `parse_data_ref` read the two other forms back.
  - **Control blocks**: a control block is the object `LN.CBName` of its logical node; its MMS address is
    `LD/LN$FC$CBName`, with the constraint of the block (`RP`, `BR`, and so on).
  - **Functional constraints**: the two-letter codes of 7-2 and of the figure of 8-1 (`ST` to `RS`), with `XX`
    refused: it is for services only, and 8-1 does not map it. The list is checked against the generated catalogue.
- **Sources**: IEC 61850-7-2 §6.2.3.3 (ObjectName), §6.2.3.4 (ObjectReference, 129 octets); IEC 61850-8-1 §7.3.1 to
  §7.3.4 (the scopes, the flattened names, the alternate access for arrays), §8.1.3.2.2 to §8.1.3.2.4 (the forms of
  the references), Annex F.3.2 (identifiers of 64), §24.2.2.2.1 (BasicIdentifier), Figure 4 and its NOTE 2
  (constraints).
- **Confidence**: high for the examples of §7.3.3, §7.3.4 and §8.1.3.2.4, which are tests; high for the limits;
  medium for the reading of the list of constraints of 8-1 Figure 4 as the names an ACSI reference may take, since the
  catalogue (NSD) lists only the 7-2 constraints and CO.
- **Test**: unit tests in `names.rs` and `reference.rs` (limits, syntax, the examples, the scopes); property tests in
  `tests/references.rs` (round trips of references, items and data references, the MMS names, the refusals of a
  name longer than 64 or with a foreign character); a cross-check of the constraints with the catalogue.
- **Licence**: `proptest` (MIT or Apache-2.0) is a development dependency of `igs-iec61850-mms-core` only; it is not
  published with the library.

### D-004 — ACSI basic and common types and their MMS mapping (task A2)

- **Decision**: the module `acsi` maps each row of the basic types (8-1 Table 15) and the common types (quality,
  check conditions, originator, timestamp) to MMS values and type descriptions, and reads them back with a typed
  `AcsiError` for each refusal. Reading never changes a value silently.
  - **Enumerations**: a standardised value, an extension (non-negative, not standardised, in the integer range) and a
    private value (negative) are three readings, and only the first is named. An extension is not an error
    (§8.1.2.2). The type description is the signed integer that holds the largest standardised value: its bits plus
    one for the sign, at least one bit (`describe_enumeration`).
  - **Coded enumerations**: the size is the number of bits of the largest standardised value (`coded_enum_size`),
    never zero (§8.1.2.1). The value is written most significant bit first (bit 0 of the bit string is the MSB).
  - **Bit strings, reading**: the bits past the size of the type are ignored and the missing ones take zero, as the
    first point of §8.1.2.1 says. **Writing**: the exact size is written, never a longer string, and the padding bits
    are zero.
  - **Strings**: a visible string is in the ISO 646 set and has at most the limit in characters (§8.1.2.5); a Unicode
    string has at most the limit in characters, not octets (§8.1.2.6, RFC 3629 serialization); an octet string has
    at most the limit in octets (§8.1.2.4). These are refused when longer, unlike the bit strings above.
  - **FLOAT32**: only the single precision form (format width 32, exponent width 8) is accepted; another form is
    another type.
  - **Quality**: 13 bits in the order of Table 44 (validity in bits 0 and 1 as a coded value, detail bits 2 to 9,
    source bit 10, test bit 11, operator blocked bit 12). **CheckConditions**: 2 bits of Table 43. **Originator**: a
    structure of `orCat` (the enumeration `OriginatorCategoryKind`) and `orIdent` (at most 64 octets), in the order of
    the catalogue (§8.1.3.17 only says "structure").
  - **Timestamp**: an UtcTime of eight octets, with the time quality of N3; an invalid quality is refused (§8.1.3.7).
- **Conflict in the standard**: the normative point of §8.1.2.1 ignores the extra bits of a longer bit string, but
  the NOTE of the same clause says that writing a longer bit string is `object-value-invalid`. The module follows the
  normative point when reading and never sends a longer string, so the NOTE is met on the sending side. Strings are
  refused when longer, which is the NOTE's rule for them.
- **Sources**: IEC 61850-8-1 Table 15, §8.1.2.1 to §8.1.2.6, §8.1.3.7, §8.1.3.17, §8.2 and Tables 43 and 44,
  Annex F.3.1 (UtcTime); IEC 61850-7-2 for the enumerations and the Originator components.
- **Confidence**: high for the string limits, the Quality and CheckConditions positions (each bit has a vector test),
  and the Timestamp mapping; medium for the enumeration size, since it is read literally as the minimum signed
  integer; medium for the reading of the bit strings, because of the conflict above. Each medium point can be changed
  in one function, and the vectors show the effect.
- **Test**: 20 unit tests in `acsi.rs`: a vector per row of Table 15, the bit positions of Tables 43 and 44, the
  coded enumeration bit order, the malformed values (zero size, bit strings of no bit, out-of-range integers, bad
  characters, too long strings, wrong MMS type, invalid time quality, short floats).
- **Not decided here**: the data model that uses these types (A3, A4); the conformance of the enumerations to the
  catalogue values beyond the classification.

### D-005 — Order of the members of a logical node (task A3)

- **Decision**: the data objects of a logical node are in the order the application gives them (the SCL order);
  the attributes of a data object are in the order of its common data class (7-3); the sub data objects come after
  the attributes of their class, each in its class order; the components of a constructed attribute are all
  included, in the order of its definition, whatever their presence.
- **Sources**: IEC 61850-8-1 §7.3.1 (the order of the components of a functional constraint follows the SCL data
  objects, and the order of the attributes of a common data class follows 7-3), and its NOTE 3 on compatibility.
- **Confidence**: medium. The catalogue does not keep the position of the sub data objects among the attributes of a
  class, so they come last; a class that places one between attributes would need its position from the namespace.
- **Test**: `tests/model.rs` (the order of the functional constraints, the order of the attributes of a data object).

### D-006 — Flattened names and the identifier limit (task A3)

- **Decision**: a logical node is one named variable, whose type has one component per functional constraint, in the
  order of Figure 4 of 8-1 (MX, ST, CO, CF, DC, SP, SG, RP, LG, BR, GO, GS, SV, SE, MS, US, EX, SR, OR, BL, RG, RS),
  then one component per data object of that constraint, then the attributes and the sub data objects. Each node of
  this tree is also a flattened named variable `LN$FC$DO$DA$...`, from the data object down. The flattened names of
  the functional constraint alone (`LN$FC`) are not variables: the examples of 8-1 §7.3.3 start at the data object.
- **The limit**: a flattened name longer than the identifier limit of the profile (64 characters under the IEC 61850
  profile, Annex F.3.2) is not listed. Its number is reported (`LnLayout::long_names`), and it stays reachable by
  alternate access to the node variable, as 8-1 §7.3.3 and §7.3.4 allow. The MMS VMD accepts 64 characters only when
  its identifier limit is raised by profile (task N6 of igs-iso9506-mms, recorded there).
- **Sources**: IEC 61850-8-1 §7.3.1, §7.3.3, §7.3.4; Annex F.3.2 for the 64 characters.
- **Confidence**: high for the tree and the flattened names, which follow the examples of §7.3.3 and §7.3.4; medium
  for the omission of the functional-constraint level, which the text does not list either way.

### D-007 — Presence checks of the model (task A3)

- **Decision**: a mandatory member is the one whose presence is `M` (or `Mmulti`, which needs one instance); it is
  always instantiated, and a logical node without one is refused. Optional and conditional members are instantiated
  only when the application names them. A multiple data object (`Omulti`, `Mmulti`, the ranges) is named by its
  definition and a number (`Ind1`, `Ind2`). A name that the class or the common data class does not define is
  refused. The other presence conditions (`MOcond`, `MFsubst`, `AtLeastOne`, the groups, and so on) are not evaluated:
  they are instantiated when named, and their evaluation waits for the data they depend on (`docs/capabilities.md`).
- **Sources**: IEC 61850-7-3 (presence conditions, the attributes of the common data classes), IEC 61850-7-4
  (the data objects of the classes and their multiplicity), MODEL-004.
- **Confidence**: high for `M` and the names; medium for the reading of `Mmulti` as one instance at least, because the
  catalogue keeps the condition name only.

### D-008 — Types of the attributes in the model (task A3)

- **Decision**: each type of an attribute is resolved from the catalogue to its MMS type description: the basic types
  of 8-1 Table 15 and §8.1.3 with a fixed form (`BOOLEAN`, the integers, `FLOAT32`, the visible, octet and Unicode
  strings with their lengths, `ObjRef` and `Currency`, `Timestamp`, `Quality`, `Check`), the enumerations of the
  catalogue (an integer that holds the largest standardised value), and the constructed attributes (structures).
  An attribute whose type name the class leaves empty takes the type name of its data object (its `underlying_type`
  in the catalogue, for example the enumeration `BehaviourModeKind` of the status `Beh`), with its own kind.
- **Not mapped yet** (refused with `UnmappedType` or `Unsupported`, never guessed): `Dbpos`, `Tcmd`, `TrgOps`,
  `OptFlds`, `SvOptFlds`, `EntryID`, `EntryTime`, `PhyComAddr` and the physical addresses of the Scsm types; the
  enumerations chosen by the instance; the arrays; the attributes of the service-tracking class of 8-1 (`Undefined`);
  and the classes with variants (the setting groups), whose variant needs a choice that the model does not make yet.
  Each one is a known gap in `docs/capabilities.md`, to be closed by its own task (reporting, control, setting groups).
- **Sources**: IEC 61850-8-1 Table 15, §8.1.2.2, §8.1.2.3, §8.1.3.7, §8.1.3.17, §8.2; the catalogue of the
  namespaces (`underlying_type`).
- **Confidence**: high for the mapped types (their forms are in Table 15 and in the A2 vectors); medium for the
  enumeration of a status taken from `underlying_type`, which the catalogue gives without its kind.

### D-009 — The served model negotiates `valt` for its named components (task A3)

- **Decision**: the configuration of a server that serves the model (`igs_iec61850_mms::model::server_config`) negotiates
  `valt`, through `ServerConfig::with_alternate_access`. The client side offers it already. Alternate access itself is
  refused item by item, as the generic VMD refuses it (D-035 of igs-iso9506-mms), until the model side of the
  alternate access is served (task A4).
- **Why**: the MMS codec requires the CBB `valt` for every named component of a type description (`validate_cbb`,
  §8.1.3.15 and the directives, as read in igs-iso9506-mms D-013). A structure of a logical node has named components (the functional
  constraints, the data objects, the attributes), so without `valt` the server cannot send the type description of its
  nodes: its own check refuses the response, and the request is left without an answer.
- **Conflict**: IEC 61850-8-1 Table 128, as encoded in the profile (igs-iso9506-mms `profile_81.rs`), requires `valt` only of a
  server that declares arrays. The codec rule is stricter for the logical-node model. The two readings differ only for
  the names of components. Both are recorded: the codec keeps its reading (confidence medium), and the choice made here
  does not depend on it beyond this configuration. If the reading of Table 128 is right, the codec rule is to be
  limited to the ISO 9506 profile; that is a change to igs-iso9506-mms, decided with the maintainer.
- **Test**: `tests/served_model.rs` (a type description of a node is answered, with the names of its components).
- **Confidence**: medium.

### D-010 — The served variables are read-only in task A3 (task A3)

- **Decision**: the variables of a served node are read-only. A write is refused with `object-access-denied`. The values of
  a node are one structure, held once: its named variable and each flattened name are views of it by path, so that
  a read of a name and a read of the node always agree. Writes are opened by the write guard of task A4 (MODEL-005), with
  the rules of each functional constraint.
- **Why**: a write needs the guard that decides which attributes accept it, and that guard is the work of A4. A
  read-only model is the honest state of A3, and SCOPE-002 forbids advertising the writes before they run.
- **Confidence**: high for the single value (the test `a_node_and_its_flattened_name_show_the_same_value`); the
  read-only state is temporary and listed in `docs/capabilities.md`.

### D-011 — Unicode-3.0 joins the accepted licences (task A3)

- **Decision**: `Unicode-3.0` is accepted in `deny.toml`. It is the licence of `unicode-ident`, which the derive macros of
  the MMS stack bring in (through `proc-macro2`, `syn` and `quote`). The MMS repository accepts it for the same crate
  (its PROVENANCE, third-party table), so the two repositories check the same set.
- **Why**: the new path dependency on `igs-iso9506-mms` brings its dependency tree, which the list did not cover.
- **Confidence**: high; the licence is permissive and the crate is data tables only.

### D-012 — What an MMS Write may change in the served model (task A4)

- **Decision**: a variable of the served model accepts a Write when all of its parts accept it:
  - the functional constraint of the variable is one that the namespace lists with the service `SetDataValues` in
    the catalogue (`SP`, `SV`, `CF`, `DC`, `BL`); `SE` is written by its own services, and the others are read-only;
  - the variable does not belong to a controllable data object, that is, a data object whose class has the service
    attributes of 8-1 (`SPC`, `DPC`, `INC`, `ENC`, `BSC`, `ISC`, `APC`, `BAC`). Its writes go through the control state
    machine (MODEL-005), which does not exist yet (task K); until then they are refused;
  - the logical node as a whole is never written: its constraints differ.
  A Write to a read-only part refuses the whole variable (8-1 §8.1.3.4.4.2: a read-only sub-component makes the
  variable read-only).
- **Enumerations**: a value written to an enumeration must be one that the namespace defines (8-1 §8.1.3.4.4.2, and its
  NOTE for the positive and the negative ranges). A value the MMS integer holds, but the namespace does not define, is
  refused with `object-value-invalid`. A value outside the MMS integer is refused as `type-inconsistent` by the VMD,
  before the variable sees it.
- **Errors** (8-1 Table 27): `object-access-denied` for a read-only variable, `type-inconsistent` for a value of another
  type, `object-value-invalid` for an undefined enumeration value, `object-non-existent` for a path that is gone. The
  results are per item (ACSI-002): a Write of several items answers each one.
- **Sources**: IEC 61850-7-2 (the services of each functional constraint, as the catalogue of the namespace gives them); IEC 61850-8-1 §8.1.2.1
  (NOTE), §8.1.3.4.4.2 and Table 27, §13.4.2 and Table 54.
- **Confidence**: high for the constraints and the error codes; medium for the controllable classes as the set of
  objects whose writes wait for the state machine: the namespace gives the class, not the state rules.
- **Test**: `crates/igs-iec61850-mms-core/tests/access.rs` (constraints, controllable objects, enumerations, paths);
  `crates/igs-iec61850-mms/tests/served_writes.rs` (the results of each refusal and of each write, per item).

### D-013 — Directory and definition services, and the state of AT-07 (task A4)

- **Decision**: the directory and definition services of the ACSI are answered by the two MMS services that the model
  already serves. GetDataDirectory and GetDataDefinition are GetVariableAccessAttributes on the name of a data object
  (8-1 Tables 55 and 13.4.4): the components of the structure are the attribute names. The logical-node directory is
  GetNameList on the domain, with the names of the nodes, and the client keeps the names that have no `$` (8-1 Table 50).
  The server does not filter the list: it lists all the named variables, as the VMD does.
- **AT-07**: the done-when of A4 names the in-process client of A6, which is not built. The test uses the MMS client
  against the served model, which covers the services; AT-07 is complete with A6, and the directory filter of the client
  is then tested there.
- **Confidence**: high for the mapping of Tables 54 and 55; medium for the filter of the logical-node directory, which
  the client applies (Table 50 read at the level of its classes, not its service text).
- **Test**: `served_writes.rs` (`the_directory_of_a_data_object_lists_its_attributes`); `served_model.rs`
  (`get_variable_access_attributes_gives_the_type_of_each_level`, `get_name_list_lists_the_node_and_its_flattened_names`).

## Third-party code and sources

| Component | Licence | Use | Notes |
| --- | --- | --- | --- |
| `igs-iso9506-mms` crates | Apache-2.0 | MMS stack | Same maintainers; added with the first task that needs it |
| roxmltree 0.21 | MIT OR Apache-2.0 | XML reading in `xtask` (task F3) | No dependency; DTDs refused |
| IEC code components (NSD 2007B6, SNSD 2003A4) | IEC Code Components EULA | Generated catalogue (task F3) | See NOTICE and D-002; files not in this repository |
| libiec61850 | GPL-3.0 | Reference peer of the bench, tag `v1.6.2.1`, commit `c28c27c`, black box only; image built locally, not distributed | Source never read; user documentation only |
| Wireshark (tshark) in `nicolaka/netshoot` | GPL-2.0 | Independent decoder of the bench captures | Run as a tool, not linked |
| proptest 1 | MIT OR Apache-2.0 | Development dependency of the core crate: property tests of the names and references (task A1) | Not published with the library |

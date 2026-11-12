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
- **Correction (P1, CONTEXT §23)**: Table 50 of 8-1, read in the text, maps the class of `GetLogicalNodeDirectory`
  to `NamedVariable` and says nothing of a filter on the names that contain `$`. The filter of the logical nodes is
  therefore a choice of the client (task A6), not a rule of the standard. Its confidence is medium.


### D-014 — The clock sources: the quality is declared by the application or the operator (task A5)

- **Decision**: the timestamps come from a `ClockSource` (core `time`), which returns the UTC time with its quality. The
  host clock (async `clock`) reads the system time, and it reports the quality declared for the host: synchronised or
  not, leap-second knowledge, and the accuracy of the synchronisation. The default is *not synchronised* with no
  declared accuracy, so a host whose state was not declared stamps its values as such. A quality that the octet cannot
  carry (accuracy above 24 bits) is refused at the clock (`ClockError::InvalidAccuracy`), and the reading stays.
- **Why**: TIME-001 forbids stamping with a clock whose state is not reported. The crate cannot verify the synchronisation
  of a host without an unsafe interface or a platform service, so the declaration is the operator's, and it is recorded
  in the PIXIT of the device (TIME-002). A stamp that would be refused by the reader is refused at the source.
- **Not implemented**: an embedded SNTP client (TIME-002, optional when the host synchronisation is declared) and the
  derivation of the accuracy from the root dispersion (TIME-003, which only applies to SNTP). They are gaps in
  `docs/capabilities.md`. PTP is not implemented.
- **Confidence**: high for the bit layout of the quality (Annex F.3.1, tested per flag) and for the refusal of accuracy
  above 24 bits; medium for the declared-quality reading of TIME-002, which the standard leaves to the PIXIT.
- **Test**: `crates/igs-iec61850-mms-core/tests/time.rs` (each flag at its bit, round trips, the accuracy bound, the loss and
  the recovery of synchronisation); `crates/igs-iec61850-mms/tests/clock.rs` (the default, the declared quality, the time).

### D-015 — The application's updates: one node per update, all or nothing, with a revision (task A5)

- **Decision**: the application changes values, qualities and timestamps through an `Updater`, which is cloned and used
  from any task. An update holds the changes of one logical node; it checks every change (the name, the type, the defined
  enumeration values, one node), applies them to a copy of the node's value, and stores the copy under the node's lock.
  Each node has a revision that goes up by one with each change, from the application or from a client. The update
  returns the revision.
- **Snapshot rules (MODEL-006)**: a read of one item, and a read of a data object, are one state of the node; a value,
  its quality and its timestamp that one update changed are read together. A Read of several items takes each item at
  its own moment, and a consistent view of several nodes is the job of a data set or a report, which build on the
  revision. Reports and data sets are not implemented yet (tasks D and B).
- **Event ordering**: the revisions of one node are ordered by the lock; the revisions of different nodes are not ordered
  among themselves.
- **Non-blocking**: the lock covers a copy and a store of the node's value. No I/O and no wait for an association happens
  under it, so an application task does not wait for the network. An update does not queue: it applies at once or is
  refused.
- **Why**: a per-node lock gives the consistency that reads need (value, quality and time together) at the smallest cost,
  and the revision gives the order that the reports will need (RUN-004 and MODEL-006). A channel to a separate model task
  would delay each update to the next poll of that task, and the MMS server already reads the node under its lock.
- **Confidence**: high for the atomicity and the refusals (tests); medium for the statement that the lock is short under
  the report load, which is measured with the reporting tasks (S4 and V3 in the plan).
- **Test**: `crates/igs-iec61850-mms/tests/updates.rs` (one state for a value, quality and time; a refused update changes
  nothing and takes no revision; the types, the enumerations and the single node; a reader that never sees a value without
  the time of its update, with 300 updates running against 300 reads).

### D-016 — Discovery and the states of a definition, and the error of an item (task A6)

- **Decision**: the client rebuilds each definition from its MMS type description and keeps it in one of three states.
  *Known*: a leaf of a type of the mapping, or a structure or an array of known leaves. *Unknown semantics*: a type of MMS
  the client does not know (`Leaf::Unknown`), or a type the server names without sending it (`Leaf::Unresolved`, with the
  path); both are kept, so that the value can be read and written generically (MODEL-002). *Malformed*: a rule of the
  protocol or of the mapping is broken (an unnamed or a repeated component, a zero-size bit string or integer, a float
  whose exponent is not shorter than its format, a negative length); it is an error with its path. A description nested
  beyond `MAX_DEPTH` is refused, not followed.
  - The logical nodes of a device are the named variables with no `$`; the flattened names are not nodes (D-013).
  - A listing is read page by page with `continueAfter` (the last name of the page). A name that repeats over the pages
    is refused, and a page that is empty while more follow is refused as stalled, so that a listing neither counts a name
    twice nor goes on without end (ACSI-001). The order of the names is the order of the server, which MMS does not fix.
  - **Amendment (interop, task A7)**: the first version refused a page that was not sorted, which the reference server
    breaks: its names are in its own order, and a sorted check refused its listing (`GGIO1$CO$SPCSO1$Oper$T`). The check is
    now on repeats only (`Listing`, `PagingError::Repeated`), with a bound on the number of names (`MAX_LISTED_NAMES`).
  - **Amendment (interop, task A7)**: the reference server describes its bit strings and strings with a negative size
    (`BitString(-13)`, `VisibleString(-255)`). A negative size is a variable length, with the absolute value as its maximum
    (ISO 9506-2 §14.2.2.4.2, as the MMS conformance rule of igs-iso9506-mms reads it). It is not malformed: `Size { length,
    variable }`. A zero size of a bit string stays malformed (no zero-length bit string, IEC 61850-8-1 §8.1.2.1).
  - The result of a read or a write has one entry per item, in the order of the items (ACSI-002).
- **The error of an item** (8-1 Tables 26 and 27): `object-access-denied` is `AccessViolation`; `object-non-existent` is
  `InstanceNotAvailable`; `type-inconsistent` is `TypeConflict` (writes); `object-value-invalid` is
  `ParameterValueInconsistent` (writes); any other error is `FailedDueToCommunicationsConstraint`.
  - **Ambiguity**: Table 27 maps `temporarily-unavailable` to two ACSI errors, `instance-locked-by-other-client` and
    `access-not-allowed-in-current-state`. The client maps it to the second, which is the state of the server; the first
    would need the lock owner, which MMS does not say. Reads have one mapping in Table 26, and it is used.
- **Sources**: IEC 61850-8-1 §7.3.1 (the layout of a node), §8.1.3.4.4.1 and .2 with Tables 26 and 27, §13.4.2 to §13.4.4
  (Tables 54 and 55); ISO 9506-2 (GetNameList and its continuation).
- **Confidence**: high for the states, the paging checks and the mapping of the errors of Table 26; medium for the choice
  of `access-not-allowed-in-current-state` in Table 27, which the table leaves two ways.
- **Test**: `crates/igs-iec61850-mms-core/tests/discovery.rs` (the states, the malformed cases, the nesting, the groups, the
  logical nodes, the pages); `crates/igs-iec61850-mms/tests/client_discovery.rs` (AT-07, client side, against the served
  model: the listings, the definitions, the acceptance of a value, the read and write results per item).

### D-017 — The interop rows: protocol criteria, functional gaps documented (task A7)

- **Decision**: a row of the bench matrix passes when the protocol criteria hold: the association is accepted, the
  capture decodes cleanly (the known Wireshark artefact excepted), and, for the reverse rows, every request of the example
  is answered. The functional part of an example (the values it reads, the controls it operates, the reports it reads) is
  not a criterion of the row: where our model or our stack does not provide it, the gap is written in `docs/interop.md`,
  with the task that closes it. A gap is never a silent skip.
- **The script of the direct rows** takes its node and its status object from the discovery, so that it runs against any
  example. A status object is chosen among those that describe a `stVal`. A client never writes a status: the write is
  expected to be refused, and the row checks that it is.
- **Why**: the example servers and clients are configured for their own models; a row that demanded their values would
  test the fixtures, not the stack. The protocol criteria are the ones the standard fixes (8-1 Tables 26 and 27 and the
  request-response pairing), and they are checked on the wire.
- **Confidence**: high for the protocol criteria, since they are the ones the captures show; medium for the functional gaps'
  list, which is the state of the rows at the date of the run, and is re-run with each change of the bench.
- **Test**: `bench/tests/test_a7_data_access.py` (7 rows), `bench/tests/test_a7_reverse.py` (6 rows); the campaign of the
  last run is in the task log.

### D-018 — The SCL parser: a read-only XML tree, no document type, no entity resolution (task S1)

- **Decision**: the SCL parser (module `scl` of the core crate) is behind the feature `scl`, off by default. It reads the
  document with roxmltree 0.21 (MIT OR Apache-2.0, no dependency), the parser already held for the NSD files (`xtask`, F3).
  A document that carries a document type declaration (`<!DOCTYPE`) is refused before reading, and roxmltree is run with
  `allow_dtd: false`, so that external and internal entities are never expanded. The default entity resolver resolves
  nothing: no file and no network is opened.
- **Why**: SCL-004 forbids external entity resolution and network fetches. SCL uses no document type declaration (IEC
  61850-6 defines the schema, not a DTD), so refusing one loses nothing and removes the attack surface of entity expansion.
- **Confidence**: high. Tests: `tests/scl_parse.rs` (an external file entity, a network entity and an expansion bomb are
  refused with `DocTypeNotAllowed`); a fuzz target `fuzz/fuzz_targets/scl_parse.rs` ran for 90 s (about two million inputs)
  without a crash or a panic.
- **Sources**: SCL-004 and SCL-002 of `SPECIFICATIONS.md`; the parser's documentation (`ParsingOptions::allow_dtd`).

### D-019 — The bounds of the SCL parser (task S1; SCL-004)

- **Decision**: a parse checks each bound while it builds the tree: the size of the document (64 MiB by default), the
  nesting (64 levels by default, and never more than `DEPTH_CEILING` = 256, which bounds the recursion of the builder),
  the number of elements (1 000 000), the number of instances (250 000), the number of attributes of one element (128), and
  the length of an attribute value (4096 bytes). The tree reader is also given twice the element bound as its node limit.
- **The instances** are the elements that the model instantiates: IED, AccessPoint, Server, LDevice, LN0, LN, DOI, DAI,
  SDI, ConnectedAP, SubNetwork. This is the list the bound counts; it is not an SCL classification.
- **Refusals**: a document too large, not UTF-8, with a document type declaration, not well-formed, nested too deep, with
  too many elements or instances, an element with too many attributes, a value too long, or a root that is not `SCL`. Each
  is an error with its reason; no partial document is returned.
- **Not in S1**: the namespace of the root (checked in S2, with the XSD), the semantic checks (S2), and the atomic activation
  of a model built from the document (S3). The parser returns a tree; it builds nothing.
- **Confidence**: high for the refusals (tests); medium for the default values, which are chosen for a large substation
  description and are to be revisited with real files (in the RTE field captures and SCD examples when the user provides them).
- **Test**: `tests/scl_parse.rs` (15 tests: one per refusal, a valid document read with its structure, two property tests
  that check that arbitrary and mutated bytes never panic).

### D-020 — The control state machines: the four models of 7-2 §20, and the choices the figures leave open (task K1)

- **Decision**: the machine (core module `control`) implements the four control models of 7-2 §20.1 to §20.3, from the
  state diagrams of Figures 40 (direct, normal), 42 (select before operate, normal), 43 (direct, enhanced) and 44 (select
  before operate, enhanced), read on the PDF pages (7-2 pp. 153, 156, 159 and 161 of the printed numbering), and the
  refusals of Table 108. The machine is sans-I/O: it takes a request, a verdict of the application, a change of status, a
  timer or a loss of association, with the time from the start of the machine, and returns the actions (responses,
  checks, `Execute`, `Mirror`, `Terminate`, timers). The application's checks are asked for by an action and answered by
  an event (CTL-003).
- **The choices the figures leave open**:
  - a failed check (operative or dynamic), a timer, a cancel, a refused behaviour or a lost association **ends the
    selection** (the arrows of Figures 42 and 44 lead to `EndSelection`); a succeeded command ends it according to
    `sboClass`: `operate-once` releases, `operate-many` keeps the owner and rearms `sboTimeout`;
  - the positive response of an operation is sent **before** the output is activated (Figures 40 and 43);
  - `Beh` decides the output: `on` and `test` activate it; `test/blocked` mirrors the command (enhanced: CommandTermination
    negative, `Blocked-by-Mode`, as Figure 44 shows); `blocked` and `off` refuse the command with `Blocked-by-Mode` before any
    output. The figures show only `on`, `test` and `test/blocked`: the refusal of `blocked` and `off` is ours (confidence
    medium);
  - a cancel of an operation in progress is positive, and the operation is answered negatively with `Abortion-by-cancel`
    (Figures 40 and 42);
  - a cancel in the change of an enhanced command, and any request during a command, are refused (Table 108);
  - the association of the owner lost during a change terminates the command with `Abortion-by-communication-loss` (Table 108
    does not cover it; confidence medium).
- **Consistency** (CTL-004; 7-2 §20.5.2: all primitives of a sequence carry the same origin, test flag and control value):
  a request whose origin or test flag differs, or whose value differs from the sequence's, is refused with
  `Inconsistent-parameters`. A selection of normal security carries no value: its first operation gives it one.
- **The two kinds of refusal** of a request that does not belong to the model: `class-not-supported` for a direct model
  (Table 108, a SelectWithValue or a Select), and `NotInModel` (an unknown control request, Table 108, last row) for the
  select requests of the other selection model. They are different service errors (K2 maps them).
- **Not in K1**: the time-activated operate (§20.4, `TimOper`), which the mapping of K2 declares unsupported; the `ctlNum`
  is carried and not checked (7-2 §20.5.2: its use is an issue of the client); the check bits are passed to the application.
- **Sources**: IEC 61850-7-2 §20.1 to §20.3 (text and Figures 40, 42, 43, 44), §20.5.2 (Table 110), Table 108; the enumerations
  `CtlModelKind`, `SboClassKind`, `BehaviourModeKind` and `ControlServiceStatusKind` of the catalogue (P5).
- **Confidence**: high for the transitions of the four figures (the figures were read, the tests follow them); medium for the
  choices listed above, which are marked where they are made.
- **Test**: `tests/control.rs` (31 tests): every transition of the four models, each refusal of Table 108 the models reach, the
  timers, the association loss, the consistency of a sequence, the catalogue names of the causes, and the determinism.

### D-021 — The checks of an SCL document: the frozen schema, the references and the gaps (task S2; SCL-002 to SCL-004)

- **Decision**: the structure is checked against the 2007B schema of IEC 61850-6:2009 (edition 2.0), frozen in the private
  package and read once by a generator (`norm_preparation/extract_scl_schema.py`), which writes the table
  `crates/igs-iec61850-mms-core/src/scl/generated/schema_2007b.rs` (157 types, 9 root elements). The validator reads the table,
  never the XSD. The occurrences of enclosing groups multiply in (a `choice` with `maxOccurs="unbounded"` repeats its members);
  under a choice, a member's minimum is 0, so the count required of a choice as a whole is not checked.
- **Wildcards**: the schema's `##other` wildcards admit foreign-namespace elements only. The parser keeps unqualified attributes
  only (namespaced attributes such as `xsi:schemaLocation` are not kept), so every attribute the validator sees is unqualified:
  an attribute the schema does not name is refused, whatever the type's attribute wildcard. `Private` is opaque: its content is
  not checked.
- **Anonymous types** are named `anon:<root element>` (SCL, LN0) or `<Parent>/<Element>` (local types). A child of simple type
  is `simple`, and its text is not checked.
- **References** (SCL-003): `lnType`, the `type` of DO, SDO, DA and BDA (a DA or BDA of bType Enum takes an EnumType, of bType
  Struct a DAType; a DA with either bType and no type is refused); the members of data sets (`FCDA`: the LDevice of `ldInst`,
  the logical node of prefix, class and instance, then `doName` and `daName` followed through the templates); the `datSet` of
  control blocks; `iedName` of `ConnectedAP` and `ExtRef`. When a logical node's type is unresolved, its data-set members are not
  checked again (one finding, not a cascade).
- **Namespaces** (SCL-004): a root outside the 2007B namespace (the other root namespaces are not supported), and an element of
  another namespace where the schema admits none.
- **Diagnostics**: each has its code, the file, a path (`SCL/IED[IED1]/AccessPoint[AP1]/Server/LDevice[LD1]/LN[GGIO1]`), the line
  of the start tag (`Element.line`, added to S1) and the message. A document with any finding is refused as a whole, and every
  finding is returned, sorted by line.
- **Decision to confirm (R1 or the user)**: the root attribute `release` (written by many tools, for example `release="4"`) is
  absent from the frozen 2007B schema, which has only `version` and `revision`; such a document is refused with SCL-002. The
  frozen schema is the reference of S2, and the held package does not show the later edition that adds the attribute. To be
  settled when real files are checked (the SCD examples of the RTE captures).
- **Gaps**: the order of the elements of a sequence is not checked; `ExtRef` is checked for `iedName` only (`srcLDInst` and
  `srcCBName` are not resolved against the other IED); control blocks are checked for `datSet` only (the other service
  declarations, such as `Services`, follow the table); the number of the exact occurrences of a choice; the dataset member
  `fc` and the uniqueness of the names; integer attribute values are checked against the range of their type.
- **Confidence**: high for the table (its counts checked with xmlschema, and the types of IED, ReportControl, DataSet and DOType
  checked against the XSD by hand); high for the refusals (tests); medium for the references, whose rules were written from the
  XSD and not yet confronted with real SCD files.
- **Tests**: `tests/scl_validate.rs` (21 tests): a valid document with a `Private` element accepted; each refusal above with its
  code, message and line; several findings all reported in line order; a diagnostic shown with its file, line, path and code;
  a malicious document type refused before the checks.

### D-022 — The build of an IED from an SCL document: what it models, what it refuses, what it keeps (task S3; SCL-001, SCL-003, SCOPE-002)

- **Decision**: `scl::build::build(document, file, ied, capabilities)` builds the model of one IED, named explicitly (SCL-001),
  from a document that has passed `validate` (S2). Each logical node is a `LogicalNode` of its catalogue class; its data objects
  are the DOs of its LNodeType, each with the DA and SDO names of its DOType (the model adds the mandatory ones). The data sets are
  kept as declared (device, node, members); the address parameters of the connections of the IED are kept as (type, text). The
  build is all or nothing: a `Configuration` only when no finding remains; otherwise every diagnostic, sorted by line.
- **Refused, never dropped** (SCL-003): an instance value (a `Val` under a DAI or SDI: the value is not built by this release,
  and ctlModel is among them, so this is a gap for K2, see below); a DOI that is not a DO of the LNodeType; a DAI or SDI that is not a
  member of the DO's type; a member of a template that the catalogue class does not instantiate (checked against the built tree, so
  that an optional member the model could not give is not lost silently); a model refusal (unknown or missing mandatory DO, unknown
  attribute, duplicate name), with the model's own message.
- **Capabilities** (SCOPE-002): the declared services of an access point (`Services`) and the declared blocks of a node are checked.
  `Capabilities::current()` executes `GetDirectory`, `GetDataObjectDefinition` and `ReadWrite`. Any other declared service is refused
  as not executed. The blocks (ReportControl, LogControl, GSEControl, SampledValueControl, SettingControl, Log, Inputs) are refused
  whatever the capabilities say, because this release does not build them: a block accepted and then not built would be lost.
- **Gaps, to close before the tasks that need them**: instance values (ctlModel for K2, the descriptions of D tasks); data sets are
  not yet served (D1); the FCDA functional constraint and the attribute's constraint are not compared; the nested data attributes of
  an optional SDO are checked against the built tree only through the template (not against the catalogue's own presence rules); the
  ordering of sequence elements (already a gap of S2).
- **Address parameters**: `Address/P` values are kept as text; they are not interpreted (no listening address is derived from them
  in this release). `S1` now keeps the direct text of each element (`Element::text`), needed for these values.
- **Test**: `tests/scl_build.rs` (9 tests: the sample builds its model, data sets and addresses; each refusal above with its code and
  message; an unknown IED; several findings refuse the whole build). `igs-iec61850-mms/tests/scl_discovery.rs` (feature `scl`): the
  sample configuration is served and discovered by the typed client (devices, nodes, functional groups, named variables).
- **Sample**: `crates/igs-iec61850-mms-core/tests/data/sample.cid` is written for the tests (a device LD1, nodes LLN0 and GGIO1, one
  data set, two address parameters). It is not an extract of the standard and not a field file.
- **Confidence**: high for the refusals (tests) and the discovery of the sample; medium for the model mapping of real CIDs, which
  have instance values and more nodes: to be confronted with the RTE field captures and SCD examples when the user provides them.

## Third-party code and sources

| Component | Licence | Use | Notes |
| --- | --- | --- | --- |
| `igs-iso9506-mms` crates | Apache-2.0 | MMS stack | Same maintainers; added with the first task that needs it |
| roxmltree 0.21 | MIT OR Apache-2.0 | XML reading in `xtask` (task F3) | No dependency; DTDs refused |
| IEC code components (NSD 2007B6, SNSD 2003A4) | IEC Code Components EULA | Generated catalogue (task F3) | See NOTICE and D-002; files not in this repository |
| libiec61850 | GPL-3.0 | Reference peer of the bench, tag `v1.6.2.1`, commit `c28c27c`, black box only; image built locally, not distributed | Source never read; user documentation only |
| Wireshark (tshark) in `nicolaka/netshoot` | GPL-2.0 | Independent decoder of the bench captures | Run as a tool, not linked |
| proptest 1 | MIT OR Apache-2.0 | Development dependency of the core crate: property tests of the names and references (task A1) | Not published with the library |

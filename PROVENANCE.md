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

### D-023 — The static data sets: their members, their lifecycle and their writes (task D1; DS-001, DS-002, AT-08)

- **Decision**: each data set of a configuration is a named variable list of its device's domain (`ServedModel::from_configuration`).
  Its members are the variables that the FCDA selects, in document order and, for a member without a data attribute, in the model's
  order. A member with a data attribute selects that variable (a structure is one variable); a member without one selects the leaves
  under its data object and functional constraint, not the structures that contain them (8-1 §14: the data of the object under the
  constraint). A member that selects no variable of the model refuses the configuration (SCL-003), so it is caught at build time
  (`resolve_data_sets`); a member whose functional constraint does not match its attribute is refused the same way.
- **Lifecycle** (DS-002): a static list is not deleted by a client. A delete request that would match one (by name, in its domain, or
  in the VMD scope) is refused whole, with access denied, and deletes nothing: no partial deletion. A definition under the name of a
  static list is refused by the VMD (object exists). Static lists are not association-specific: a disconnection leaves them as they are.
- **Reads and writes**: GetDataSetValues is a Read with the list; SetDataSetValues is a Write with the list. Each member keeps the
  access rules of its variable (the read-only guard of A4): a write to a status member fails that item, and the other items are
  written. The request as a whole is not atomic, as the MMS Write is not; each item is written or refused as a whole.
- **Not in D1**: dynamic data sets (DS-001 by M2: they are defined by a client and belong to the association), data sets referenced
  by enabled service objects (none exists before the reports, D2 and the buffered reports), and writable members in the sample
  (the sample has none, so the positive write is not tested here: to add with a configuration that has a setpoint member).
- **Test**: `igs-iec61850-mms/tests/data_sets.rs` (feature `scl`, 5 tests: the list is discovered, the members keep their order, the
  values are read in that order, a static list is not deleted by a specific or a domain request, and a write to read-only members fails
  item by item); `igs-iec61850-mms-core/tests/scl_build.rs` (the resolution of members: order, the leaves of a data object, a member
  selecting nothing).
- **Confidence**: high for the expansion and the lifecycle (tests); medium for the choice of refusing a delete request whole rather than
  deleting its unprotected lists (a client sees one answer; it is the conservative reading of DS-002).

### D-024 — The unbuffered report control block as a state machine: reservation, writes, triggers, buffer time, integrity and GI (task D3; RPT-001)

- **Source**: IEC 61850-7-2 §17.2 (17.2.2.5 to 17.2.2.13, 17.2.3.2.3, 17.2.4) and 8-1 §17.1.3 (table 73, the rules of the
  writes). Figures 26 (the BRCB state machine, whose `disabled` and `enabled` states the URCB keeps, without `resync` and
  without entries) and 30 (the general interrogation) were read from the PDF pages: the prepared text marks them missing.
  The reading was limited to these figures and to the clauses of the task.
- **Reservation** (17.2.4.5): enabling, and every configuration write, need the reservation of the writer. A reservation is taken
  by a free block or by its holder; another client is refused. `Resv` is written only while the block is disabled (8-1 table 73,
  c1). When the holder's association is lost, its reservation is released and a block it enabled is disabled.
- **Writes while enabled**: `RptID`, `DatSet`, `OptFlds`, `BufTm`, `TrgOps` and `IntgPd` are refused while enabled (c1 and the
  table). A write of a data set that does not exist is refused; the NULL data set cannot be enabled (8-1 17.1.3).
- **Enabling** resets `SqNum` to zero; the sequence number then increments per report and wraps after 255 (INT8U). Integrity, if on,
  starts at enabling.
- **Triggers**: the notifications of dchg, qchg and dupd count when their option is on; any other is ignored. Without a buffer time, each
  notification is one report. With a buffer time (17.2.2.9), the notifications of the interval are combined into one report at the expiry,
  and the next notification starts a new interval. A second notification of the same member during the interval sends the pending report
  at once and restarts the interval: this is the strict reading of 17.2.2.9 for status values, and the substitution allowed for analogue
  values is not taken.
- **Integrity** (17.2.3.2.3.3): every member is reported at each period; the buffered notifications are sent first, as the text requires.
  The period is re-armed from the instant of the expiry, not from the nominal time, so the timer is deterministic under controlled time.
- **General interrogation** (17.2.3.2.3.4, figure 30): refused while disabled (`NO GI ALLOWED`); with the GI trigger on, the buffered
  notifications are sent and then every member, with the reason GI; with the trigger off, the write is answered without a report. GI
  is not kept as a state: it is set back to FALSE at once.
- **Unbuffered block**: no event history (figure 30, NOTE), so a report is not kept for later; `OptFlds.buffer-overflow` and `entryID`
  are not applicable (8-1: their value is false). The application's `Arm` replaces the previous expiry of the same timer.
- **Not in D3**: the delivery and the encoding of the reports (D4), the reading of the attributes (GetURCBValues), the preconfigured
  reservation and owner of an SCL `ClientLN` (the SCL build still refuses `ReportControl`, SCOPE-002, until D4 wires the block into the
  server), and the BRCB (B tasks).
- **Test**: `igs-iec61850-mms-core/tests/report_urcb.rs` (16 tests with controlled time): reservation by one client at a time, enabling
  needs reservation and data set, configuration only by the holder and only while disabled, the sequence number restarts and wraps,
  data-change without buffer time, triggers that are off, the buffer time, the repeated notification, integrity with its buffered
  entries first, GI enabled and disabled, disabling drops pending notifications, loss of the holder, and the order of the entries.
- **Confidence**: high for the transitions and the reservation rules (figures and clauses read); medium for the strict flush of a
  repeated status notification and the re-arm rule of the integrity timer, which are readings to confront with a reference client.

### D-025 — The layout and the delivery of the unbuffered reports (task D4; RPT-002, RPT-003)

- **Layout** (8-1 §17.2.1, table 74): one `InformationReport` on the VMD-specific list `RPT`, whose results are, in order: `RptID`, the
  reported options, the sequence number (INT8U), the time of entry (binary time, six octets: the milliseconds of the day, then the days
  since 1 January 1984), the data set name, the configuration revision, the inclusion bit string, the data references, the values, and the
  reasons for inclusion. The reported options and the reasons take their bits from tables 41 and 38 (data-change 1 … general-interrogation
  5); the options are at least ten bits, the reasons at least six. The buffer-overflow, entry identifier and segmentation options are never
  reported (8-1 17.1.3 and 17.2.5.2). An empty `RptID` is replaced by the block reference, `LD/LN.name` (7-2 17.2.4.3).
- **Inclusion**: one bit per variable of the data set (the NamedVariableList members, 8-1 17.2.1), not per FCDA. A member that selects a
  data object selects several variables, and an entry of the report includes all of them. The data-reference of a variable is `LD/LN.DO.DA`
  (the constraint is left out).
- **Triggers**: the trigger flags of a data attribute come from its common data class in the catalogue (dchg, qchg, dupd). A change of the
  value raises the data-change or the quality-change of the attribute; any update raises the data-update. One notice carries one trigger:
  data-change, then quality-change, then data-update (7-2 NOTE 3 prefers a single report).
- **Owner and delivery** (RPT-003): a report is queued for the association that reserved the block and enabled it, and for no other. The
  queue is the observable delivery of N5: a saturated or closed association is a refusal of the delivery, seen by the application through
  `Server::next_delivery`. The release of an association (`Model::released`) releases its reservation and disables its block (D3).
- **Clock and polling**: the blocks keep their time from the creation of the served model; the requests and the timers use that clock. The
  server polls the model at its tick: the notices, the expired timers and the outbound reports are handled there.
- **Bounds** (RUN-001): the notices are kept up to 65 536; beyond that, the oldest is dropped and counted, and `Reports::dropped_notices`
  gives the count. A dropped notice is never silent.
- **Not in D4**: the MMS attributes of the blocks (`RptEna`, `Resv`, `DatSet`… written and read through the services), and the client API:
  these are task D5, so the application drives a block through `Reports::request`. The blocks named with an index (`indexed="true"`) keep
  their name as written. A value written by an MMS client raises no notice: only the application's updates do (a gap to close with D5).
  Buffered blocks (B tasks) are refused as SCOPE-002.
- **Test**: `igs-iec61850-mms-core/tests/report_encode.rs` (8 vectors: the options bits, the order of table 74, the inclusion bits, the
  integrity report, the reason bits, the binary time, the object reference, the empty identifier); `igs-iec61850-mms/tests/reports.rs`
  (5 tests on the served sample: the owner receives the report of a change and no other association; a change that changes nothing reports
  nothing; a change outside the data set reports nothing; a disabled block reports nothing and refusals are explicit; the close of the owner
  releases and disables the block). The saturation diagnostic is N5's (AT-06): it is not re-tested here, since it depends on the timing of the
  server's tick.
- **Confidence**: high for the layout (vectors from the tables); medium for the choice of one trigger per notice and for the reading of the
  dupd trigger, which the attributes of the catalogue give; to be confronted with a reference client in D6.

### D-026 — The MMS mapping of the controls: services as writes and reads, the answers, and the sequences (task K2; CTL-001, CTL-002, CTL-004)

- **Mapping** (8-1 §20.4, tables 94 to 100 and 109): Select is a read of `SBO` (positive: the object reference `LD/LN$CO$DO`; negative: the NULL
  value); SelectWithValue a write of `SBOw`; Operate a write of `Oper`; Cancel a write of `Cancel`. Each write carries the structure of table 94:
  `ctlVal`, `origin`, `ctlNum`, `T`, `Test`, `Check`. The optional `operTm` is absent: TimeActivatedOperate is declared unsupported (no
  `operTm` component, and no time-activated operation is accepted). A select read of an object that is not controllable is a NULL value.
- **Answers**: a positive write is `Success`. A negative write is a `DataAccessError` (table 100) and, for the services of table 95, a
  `LastApplError` (an `InformationReport` on the VMD list `LastApplError`, table 109) to the client that wrote. The `CommandTermination` is an
  `InformationReport` on `LN$CO$DO$Oper` with the structure of the operation (table 105); a failed command sends `LastApplError` instead. The
  `Error` component is `Operator Test Not OK` (3) for a failed test, `No Error` (0) otherwise; the additional cause is the value of the
  catalogue enumeration `ControlServiceStatusKind`.
- **Access results (table 100)**: the prepared text gives the rows TEMPORARILY-UNAVAILABLE (already selected or being operated), HARDWARE-FAULT
  and OBJECT-ACCESS-DENIED, and no more: its last rows are cut. The mapping used: a selection held by another client, a command in execution,
  or a locked object is TEMPORARILY-UNAVAILABLE; an inconsistent parameter is OBJECT-ATTRIBUTE-INCONSISTENT; any other refusal is
  OBJECT-ACCESS-DENIED. To confirm against the complete table 100 (the text of the rows is missing from the prepared text).
- **Checks and execution** (CTL-003): the machine asks the application for its checks (operative, dynamic, selection) through `ControlHandler`,
  answered synchronously; the output is activated through `ControlHandler::execute`. The default accepts every check. The deferred outcomes
  are K3.
- **Status change**: an enhanced model ends its command when the status (`node$ST$do$stVal`) of the object changes (a notice of the updater with a
  change), or at the operate timeout (`operTimeout`, negative: `Time-limit-over`).
- **Control objects**: each data object whose `ctlModel` is not status-only is a control object, with its parameters from the configuration
  (`sboClass`, `sboTimeout`, `operTimeout`, and the behaviour `Beh$stVal`). The defaults are 10 s, 3 s, and behaviour `On` (a value
  missing from the configuration takes these defaults; the values are the 2007B attributes of the data objects).
- **Sequence of a normal-security selection (correction of K1)**: the select of a normal-security object is a read without parameters, so its
  sequence is unspecified; the first operation fixes its origin, its test flag and its control value. The K1 test that expected the origin of the
  select to be checked is rewritten on the enhanced model, where the select carries its parameters; a test of the normal selection is added.
- **Initial values (correction of S3, D-022)**: a `Val` of a basic data attribute directly under its data object is kept by the configuration and
  applied to the served model at its MMS type (`ctlModel`, `sboTimeout`, `Beh$stVal` are such values). The values nested in a structure remain refused.
- **Not in K2**: the reports of the `LastApplError` precede the write response in the standard, and here they follow it (the next poll): the client
  matches them with the origin and the control number. The `T` of the operation is copied from the request into the termination; its timestamp is
  not checked. A control object of an other logical device of the IED is served by its device only. The time-activated operate (§20.10) is
  refused.
- **Test**: `igs-iec61850-mms/tests/controls.rs` (9 tests with the served configuration, the network clients and the MMS writes and reads): a direct
  normal operate without selection; a select-before-operate refused without selection, with `LastApplError` (`Object-not-selected`); a selection of
  one client refused to another (NULL select, temporarily unavailable operate); an enhanced select with value and operate ending with the status
  change; an enhanced operate ending negatively at the operate timeout; an interlock of the application refusing the operate with its cause;
  an object without control model not a control object; a cancel releasing the selection; the close of the selecting client releasing it.
  `igs-iec61850-mms-core/tests/control.rs` (32 tests: the state machine, with the normal selection test added).
- **Confidence**: high for the state machine (figures and clauses of K1, the sequence correction read from 7-2 §20.5.2); medium for the table 100
  mapping (the complete table is missing from the prepared text); to confirm with a reference client in D6 and K3.

### D-027 — Deferred outcomes and the typed control API (task K3; CTL-003, CTL-004, ACSI-001)

- **Deferred outcome** (CTL-003, the extension the requirement asks for): the machine takes `Event::Deferred { outcome }`. For an enhanced command
  that waits for the change of its status (the state `Change`), a positive outcome completes it as a change does, and a negative one terminates it
  with its cause (CommandTermination negative, with the additional cause), and ends the selection (7-2 §20.5). In another state the event is
  ignored.
- **The application's path** (`ControlHandle`): `ServedModel::control_handle()`; `complete(reference, outcome)` queues the outcome, and the next poll of
  the model applies it. A reference that is not a control object of the model is refused (`ControlError::UnknownObject`), so an outcome is never
  silently lost. The handle can be used from another task.
- **Not deferred**: the checks. A write response of MMS is given by `Model::dispatch`, which answers at once. A check that the application cannot
  answer at once therefore has no place in this release: the handler answers from its cached state, and must not block (CTL-003). A deferred
  response of a write would need the MMS server to answer later (a change of the MMS layer, not of this task). The deferred outcome of the command
  is the path that is implemented.
- **Typed client API** (`client_control`): `select` (a read of SBO: `Some(reference)` or `None`), `select_with_value`, `operate`, `cancel` (writes of
  the structure of table 94, with a `Command`: control value, originator, control number, test flag, checks; the time is the zero timestamp, which the
  server does not check). `termination(report)` decodes a CommandTermination (on `Oper`) or a LastApplError (table 109) from the reports of the
  association; `cause_name` gives the name of an additional cause from the catalogue.
- **CTL-004 cases tested**: the wrong client (competing clients refused), the invalid transition (an operate without selection refused), the cancel,
  the disconnection (the selection of a closed association released), the asynchronous failure (a deferred negative outcome), and the timeout of an
  enhanced operate (the operate timeout). The expired selection (the select timeout) is tested at the state machine (K1).
- **Test**: `igs-iec61850-mms/tests/controls_client.rs` (8 tests, feature `scl`: the select-before-operate object through the typed API, competing
  clients, cancel, the deferred positive and negative outcomes decoded, the operate timeout, an unknown object refused by the handle, and a report that is
  not a termination not decoded); `igs-iec61850-mms-core/tests/control.rs` (two new tests of the deferred event).
- **Not run here**: the interoperability with the reference peer (the controls of the bench, AT-11 with the peer): it needs the bench containers of F4 and
  is left to D6 and the bench campaign (the plan's "and with the peer").
- **Confidence**: high for the state transitions (tests) and the decoding (vectors built from the reports of the tests); medium for the
  reading that the outcome of a deferred command is the CommandTermination of the enhanced models only (the normal models do not terminate, as K1 states).

### D-028 — The client side of the unbuffered reports: attributes as variables, configuration by writes, decoding of the reports (task D5; RPT-001, RPT-002, RPT-003)

- **Attributes of a block**: each attribute of the block (8-1 table 73) is an MMS named variable `node$RP$block$attribute` in the device's domain
  (`RptID`, `RptEna`, `Resv`, `DatSet`, `ConfRev`, `OptFlds`, `BufTm`, `SqNum`, `TrgOps`, `IntgPd`, `GI`, `Owner`). The served model names its data
  attributes the same way (flattened, as D-005 chose), so a client reads a block by its components. The structure of 8-1 (the whole block as one
  variable, read by GetURCBValues) is not served: a client expecting the structure must be checked against the peer (D6).
- **Reads**: a read of the attributes is answered from the state of the block: `RptEna` and `Resv` from the state machine, `SqNum` from the next report,
  `DatSet` as the empty string when NULL, `GI` as FALSE (a request is not kept as a state), `Owner` as an empty octet string (the owner is not
  reported as an IP address in this release).
- **Writes**: each attribute write is one request of the state machine of D3: `Resv`, `RptEna`, `DatSet`, `OptFlds`, `BufTm`, `TrgOps`, `IntgPd`, `RptID`
  as the standard gives them (configuration only while disabled and by the holder of the reservation). `GI` TRUE is a general-interrogation request;
  `GI` FALSE is accepted and changes nothing. `ConfRev`, `SqNum` and `Owner` are read-only: access denied. A refusal is an access result: a
  reservation held by another client is TEMPORARILY-UNAVAILABLE, the other refusals of the state (not reserved, enabled, general interrogation while
  disabled) are OBJECT-ACCESS-DENIED, and a data set that does not exist or is not set is an invalid value (table 100 is incomplete in the prepared
  text: see D-026).
- **Client API** (`client_report`): `urcb_values` reads the attributes in one request; `urcb_write` writes one attribute (`UrcbWrite`), the typed form
  of each write. `decode_report` decodes an `InformationReport` on `RPT` in the order of table 74: the options (table 41), the sequence number, the time
  of entry, the data set name, the configuration revision, the inclusion bit string, the references and values of the included variables, and their
  reasons (table 38). The decoder reads the options of the report, so the layout does not depend on the block's configuration; an entry carries the
  position of its variable in the data set.
- **Test**: `igs-iec61850-mms/tests/reporting.rs` (AT-09, feature `scl`, 8 tests): the owner receives the decoded report of a change, in the layout (RptID,
  data set, sequence number 0 at the first report, configuration revision, the position and value of the variable, its reason); two clients with
  distinct subscriptions each receive only their block's reports; a disabled block reports nothing and its values are read; a general interrogation
  reports every member with its reason; the integrity period repeats a report of every member; a quality trigger alone does not report a data change
  but reports a quality change; a write by a client that does not hold the block is refused. `decode_report` refuses a message that is not on `RPT`.
  `igs-iec61850-mms-core/tests/report_encode.rs` checks the round trip of the options, the triggers and the reasons.
- **Not in D5**: the buffered blocks (B tasks); the indexed names (`indexed="true"`); the preconfigured reservation of an SCL `ClientLN` (a client set of
  the owner is not configured here); the interoperability with the reference peer (D6, the bench is not run in this session).
- **Confidence**: high for the encoding and decoding (round trip, and the layout of table 74 checked by the vectors of D4); medium for the flattened
  attribute names as the standard's clients read them, which is what D6 must confirm.

### D-029 — The reports against the reference peer: the forward rows, and what the reverse row needs (task D6; bench)

- **Rows that pass** (`bench/tests/test_d6_reports.py`, the reference example `server_example_basic_io`): our client `report` command configures the
  unbuffered block `LLN0$RP$EventsRCB01` (data set `LLN0$Events`), enables it, and decodes the reports of the reference server: the general
  interrogation (four entries, reason general-interrogation) and the integrity period (sequence numbers 0, 1, 2; four entries each, reason
  integrity). The capture decodes without an error in tshark.
- **Evidence of the reading**: the reference server serves the attributes of its block as variables `LLN0$RP$EventsRCB01$RptID`, `…$RptEna`,
  `…$DatSet`, `…$OptFlds`, `…$TrgOps`, `…$IntgPd`, `…$GI`, `…$SqNum`, `…$ConfRev`, `…$BufTm`, `…$Resv` (8-1 table 73, flattened): the names
  D-028 serves and reads. The data set is named `simpleIOGenericIO/LLN0$Events` (the domain is part of the name the peer reports).
- **Not passing: the reverse row.** The example client of the peer that receives the reports of our server needs (a) our server to serve a
  configuration read from SCL (the binary serves the demonstration model, without a report control block: the step is the S3 build wired into
  `igs61850-server`), and (b) a model that answers what the example reads (names and values of `simpleIOGenericIO`), which is a fixture that does
  not exist yet (the functional match of the example model, listed in `docs/interop.md`). Until both exist, D6 is not complete: the case of the
  plan's "bench rows in both directions" is open for the reverse direction.
- **The handshake**: the reference server requires the `str2` parameter CBB, which the client offers only under `--lenient-parameter-cbbs` (A7).
- **Confidence**: high for the forward rows (the peer's own reports, decoded; the capture decodes); the reverse row is not evaluated.

### D-030 — The dynamic data sets: persistent and association-specific, their lifecycle and their limits (task D2; DS-001, DS-002, AT-08)

- **Persistent** (domain-specific name): a client defines the set with `DefineNamedVariableList` on its device; the generic VMD holds it, and it lasts
  until a client deletes it (any association may delete it: a persistent set has no owner in this release). The members are the variables of the
  served model; a member that is not served refuses the definition (object non-existent).
- **Association-specific** (application association-specific name, `AaSpecific`): the served model keeps the set for the association that defined
  it, with the members as written. The generic VMD does not hold these names (8-1 and the MMS layer), so the served model does: definition, read
  (through the members, with the rules of the normal reads), write (through the members, with the rules of the normal writes), attributes and
  deletion are answered by `datasets.rs`. A set of another association is not seen; the same name in another association is another set; a
  release of the association drops its sets (`Model::released`).
- **Static sets are protected** (DS-002): a static set (D1) is never deleted or redefined by a client, whatever the association; the refusal is the
  one of D1 (the delete of a static set is refused whole; a definition under its name is object exists).
- **Referenced sets** (DS-002): a dynamic set cannot be referenced by a report block in this release: the blocks reference the data sets of their
  configuration (their DatSet is checked against the configured sets). So no enabled service object refers to a dynamic set, and the refusal
  "while referenced" applies to the static sets only, which are already refused. The DS-002 rule for dynamic sets referenced by enabled service
  objects becomes live with the dynamic references of the blocks (a gap: the blocks take a set of the configuration).
- **Not in D2**: the `GetNamedVariableListAttributes` of a persistent set reports `mms_deletable: true`, as the generic layer does; the access
  control of a persistent set (who may define or delete it) is not set; the limit on the number of sets per association is not set (RUN-001 limits
  of the model).
- **Client API** (`client_dataset`): `define_data_set`, `delete_data_set` (persistent), `define_association_data_set`, `read_association_data_set`,
  `delete_association_data_set`.
- **Test**: `igs-iec61850-mms/tests/datasets.rs` (feature `scl`, 4 tests with two associations): a persistent set defined by one client is listed,
  read and its members listed by another; a persistent set is deleted by another association; an association-specific set is private, the same
  name is another set in another association, and it is dropped with its association; a static set cannot be defined again and an unserved member
  refuses a definition.
- **Confidence**: high for the privacy, the lifecycle of the association sets and the static protection (tests); medium for the choice that a
  persistent set has no owner (the standard leaves the access control to the implementation; to confirm with the peer in D6).

### D-031 — The differences of the first edition that a client meets, and how the crate tells the editions apart (tasks E1, E2; ED1-001 to ED1-004)

- **Inventory** (E1): the differences are listed in the private notes (`norm_preparation/docs/spec/edition1-inventory.md`), with their clause
  sources (7-2 annex F, 8-1 annex K), their effect on a client, and their decision. The public repository holds only the decisions below, in our own
  words. The differences that matter for the code: the additional causes 26 and 27 (their meanings are exchanged between the first and the second
  edition, by the corrections of the tissues), the longer names (LD, LN, data object, object reference, control block name, `RptID`), the
  deprecated and renamed services and enumerated values, and the attributes that the first edition adds (`Owner`, `ResvTms`).
- **Detection** (E2, ED1-001): the edition of a device is read from its namespace string, `LLN0.NamPlt.ldNs` (FC EX): a string that carries the
  2003 namespace is the first edition, one that carries the 2007 namespace is the second. Any other string gives no edition, which is never assumed
  (`IecClient::detect_edition`, `client_control::edition_of_namespace`).
- **Profile** (E2, ED1-004): the edition is a parameter of the reading of the causes (`termination_in(edition, report)`,
  `cause_name_in(edition, value)`): the first edition exchanges the causes 26 and 27, and every other cause is the catalogue's. The default is the
  second edition. No check is relaxed for a server of the second edition, and an unknown edition keeps the second edition's checks.
- **Not done** (E3): an interop row with an Edition 1 peer. The reference peer of the bench is the second edition's example set; no first-edition
  peer is available, and no capture from the user is recorded. The behaviours of the first edition are declared untested. The complete value table
  of `ControlServiceStatusKind` for the first edition is not known: only 26 and 27 are changed by the annex, and the other causes are assumed equal.
- **A correction found on the way** (A2, E2): the visible and the Unicode strings of a given maximum were described with a positive size, which MMS
  reads as an exact length, so a value of any other length was refused. They are now variable lengths with the maximum, a negative size (ISO 9506-2
  §14.2.2.4.2, as the reference peer describes them). The table test of A2 is updated.
- **Confidence**: high for the codes and the detection of the namespace strings (tests with served devices); medium for the table of the causes of
  the first edition beyond 26 and 27 (to confirm with a first-edition capture, E3).

### D-032 — The gateway building blocks: one runtime, explicit mirrors, and the path of the values (task G1; GW-001)

- **Composition**: a process runs the clients of the downstream servers and the upstream server in one tokio runtime. The mirror of a downstream
  variable is explicit: a (downstream device, variable) pair maps to one (upstream device, variable) pair, and the names are the ones the models use.
- **Path of the values**: a mirror step reads the downstream variables of one client, then sets the upstream variables through the updater (A5), one
  update per upstream node. The updater never waits for the network, so the model task of the upstream server is not blocked (GW-001).
- **Errors are not hidden**: a failed downstream read is returned to the caller of the step; the loop `run_mirror` stops with that error. An item the
  downstream does not answer is left out of the step (not guessed), and an upstream refusal is returned.
- **Test**: `igs-iec61850-mms/tests/gateway.rs` (feature `scl`): two downstream servers (the sample configuration), two downstream clients, one upstream
  server built in code with two devices; a change downstream reaches the upstream model, and the upstream client reads it; the other downstream value
  stays as it was.
- **Not in G1** (G2): the quality on the loss of a downstream association (GW-003), the forwarding of controls with the deferred outcome (GW-004), and
  the upstream model built from SCL with its own reports (GW-002, GW-005; the reports of an upstream block already follow its own model, since the
  updater notices every change of the model).
- **Confidence**: high for the composition and the path of the values (the integration test); the example of the requirement with the peer's example
  servers as the downstream is not run here.

### D-033 — The gateway: quality on loss, forwarding of the controls, and the outcome of a forwarded command (task G2; GW-003, GW-004, GW-005)

- **Quality on loss** (GW-003): a step of a mirror that fails (the downstream association is lost, or a read fails) marks the quality of each
  mirrored status invalid (the `q` of its data object) and returns the error; a step that succeeds sets the quality good again. A stale value is not
  kept as good. The loop does not reconnect in this release: a restart is the caller's decision.
- **Forwarding of the controls** (GW-004): the upstream control object is served as any control object (K2), with a handler that forwards the
  command: `ForwardHandler::execute` sends the operate to the downstream control object (a spawned task, so the model task is not blocked). The
  upstream operate has been answered positively, as the standard has it; its termination waits for the downstream outcome. `forward_outcomes` reads
  the downstream events and completes the upstream command through its handle (K3) with the downstream outcome: a positive termination completes it,
  a negative one terminates it with the downstream cause (its name in the catalogue).
- **Upstream reports** (GW-005): the reports of the gateway come from its own blocks, since the updater notices every change of the upstream model
  (D4); a downstream report PDU is not relayed. This follows from the design and is not tested by a separate row in this release.
- **Proxy LDs** (GW-002): the upstream model is built in code here, and from SCL in the control test; the proxy `LPHD` node of 7-4 is not built.
  The mapping is explicit (the mirror and the device names).
- **Tests**: `igs-iec61850-mms/tests/gateway.rs` (two tests: a change downstream reaches the upstream model; a lost downstream association makes the
  mirrored quality invalid, and the failed step is returned); `igs-iec61850-mms/tests/gateway_control.rs` (one test: an upstream operate is forwarded;
  a downstream timeout gives the upstream termination negative with `Time-limit-over`; a downstream status change gives the upstream termination
  positive).
- **Confidence**: high for the quality on loss and the forwarded outcome (tests with real associations); medium for the mapping of a failed downstream
  operate to `Unknown` (the downstream refusal cause is not forwarded in this release: a refusal of the downstream operate is reported as unknown).
  The example with the peer's servers downstream (AT-16 with the reference) is not run here.

### D-034 — The capability statements: a checked source and the generated matrix (task V1; SCOPE-001, AT-14)

- **Source**: `docs/capabilities.tsv`, one row per item: requirement, area, item, state, and the tests that cover it (`path::function`, comma
  separated). A row without a test is a declared gap (BRCB-001, SEC-001 have none). The checker refuses a row without five columns, a test without a
  function, and a test whose file is missing or does not define the function.
- **Output**: `cargo xtask statements` writes `docs/statements.md` (by area, with the tests); `--check` only verifies (AT-14). The plan names
  `docs/capabilities.md` for this output, but that file is the hand-written register of the gaps and their decisions (committed with the server and
  time tasks). It is kept as it is: the generated rows are to be merged into it when the PICS, MICS and PIXIT drafts are written.
- **Not produced in this release**: the PICS, MICS and PIXIT drafts of V1. They are written from the rows in English, as a later step; the PIXIT
  values come from the implementer's configuration and are not invented here. The tables of the standard are not copied (licensed text).
- **Confidence**: medium on the choice of a tab-separated source over the layout of the standard's conformance tables; high on the checker.
- **Tests**: `xtask/src/statements.rs`: six tests (the parsing, the five columns, the empty function, a gap in the rendering, and the repository
  source: every test named there exists).

### D-035 — The examples, the changelog and the documentation gate (task V2)

- **Examples** (`crates/igs-iec61850-mms/examples/`): `client` reads the status of the demonstration model from a server given on the command
  line; `server` serves it and changes its status every second through the updater, as an application does; `gateway` runs one mirror step in one
  process, with a downstream and an upstream server on the loopback interface, and the demonstration model on both sides (its device name is the
  same on both, which the upstream server does not mind). The examples are built by CI (`cargo build --workspace --examples`).
- **Fix found by the gateway example**: `mirror_step` returned the number of upstream changes (a value and its quality per mirror, so two) while its
  documentation says variables. It now counts one per mirror read. No test depended on the old count.
- **Documentation gate**: the rustdoc warning of `Limits::max_instances` (a link to a private constant) is a plain reference; the CI step
  `RUSTDOCFLAGS="-D warnings"` passes, and no public item is undocumented.
- **Changelog**: `CHANGELOG.md` (Keep a Changelog) for the unreleased 0.1.0 of the crate. It lists the implemented capabilities and the gaps as they
  are in `docs/capabilities.tsv`; the version is not chosen here.
- **Confidence**: high for the examples and the documentation gate (they build and run); medium on the changelog's wording, which the maintainer
  should read before a release.

### D-036 — Fuzz targets on the state machines, and the first measurements (task V3; §12.4)

- **Fuzz targets**: the decoders of the report options and triggers are fuzzed with a round-trip assertion (the options and the triggers decode to
  the same values after the encoder), because a decoder that accepts bytes it cannot produce is a defect of the encoding. The state machines of the
  report block and of the control object are fed with event sequences decoded from the bytes; the clock only moves forward. The sequences check
  that no input panics or breaks an invariant of the machine that is asserted; they do not check the answers, which the unit tests do.
- **Duration**: one run of 60 seconds per target, recorded in `docs/ai-log/V3.md`. This meets the recorded-duration criterion of V3; it is not a
  campaign.
- **Measurements**: the method of `docs/measurements.md` (loopback, one item per read, the timing boundaries stated). Only the request throughput and
  latency are measured; the report, buffer and transport items of §12.4 are listed as not measured, with no number.
- **Confidence**: high on the targets (they build and run clean); medium on the measurements as a description of this machine only.

### D-037 — The buffered report block: states, history, overflow and resynchronisation (task B1; BRCB-001)

- **States** (7-2 §17.2.2.5, figure 26): `disabled`, `resync`, `enabled`. Association loss goes to `disabled` from any state and keeps the history
  (§17.2.3.2.3.6). The figure's arrows are partly lost in the text; the transitions are taken from the prose of §17.2.2.15 and of the state
  descriptions, and the tests name each one.
- **NULL `EntryID`** (confidence medium): the prose says it resynchronises to the oldest entry and that the state goes from `resync` to `disabled`.
  Implemented literally: from `disabled` it moves to `resync` (the figure's "set EntryID succeeds" arrow), from `resync` back to `disabled`, with the
  pointer at the oldest entry in both cases. An `EntryID` not in the history is refused: from `disabled` the state stays, from `resync` it goes to
  `disabled` (the MMS error code differs by state, which the encoder of B2 reports).
- **Enabling** (§17.2.2.15): from `disabled` the reports start with the oldest entry; from `resync` after the entry the client set. The first report
  after a `disabled` enabling carries `BufOvfl` (§17.2.3.2.2.8); a resynchronised one does not. `SqNum` restarts at zero at each enabling, as 17.2.2.10
  says; it is 16 bits (table 70, INT16U).
- **Buffer time** (§17.2.2.9): notifications are combined into one entry timed at the window's start. A second notification of a member inside the
  window closes the window at once and opens the next one: the rule the standard gives for status values, applied to every member (the analogue rule
  for measured values is a choice, not taken). With a buffer time of zero each notification is one entry. A buffer timer that fires before the
  deadline of the open window is ignored: the timer of a replaced window is stale.
- **Integrity** (§17.2.2.12, §17.2.3.2.3.6): integrity entries of all members are buffered in every state, from `start` and then each period. A change of
  `IntgPd` or `TrgOps` restarts the period; a change of another attribute does not.
- **Overflow** (§17.2.3.2.2.8): the history keeps `capacity` entries, given by the caller (the standard leaves the number to the server, NOTE 2 of
  §17.2.3.2.3.6). The oldest entry is discarded; if it was not yet sent, the next report carries `BufOvfl` and the pointer moves past it. An entry sent
  and then discarded is not a loss. While enabled every entry is sent at once, so an unsent entry exists only while disabled or resynchronising.
- **Purge and configuration** (table 70, notes c1 and c2): `PurgeBuf` and the configuration writes are refused while enabled. A change of `RptID`,
  `DatSet`, `BufTm`, `TrgOps` or `IntgPd` purges the history and the open window while disabled, as the table says. `OptFlds` is c2 only: no purge.
- **Not in this release** (B2 or later): the reservation and the owner (`ResvTms`, `Owner`, c3), the general interrogation, the segmentation of the
  integrity and general-interrogation snapshots, and the MMS encoding of the entries (`EntryID` as an octet string, `TimeOfEntry`). The client side is B2.
- **Known gap, to close in B2**: an entry keeps its members and reasons but not their values. The values are read when a report is formatted, so a
  report of an old entry carries the current values, not the values at its time (17.2.3.2.2.9 and figure 34). The fix is to carry the value in the
  notification; it changes the notification event of this machine, so it is made with the encoding, not here.
- **Tests**: `igs-iec61850-mms-core/tests/report_buffered.rs`, fifteen deterministic tests (disconnect, resumption, overflow, resynchronisation, purge,
  refusals, integrity, sequence numbers, the last `EntryID`).

### D-038 — The buffered reports: the wire layout, the values of the entries, the owner, and the segments (task B2; BRCB-002, BRCB-003)

- **Wire layout**: the results follow IEC 61850-8-1 table 74 (RptID, OptFlds, SeqNum, TimeOfEntry, DatSet, BufOvfl, EntryID, ConfRev, SubSeqNum,
  MoreSegmentsFollow, the inclusion bit string, the data references, the values, the reasons). 7-2 table 73 lists the parameters in another order,
  and does not list the inclusion bit string, which 8-1 requires in every report. The decoder follows the table of the wire. The first attempt
  against the reference server failed on this order, and the failure is the evidence for the choice (matrix C2 of `docs/interop.md`).
- **Inclusion bit string**: one bit per variable of the data set; a segment sets the bits of its own variables. The decoder reads the number of
  values from the set bits, not from the length of the list.
- **Values of the entries**: an entry keeps the value of each of its variables at the moment it is made: the update carries the written value in
  its notice, the block keeps the last value notified of each variable, and an entry takes those values (`Action::Entered`). This closes the gap of
  B1 (D-037), where the report read the values when it was formatted.
- **Owner**: the association that enabled the block, set before the reports of the enabling are formatted and cleared when the block is disabled
  or the association is lost. The reports go to the owner only (RPT-003).
- **Segments**: a report with the segmentation option is split into segments of at most 4096 estimated octets (a conservative count of each value's
  encoding); each segment has the same sequence number, its own subsequence number, and `MoreSegmentsFollow` on all but the last. The negotiated
  PDU size is not read by this release, so the budget is an estimate, not the limit of the association. Without the option the report is one segment.
  The unit tests cover the split; no end-to-end test builds a report of that size.
- **Configuration from SCL**: a buffered `ReportControl` is built into `Configuration::buffered` (with `bufOvfl` and `entryID` from its `OptFields`).
  The segmentation option is not in the SCL: the client sets it.
- **Not in this release**: the general interrogation and the reservation (`ResvTms`, `Owner` as a reservation) of the buffered blocks; the history
  is bounded by a fixed capacity of 256 entries and is not trimmed by the standard's entry lifecycle, which the standard leaves to the server.
- **Interop**: the reference server's buffered integrity reports decode without an error in tshark, and our client receives them in order
  (`bench/tests/test_b2_buffered.py`). The reverse direction and the resumption against the peer are not tested.
- **Confidence**: high on the layout (the peer's reports decode with it); medium on the segment budget, which is a choice of this release.

### D-039 — The conformance drafts are written by hand, and the generation is open (task V1)

- **Drafts**: `docs/conformance-drafts.md` holds the ACSI statement, the PICS, the MICS and the PIXIT as drafts for review. Each claim names its row of
  `docs/capabilities.tsv`, and each PIXIT value is the default of the code, named by its constant.
- **Why by hand**: the plan has `cargo xtask statements` write these drafts; the rows of the source carry the states and the tests, but not the
  service table, the logical node list or the PIXIT values, which are text of the implementer. Generating them needs those inputs in the source first.
  Decision of the owner: the hand-written drafts are the deliverable of this release, reviewed before publication (R1). V1 is ticked on
  its Done-when (AT-14). Generating the drafts from data is left for a later release.
- **Not copied**: the tables of the standard's annexes (PICS, MICS, PIXIT forms) are not reproduced; the drafts use their own structure, which is
  licensed text's free paraphrase of what the library does.
- **Confidence**: high on the PIXIT values (read from the code); medium on the ACSI and MMS lists, which restate the rows and the dispatch.

### D-040 — The reverse reports: the names of the data sets, the block as a structure, and the object reference of DatSet (task D6, reverse)

- **Data set names**: a data set declared in a logical node is named in MMS `LN$name` (`LLN0$Events`), as 8-1 has it, and the `DatSet` attribute of a
  block is the same name. The SCL build qualifies the names with the node. Before, the names were the bare SCL names (`Events`), which the reference
  client refused with `object-non-existent`. The change touches the SCL build, the tests that name data sets through the SCL, and the client's data set
  identifier (`$` is allowed in an identifier, not in a device name).
- **The block as a structure**: a read of `node$RP$name` (or `node$BR$name`) returns the attributes as one structure, in the order of the block's table
  (8-1 table 73 for the unbuffered block, 7-2 table 70 for the buffered one). The reference client reads the block this way (`getRCBValues`).
- **`DatSet` as an object reference**: the server answers `device/LN$name` (the peer's form), and accepts a write of either that form (the device of the
  block) or the bare name. The client writes the reference form, which the peer requires (`ParameterValueInconsistent` otherwise); the client's readers
  take the bare name back.
- **Evidence**: the reverse row of `docs/interop.md` (matrix C3): 22 reports of our server in the capture of the reference client, with no service error.
  Each finding above was a failure of the reference client's run, read from the capture (`tshark` on the pinned image), not inferred.
- **Confidence**: high on the names and the structure (the reference client runs with them); medium on the order of the structure's components, which
  the reference client does not complain about but which is not checked against its decoder (it uses the positions of its own type).

### D-041 — R1, generated files from the IEC code components: kept in the repository with the IEC notice (owner's decision)

- **Decision** (owner, R1): the files generated from the IEC code components (`catalogue/generated/ns_*.rs`, `scl/generated/schema_2007b.rs`) stay
  in the repository under Apache-2.0 for the code, with the IEC copyright notice, the attribution note in each file header, the SPDX identifier
  `IEC-Code-Components-EULA`, and `NOTICE` (as they are now).
- **Check of the EULA text (2026-11-13)**: the text recorded in `NOTICE` is identical, word for word, to the SPDX text of
  `IEC-Code-Components-EULA` (fetched from spdx.org, which points to www.iec.ch/CCv1). It contains no purchaser condition: the words
  "purchaser", "purchase", "buyer" and "licensee" are absent. The earlier note about a purchaser condition came from the general terms of the IEC
  webstore for purchased publications, not from the Code Components licence. The official pages (www.iec.ch/CCv1 and `CC-EULA.pdf`) answered
  HTTP 403 to the check, so the official wording itself is not verified.
- **Modifications, still open**: the licence extends to "modifications permitted under the relevant IEC standard". Whether the generated tables
  are such a modification depends on the code-component clause of the standards (7-2, 7-3, 7-4, 8-1, 6), which is in the purchased text. Not
  checked here.
- **Redistribution (fixed in 0.1.3)**: the EULA requires that redistributions keep the copyright notice, the list of conditions and the
  disclaimer. Before 0.1.3 the published crates did not contain `NOTICE` or the licence file (the generated files point to "NOTICE at the root"),
  so 0.1.0 to 0.1.2 do not carry them. From 0.1.3, `LICENSE` and `NOTICE` are copied into `crates/igs-iec61850-mms` and
  `crates/igs-iec61850-mms-core` and included in the packages (`cargo package --list`). The two copies must be kept identical to the root files.
- **Not done here**: the generated files were not regenerated; their headers keep the reference to "NOTICE at the root", which is true for the
  repository.

### D-042 — `Dbpos` as a two-bit coded enumeration (task Q1)

- **Decision**: the basic type `Dbpos` (the status of a double-point object: DPS `stVal`, DPC `stVal`, `XCBR.Pos`) is the coded enumeration
  DpStatusKind of two bits, with the four literals of 7-2 Table 24: `intermediate-state` 0, `off` 1, `on` 2, `bad-state` 3. Its MMS type is a bit
  string of two bits (`describe_coded_enum(DOUBLE_POSITION_SIZE)`), read and written with `DoublePosition` in `acsi.rs`. A read ignores the bits past
  the two of the type (§8.1.2.1).
- **Why**: a logical node with a `Dbpos` attribute was refused whole (`UnmappedType`), and the SCL build is all or nothing (`SCL-003`), so a breaker
  could not be served.
- **Sources**: IEC 61850-7-2 Table 24 (prepared text, `docs/spec/7-2`), 8-1 §8.1.2.1 and §8.1.2.3 (the coded enumeration as a bit string, most
  significant bit first), and the coded enumeration size rule of D-004.
- **Confidence**: high for the size (the largest value is 3) and the bit order (the same rule as the other coded enumerations, with vectors); high
  for the literals (Table 24 read directly). Medium for the reading of a longer bit string, which follows D-004.
- **`Tcmd` (same change)**: the step command of Table 23 (StepControlKind: `stop` 0, `lower` 1, `higher` 2, `reserved` 3) is a two-bit coded
  enumeration (`StepCommand`, `step_command_to_data` and `step_command_from_data`). In the namespace `Tcmd` is the type of the `ctlVal` service
  parameter of BSC and BAC, not of a data attribute, so no served object holds it: the mapping prepares its control (task K).
- **Not done**: the control of a `Dbpos` or a `Tcmd` (write of `ctlVal`) is refused until task K writes its control machine, as before.
- **Tests**: `acsi.rs`, the vectors of the four values and the reading of extra bits; `tests/model.rs`, a breaker position built with its two-bit
  status at zero, and the refusal of `BrcbTrk` (which holds `TrgOps` and `EntryID`) with its path.
- **End-to-end check**: `tests/data/breaker.cid` (an IED with `LD1`, `LLN0` and `XCBR1` with its six mandatory objects) builds through the SCL
  path, and `XCBR1$ST$Pos$stVal` is a two-bit bit string at zero. Without the `Dbpos` mapping the same file is refused with `SCL-003` on
  `XCBR1.Pos.stVal`, so the test checks the change.

### D-043 — The other basic types of Q1: trigger and option fields, entry identifier and time, PhyComAddr refused (task Q1)

- **Decision**: the basic types are mapped from 8-1 §8.1.3 as follows.
  - `TrgOps` (`TriggerConditions`, §8.1.3.9, Table 37), `OptFlds` (`RCBReportOptions`, §8.1.3.13, Table 41) and `SvOptFlds`
    (`SVMessageOptions`, §8.1.3.15, Table 42) are bit strings of their minimum size: six, ten and seven bits. Bit 0 (bit 1 for the sampled
    values) is reserved, and each named bit has its position in the table. When read, the bits past the minimum are ignored and the missing
    ones are zero, as D-004 gives for the other bit strings. When written, exactly the minimum size is sent.
  - `EntryID` (§8.1.3.5) is an octet string of exactly eight octets; any other length is refused. Its reserved value is eight zero octets.
  - `EntryTime` (§8.1.3.8) is the six-octet TimeOfDay (milliseconds and the day since 1 January 1984). The four-octet form has no day and is
    refused. The zero value is the NULL entry time (7-2 §6.2.3.11).
  - `PhyComAddr` stays refused. Its form is one of two: Layer 2 (Table 16: address, priority, VLAN, APPID) or UDP/IP (Table 40), and the
    choice follows the protocol of the multicast traffic. That traffic (GOOSE, sampled values) is outside the scope of the library. The
    namespace holds it in `NTS.dstAddress` and `GTS.dstAddress`, and the tracking objects that hold them (`UsvcbTrk`, and `GocbTrk` of the
    GOOSE class) are refused whole. For `GocbTrk` the error is `Unsupported`, because the type comes from the specific mapping, not from the
    catalogue; for `UsvcbTrk` it is `UnmappedType`.
  - The initial value of an octet string is now zero octets of its size (before: empty), so that an `EntryID` starts at its reserved value.
    An octet string of a variable size still conforms. The initial value of a `BinaryTime` is the zero time of day.
- **Sources**: 8-1 §8.1.3.5, §8.1.3.8, §8.1.3.9 (Table 37), §8.1.3.13 (Table 41), §8.1.3.15 (Table 42), §8.1.3.3 and §8.1.3.12 (Tables 16 and 40),
  7-2 §6.2.3.11 (NULL EntryTime); all in the prepared text `docs/spec/8-1`.
- **Confidence**: high for the bit positions and the sizes, which are in the tables; high for EntryID and the NULL EntryTime; medium for the
  reading of a longer bit string (D-004 policy, applied to the new fields).
- **Effect**: the tracking objects of the reports, logs and setting groups (`BrcbTrk`, `UrcbTrk`, `LocbTrk`, `SgcbTrk`) are served. The GOOSE and
  sampled value tracking objects stay refused, because they hold a `PhyComAddr`.
- **Tests**: `acsi.rs`, the bit positions of each table, the reading of longer and shorter strings, the length of `EntryID`, the six-octet
  `EntryTime`; `model/types.rs`, the resolution of each name; `tests/model.rs`, the refusal of `UsvcbTrk.dstAddress` and the served `BrcbTrk`
  with its entry fields at zero.

### D-044 — General interrogation and reservation of the buffered block (task Q4; BRCB-004)

- **Sources**: 7-2 §17.2.2.13 (`GI`), §17.2.2.17 (`ResvTms`), §17.2.2.18 (`Owner`), §17.2.3.2.3.4 (the GI service), and figure 30 (the
  logical state machine of the general interrogation). The figure is missing from the prepared text (`docs/spec/7-2`, marked "transcribe
  manually"); it was read from page 110 of the PDF (printed page 106), as D-024 did for the unbuffered block.
- **General interrogation** (figure 30, 17.2.3.2.3.4):
  - refused while the block is disabled (initial state NO GI ALLOWED): a negative response, nothing sent;
  - while enabled, with `TrgOps.general-interrogation` on: the open buffer-time window is closed first, so that the buffered entries are
    sent before the interrogation (the text says so); then one report of every member, with the reason general-interrogation;
  - with the trigger off: a positive response, nothing sent;
  - `GI` is not kept as a state: it is FALSE at once, as the figure has it.
  - The report is an **entry of the history**, with its own `EntryID` and the next sequence number. The first reading (a report outside the
    history, with the NULL identifier) was wrong: the reference peer keeps the general interrogation as an entry, with an identifier after
    its last entry (bench row Q4, `docs/interop.md`, matrix C4).
  - Not done: the restart of an interrogation that is being sent in segments (the text says a new request stops the remaining segments).
- **Reservation** (`ResvTms`, 17.2.2.17), `Write::ReserveTime(seconds)`:
  - a free block is reserved for the writer; the holder changes its time; zero releases it; zero on a free block is a positive answer;
  - another client is refused (`Reserved`, mapped to temporarily-unavailable, the same as the unbuffered block, D-024) for every write while
    the block is reserved: enabling, configuration, `PurgeBuf`, `EntryID`, `GI`, and a reservation;
  - when the association of the holder is lost, the reservation lasts the time the holder asked for (a timer, `Timer::Reservation`), and
    then the block is free and `ResvTms` reads zero (17.2.2.17, the positive value). A holder that asked for no time has no reservation.
  - A negative value is refused (`OBJECT_VALUE_INVALID`). The preconfigured value -1 (SCL `ClientLN`) is not supported: the SCL build still
    refuses the report control, SCOPE-002.
- **Enabling a free block does not need a prior reservation**: the text has the client reserve before configuring or enabling
  (17.2.2.17). The unbuffered block refuses an enabling without its reservation (D-024). The buffered block does not, because the server
  shall only keep other clients out while a block is reserved, and because the interoperability tests with the reference peer enable
  without a reservation (B2). This asymmetry is deliberate; confidence medium.
- **Owner**: the client that enables the block is its owner. Only the loss of the owner disables the block; before this task any loss did.
  The holder of a reservation gets its loss even when it is not the owner, so that its reservation can run. `Owner` is still not served
  (an empty value), since its value is an identifier that the SCSM defines (17.2.2.18).
- **Segmentation**: the general-interrogation report goes through the same segmentation as the other reports (the server splits it).
- **Tests**: `tests/report_buffered_reservation.rs` (10 tests with controlled time: the refusal while disabled, the report of every member,
  the window closed first with the next sequence number, the trigger off, the exclusivity of a reservation, its release, the zero on a free
  block, the reservation of a lost holder and its timer, a loss that changes nothing, and the loss of the owner), and two association tests in
  `tests/buffered.rs` (a general interrogation to the owner; a reservation that refuses the enabling of another association).

### D-045 — The log (`LOG`) and its control block as core state machines (task Q5; LOG-001)

- **Sources**: 7-2 §17.3.1.1 (logging is independent of the associations), §17.3.1.2 (the log buffer: the oldest entry is overwritten, the
  identifiers keep increasing), §17.3.2 and Table 78 (the LCB attributes), §17.3.4 and Table 82 (the LOG and its `EntryData` choice), §17.3.5
  (the reason codes of the log entries), §17.3.6.2 to §17.3.6.4 (`QueryLogByTime`, `QueryLogAfter`, `GetLogStatusValues`); 8-1 §17.3.3.4 and
  Table 77 (the mapping of the entries to MMS journals). All in the prepared texts (`docs/spec/7-2`, `docs/spec/8-1`). Figure 36 (the overview)
  is missing from the prepared text; it is not needed for the state machines, which the clauses describe in full.
- **The log**: a bounded first-in first-out buffer. An entry has an identifier (a counter that is not reused) and its time of entry (the
  time it was added). The content is either the members that a control block logged, with their reasons, or the transition of `LogEna` of a
  control block (the second choice of `EntryData`, §17.3.4).
- **Queries**: `query_by_time` returns the entries whose time of entry lies in the range, both ends included, an absent end being open
  (§17.3.6.2). `query_after` takes the time and the identifier of an entry, and returns the entries after it, never that entry. The pair must
  match a held entry; otherwise the query is refused (`UnknownEntry`). The text of Table 85 says the pair identifies the selected entry, which
  is the reading taken; confidence medium, to confront with the MMS mapping of `QueryLogAfter` (8-1 §17.3.4.6, Table 81).
- **Status**: the oldest and the newest time and identifier; an empty log gives zero times and the NULL identifiers (§17.3.6.4).
- **The control block**: `LogEna` TRUE needs a log (`LogRef`) and a data set that exists; the transition is recorded, and it is recorded
  again at FALSE. While enabled, every configuration attribute is refused (§17.3.2). The general interrogation is refused as a trigger of
  logging ("shall not be supported", §17.3.2). A change of a monitored member is recorded at once, with its reason (§17.3.5.2); the integrity
  period records every member with the reason integrity (§17.3.5.3). The buffer time is not supported in this release. The application
  trigger (§17.3.5.4) is not in the core: it is a write by the application, to come with the server.
- **Associations**: the control block is not affected by the loss of an association (§17.3.1.1); the entries made meanwhile are kept.
- **Values**: as for the buffered reports, the core does not hold the values. The caller reads the members when it appends a record, which
  is how the server will keep the values of each entry.
- **Not done (the MMS part)**: `ReadJournal`, the journal entries (EntryContent, the `event` and `listofVariable` elements of 8-1 Table 155),
  and the MMS names of the LCB and LOG. The MMS layer (`igs-iso9506-mms`) has no journal service: only the generated codec types exist. This
  needs a phase N in that repository, which is a decision for the owner (see the ai-log of Q5).
- **Tests**: `tests/logs.rs` (10 tests with controlled time): the bounded buffer and the identifiers, the queries by time and after an
  entry, the status, the recording while enabled only, the transitions of LogEna, the refusal of configuration while enabled, the refusal
  of the general interrogation, the requirements for enabling, and the integrity period.

### D-046 — Classes with setting-group variants: SP, or SG and SE with the device's setting groups (task Q2)

- **Sources**: IEC 61850-7-3 §7.6.1 (the settings are split into the variants SP, SG and SE: SP for an individual setting, SG for a setting
  in a setting group, SE for the value edited in the group selected for editing), the variant classes of the catalogue (`SPG`, `ING`, `ASG`,
  `CURVE`, `ENG`... each with SP, SG and SE); IEC 61850-6 (the `SettingControl` of the LN0 declares the setting groups, `numOfSGs` and
  `actSG`, §8.6). The 7-3 text was extracted from the PDF of the standard (`docs/standard/IEC61850/`), as the prepared texts do not hold it.
- **Rule**: in a device without a `SettingControl`, a setting takes the variant SP. In a device with one, it takes SG (its active value) and SE
  (its edited value): the attributes of SE with the functional constraint `SE` are added to those of SG, and the description and the extension
  are not repeated. The variant depends on the device, not on the instance of the data object.
- **Declaration**: the `SettingControl` is kept in the configuration (`Configuration.setting_groups`): the device, `numOfSGs` (at least one,
  default one) and `actSG` (from one to `numOfSGs`, default one). An out-of-range value is refused (`SCL-003`). Before this change it was
  refused as not built (SCOPE-002), which lost it.
- **Not done (O3)**: the services of the setting group control block (SelectActiveSG, SelectEditSG, SetEditSGValue, ConfirmEditSGValues,
  GetSGCBValues) are declared services, so they are refused (SCOPE-002); the values of SG and SE are read, and their writes are refused
  (SE is not writable without the services).
- **Tests**: `model.rs`, `a_setting_object_takes_the_variant_of_its_device` (SP without groups; SG and SE with them, no repeated attribute);
  `scl_build.rs`, a device with a `SettingControl` builds with `LLN0$SG$MltLev$setVal` and `LLN0$SE$MltLev$setVal`, its declaration is kept,
  and `actSG` three of two groups is refused.
- **Confidence**: high for the variant rule (read from 7-3 §7.6.1) and the shape of the attributes (the catalogue); medium for the reading
  that the SG and SE attributes of one setting form one data object, which the flattened names of 8-1 §7.3 suggest.

### D-047 — Arrays, and the enumerations chosen by the instance (task Q3)

- **Arrays**: an attribute `is_array` is one MMS variable of the array type (8-1 §8.1.2.7: the IEC 61850-7-2 array maps to an MMS array
  for each functional constraint). The number of elements is the value of its size attribute (`size_attribute`, here `maxPts`) in the
  instance: the integer `Val` of the `DAI` of the data object (`DOI`), read by the SCL build (`integer_values`). An array whose size is not
  given is refused (`Unsupported`), and so is a size above 4096 elements (`MAX_ARRAY_ELEMENTS`). The elements have the type of the attribute
  (a basic type or a structure such as `Point`); the initial value holds that many zero elements.
  - The write of a whole array is checked against its number of elements by the MMS layer (`vmd/conform.rs`, `array_conforms`): a wrong
    count is refused, which the test `an_array_is_updated_with_its_number_of_elements_and_a_wrong_number_is_refused` shows.
  - Not done: the access to one element (the alternate access, 8-1 §8.1.3 and §24.2.2.4, D-009) is refused.
  - Scope of the catalogue: six attributes are arrays (`hstVal`, `hstRangeC` of the history classes HST and HMV, `crvPts` of the curve
    settings CSG and CSD, which the case tested covers with `PTTR.TmTmpChr`).
- **Enumerations chosen by the instance**: the catalogue gives a name to every enumerated attribute of the classes that have one: the
  attribute's own type, or the `underlying_type` of its data object. This was checked for every data object of the classes ENC, ENG and ENS
  (93 data objects, none without a name). The refusal `Unsupported` is kept for an attribute without a name, which the catalogue does not
  have. A mechanism that takes the values from the SCL `EnumType` was written and removed: nothing in the standard namespace needs it, and
  code that is not reached is not kept.
- **Tests**: `model.rs`, `an_array_has_the_number_of_elements_its_instance_gives` (refused without the size; three elements with it, of the
  point type; the initial value holds three zero elements); `updates.rs`, `an_array_is_updated_with_its_number_of_elements_and_a_wrong_number_is_refused`
  (three points applied, two refused, through the served model).
- **Confidence**: high for the size rule (7-2 §6.2.5.5 and the 8-1 mapping), medium for the reading of the size attribute as an instance value
  (the SCL `DAI`), which the catalogue names but does not give.

### D-050 — The setting group control block: the state and the rules of the services (task O3)

- **Sources**: IEC 61850-7-2 §16.2 (the SGCB: `NumOfSG`, `ActSG`, `EditSG`, `CnfEdit`, `LActTm`, `ResvTms`, the default of `EditSG` at power-up
  is zero) and §16.3 (SelectActiveSG, SelectEditSG with its reservation, SetEditSGValue, ConfirmEditSGValues, GetEditSGValue,
  GetSGCBValues); IEC 61850-8-1 §16.1 (the SGCB is a named component of LLN0 with FC SP, table 71) and §16.2 (each service is a
  specialisation of an MMS read or write of those attributes). The 7-2 text is in the prepared texts; the figure of the settings model
  (figure 24) is missing and not needed for the rules.
- **No new service PDU**: the setting group services are writes and reads of the SGCB attributes and of the SG and SE attributes, so the
  MMS layer of `igs-iso9506-mms` needs no change for them.
- **State** (`settings.rs`, `Sgcb`): `NumOfSG` (at least one), `ActSG` (1 to `NumOfSG`), `EditSG` (0 to `NumOfSG`, zero at power-up),
  `CnfEdit`, `LActTm`, and the holder of the editing with the time of its last SelectEditSG.
- **Rules**:
  - SelectActiveSG: a group from 1 to `NumOfSG`; it dates `LActTm`. A refused value changes nothing.
  - SelectEditSG: zero or a group from 1 to `NumOfSG`; a non-zero group reserves the editing for the client when it is free or its own;
    zero cancels the editing of this client. Another client is refused (`Reserved`).
  - SetEditSGValue: allowed only for the holder, with a group selected (`NotReserved` otherwise, `NothingToConfirm` when `EditSG` is zero).
  - ConfirmEditSGValues: only the holder, with a group selected. It overwrites that group, releases the editing, sets `EditSG` back to zero,
    dates `LActTm` when the active group is confirmed, and sets `CnfEdit` until the storage is done (`storage_done`).
  - The reservation expires `RESERVATION` (60 s) after the last SelectEditSG without a confirmation; the holder's association loss releases
    it (§16.3.3). `ResvTms` reads its value while the editing is held, zero when it is free.
- **Decisions where the text is open**: the duration of the reservation is a local choice, since the SCL file does not give it in this
  release; `EditSG` returns to zero after a confirmation (the text does not say what it holds afterwards).
- **Not done (the served part)**: the SGCB as a named component of LLN0 in the served model (its reads with GetSGCBValues, its writes as
  SelectActiveSG, SelectEditSG and ConfirmEditSGValues); the values of the groups (the active values read from FC SG, the edited values
  written with FC SE, the storage of each group); the Owner of the reservation. Until then the services stay refused (SCOPE-002).
- **Tests**: `tests/settings.rs` (nine, with controlled time): the initial state, the active selection and its date, the editing reservation
  and its cancellation, the holder's writes, the confirmation (the overwrite, the release, the re-activation), a confirmation of another
  group, the refusals of a confirmation, the expiry of the reservation, the loss of the holder.
- **Confidence**: high for the state and the services' conditions (the text of 7-2 §16.3); medium for the duration and for `EditSG` after
  a confirmation.

## Third-party code and sources

| Component | Licence | Use | Notes |
| --- | --- | --- | --- |
| `igs-iso9506-mms` crates | Apache-2.0 | MMS stack | Same maintainers; added with the first task that needs it |
| roxmltree 0.21 | MIT OR Apache-2.0 | XML reading in `xtask` (task F3) | No dependency; DTDs refused |
| IEC code components (NSD 2007B6, SNSD 2003A4) | IEC Code Components EULA | Generated catalogue (task F3) | See NOTICE and D-002; files not in this repository |
| libiec61850 | GPL-3.0 | Reference peer of the bench, tag `v1.6.2.1`, commit `c28c27c`, black box only; image built locally, not distributed | Source never read; user documentation only |
| Wireshark (tshark) in `nicolaka/netshoot` | GPL-2.0 | Independent decoder of the bench captures | Run as a tool, not linked |
| proptest 1 | MIT OR Apache-2.0 | Development dependency of the core crate: property tests of the names and references (task A1) | Not published with the library |

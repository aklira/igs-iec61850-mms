# Capabilities and known gaps

This file lists what the library does and what it does not do yet, for the matrices of SCOPE-001 to SCOPE-003. The
matrices themselves are generated from one source (AT-14); until that exists, each row names the decision that
settles it. A gap is an item that is refused or reported explicitly, never served with a guess.

## Server model (task A3)

| Item | State | Decision |
| --- | --- | --- |
| Logical devices as MMS domains; logical nodes as named variables with flattened names (`LN$FC$DO$DA`) | Served | D-006 |
| Names up to 64 characters, by the IEC 61850 profile | Served (the VMD holds 64 once its limit is set) | D-006, igs-iso9506-mms D-037 |
| Flattened names longer than 64 characters | Not listed; counted in the layout; reachable by alternate access | D-006 |
| Functional-constraint level (`LN$FC`) as a variable | Not a variable | D-006 |
| Mandatory data objects and attributes checked against the namespace | Served (refused when missing) | D-007 |
| Multiple instances of a data object (`Ind1`, `AnIn1`, ...) | Served | D-007 |
| Presence conditions other than `M` (`MOcond`, `MFsubst`, `AtLeastOne`, the groups) | Not evaluated: the member is instantiated when the application names it | D-007 |
| Optional sub data objects | Not instantiated (only the mandatory ones are) | D-005 |
| Arrays (`is_array`) | Refused (`Unsupported`) | D-008 |
| `Dbpos` (double-point status, DPS, DPC, `XCBR.Pos`) | Served as a two-bit coded enumeration (7-2 Table 24) | D-042 |
| Basic types `Tcmd`, `TrgOps`, `OptFlds`, `SvOptFlds`, `EntryID`, `EntryTime`, `PhyComAddr` | Refused (`UnmappedType`) until their source clauses are read; an object that holds one is refused whole | D-008, D-042 |
| Enumerations chosen by the instance (no type name in the class) | Refused, unless the data object gives the name (its `underlying_type`) | D-008 |
| Classes with variants (the setting groups `SPG`, `ING`, ...) | Refused until the variant is chosen | D-008 |
| Service attributes added by 8-1 to the controllable and reporting classes (`Oper`, `SBOw`, `Cancel`, the control blocks) | Not in the model yet: control and reporting tasks | D-008 |
| Read of a served node or of a flattened name | Served, one value per node | D-010 |
| Write to a served variable | Served for `SP`, `SV`, `CF`, `DC` and `BL` attributes of non-controllable objects; refused otherwise (`object-access-denied`) | D-012 |
| Write of an enumeration | Served with the values the namespace defines; others refused (`object-value-invalid`) | D-012 |
| Writes to controllable objects (`SPC`, `DPC`, `ENC`, ...) | Refused until their control state machine (task K) | D-012, MODEL-005 |
| Directory and definition (GetDataDirectory, GetDataDefinition, logical-node directory) | Served by GetVariableAccessAttributes and GetNameList; the logical-node filter is the client's (A6) | D-013 |
| Alternate access | Refused item by item; `valt` is negotiated | D-009 |
| Application extensions (the `EX` constraint, extension classes) | Not accepted | D-007 |

## Time and application updates (task A5)

| Item | State | Decision |
| --- | --- | --- |
| Timestamps with their quality (leap-second knowledge, clock failure, not synchronised, accuracy 0 to 24 or unspecified) | Served; refused above 24 bits at the clock | D-014 |
| Host clock with a declared quality (default: not synchronised) | Served | D-014 |
| Embedded SNTP client (TIME-002) | Not implemented; the host synchronisation is declared instead | D-014 |
| Accuracy from the root dispersion (TIME-003) | Not applicable without SNTP | D-014 |
| PTP (IEC/IEEE 61850-9-3) | Not implemented | D-014 |
| Application updates of values, qualities and timestamps | Served, per logical node, all or nothing, with a revision | D-015 |
| Snapshot of several items of one Read | Not one state: each item at its own moment | D-015, MODEL-006 |
| Consistent view of several nodes | Through data sets and reports (tasks D and B), not yet | D-015 |

## Client discovery and data access (task A6)

| Item | State | Decision |
| --- | --- | --- |
| Logical devices (domains), logical nodes, named variables, paged with `continueAfter`, checked page by page | Served | D-016 |
| Functional groups and data objects of a node, from its type description | Served | D-016 |
| Definitions rebuilt from the type descriptions; unknown types kept generically; malformed descriptions refused | Served | D-016, MODEL-002 |
| Value check against a definition (`DataDefinition::accepts`) | Served | D-016 |
| Reads and writes with one result per item, in order | Served | D-016, ACSI-002 |
| Error of a written item `temporarily-unavailable` | Mapped to `AccessNotAllowedInCurrentState` (Table 27 is ambiguous) | D-016 |
| Arrays (client definitions) | Defined; the served model has no array yet | D-016 |
| Typed data sets and control blocks in the client | Not yet (tasks D, B and K) | ACSI-001 |

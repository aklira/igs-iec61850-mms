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
| Basic types `Dbpos`, `Tcmd`, `TrgOps`, `OptFlds`, `SvOptFlds`, `EntryID`, `EntryTime`, `PhyComAddr` | Refused (`UnmappedType`) until their source clauses are read | D-008 |
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

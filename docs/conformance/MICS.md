# Model implementation conformance statement (draft)

Draft of the MICS of `igs-iec61850-mms`: the logical devices, logical nodes, common data classes and models that the library implements, with
the ones it does not. Each row names its evidence (`path::function`, a decision `D-nnn`, or a file), and `cargo xtask statements --check`
verifies it. A draft, not a signed statement (`IMPLEMENTATION.md`, X7 and X11).

| Item | Status | Note | Evidence |
| --- | --- | --- | --- |
| Logical device as an MMS domain | Supported | | crates/igs-iec61850-mms/tests/served_model.rs::get_name_list_lists_the_node_and_its_flattened_names |
| Logical node classes of the catalogue (IEC 61850-7-4, latest releases) | Supported | names, data objects and their order, from the NSD files | crates/igs-iec61850-mms-core/tests/catalogue.rs::counts_match_the_nsd_files |
| Logical nodes instantiated from an SCL file | Supported | the IED of the file, its nodes and its data sets | crates/igs-iec61850-mms/tests/scl_discovery.rs::the_sample_configuration_is_discovered_by_the_client |
| Common data classes with a status (SPS, DPS, ...) | Supported | the double point is a two-bit coded enumeration | crates/igs-iec61850-mms-core/tests/model.rs::the_position_of_a_breaker_is_a_two_bit_double_point |
| Common data classes with a variant (SP, SG, SE) | Supported | the variant of the device | crates/igs-iec61850-mms-core/tests/model.rs::a_setting_object_takes_the_variant_of_its_device |
| Controllable common data classes | Supported | control models of 7-2 §20 | crates/igs-iec61850-mms-core/tests/control.rs::a_direct_operation_with_normal_security_is_checked_twice_and_executed |
| Arrays with the number of elements of the instance | Supported | `maxPts` from the SCL | crates/igs-iec61850-mms-core/tests/model.rs::an_array_has_the_number_of_elements_its_instance_gives |
| Enumerations named by the data object | Supported | every standard enumeration has a name (D-047) | crates/igs-iec61850-mms-core/tests/access.rs::an_enumeration_holds_the_values_its_namespace_defines |
| Data sets, static and dynamic | Supported | | crates/igs-iec61850-mms/tests/data_sets.rs::the_data_sets_of_the_device_are_listed |
| Unbuffered report control blocks | Supported | | crates/igs-iec61850-mms/tests/reporting.rs::a_configured_block_reports_the_change_to_its_owner_in_the_layout_of_the_standard |
| Buffered report control blocks | Supported | | crates/igs-iec61850-mms/tests/buffered.rs::a_general_interrogation_reports_every_member_to_the_association_that_enabled_the_block |
| Log control blocks | Core only | the block records into its journal; its MMS services are open (X2) | crates/igs-iec61850-mms-core/tests/logs.rs::each_transition_of_log_ena_is_recorded |
| Journals of the logs | Supported | each Log of an SCL file is an MMS journal of its node | crates/igs-iec61850-mms/tests/logs.rs::the_journals_of_a_domain_are_listed_by_get_name_list |
| Setting group control blocks | Supported | the SGCB of each device with a SettingControl | crates/igs-iec61850-mms/tests/setting_groups.rs::the_sgcb_is_read_as_a_structure_of_its_attributes |
| File services | Supported | the store of the server (D-054) | crates/igs-iec61850-mms/tests/files.rs::a_stored_file_is_listed_by_file_directory_with_its_parts |
| Presence conditions other than M | Not evaluated | the member is instantiated when named (D-007) | D-007 |
| Optional sub data objects | Not instantiated | only the mandatory ones (D-005) | D-005 |
| Application extensions (EX constraint) | Not accepted | (D-007) | D-007 |
| GOOSE and sampled value control blocks | Not served | the destination address is refused (D-043) | D-043 |
| Edition detection of the device | Supported | the first and the second edition are recognised | crates/igs-iec61850-mms/tests/edition_detect.rs::the_namespace_of_the_first_and_of_the_second_edition_are_recognised |
| Edition 1 interoperability with a peer | Not tested | no Edition 1 peer (E3 blocked) | D-020 |

# ACSI conformance statement (draft)

Draft of the ACSI conformance statement of `igs-iec61850-mms` (IEC 61850-7-2, service tables; IEC 61850-8-1 for the mapping). It lists
the services of the products (`SPECIFICATIONS.md`); a service that is not in the table is not declared. Each row names its evidence: a test
(`path::function`), a decision of `PROVENANCE.md` (`D-nnn`), or a file. `cargo xtask statements --check` verifies every piece of evidence.
This is a draft: it is not a signed statement, and the maintainer approves the final one (`IMPLEMENTATION.md`, X11).

| Service | Status | Note | Evidence |
| --- | --- | --- | --- |
| GetServerDirectory | Supported | logical devices of the server | crates/igs-iec61850-mms/tests/client_discovery.rs::the_logical_devices_and_nodes_are_listed |
| LogicalDeviceDirectory | Supported | logical nodes of a device | crates/igs-iec61850-mms/tests/client_discovery.rs::the_logical_devices_and_nodes_are_listed |
| LogicalNodeDirectory | Supported | the named variables of a node, paged | crates/igs-iec61850-mms/tests/client_discovery.rs::the_named_variables_are_the_listing_of_every_page |
| GetDataValues | Supported | one result per item, in order | crates/igs-iec61850-mms/tests/client_discovery.rs::a_read_answers_one_result_per_item_in_the_order_of_the_items |
| SetDataValues | Supported | SP, SV, CF, DC and BL of objects that are not controllable; other writes refused | crates/igs-iec61850-mms/tests/served_writes.rs::each_item_of_a_write_has_its_own_result |
| GetDataDirectory | Supported | the attributes of a data object, with their types | crates/igs-iec61850-mms/tests/served_model.rs::get_variable_access_attributes_gives_the_type_of_each_level |
| GetDataDefinition | Supported | the definition of a data object | crates/igs-iec61850-mms/tests/client_discovery.rs::a_data_object_is_defined_by_its_attributes_and_accepts_its_values |
| GetDataSetValues | Supported | the members in their order | crates/igs-iec61850-mms/tests/data_sets.rs::the_data_set_reads_its_values_in_the_order_of_its_members |
| SetDataSetValues | Supported | each member keeps its access rule; the request is not atomic (D-024) | crates/igs-iec61850-mms/tests/data_sets.rs::a_write_to_a_read_only_member_fails_that_item |
| CreateDataSet | Supported | dynamic data sets, private to their association | crates/igs-iec61850-mms/tests/datasets.rs::an_association_specific_data_set_is_private_and_dropped_when_its_association_ends |
| DeleteDataSet | Supported | a static data set is not deleted by a client | crates/igs-iec61850-mms/tests/datasets.rs::a_persistent_data_set_is_deleted_by_another_association |
| GetDataSetDirectory | Supported | the data sets of the device | crates/igs-iec61850-mms/tests/data_sets.rs::the_data_sets_of_the_device_are_listed |
| GetBRCBValues, SetBRCBValues | Supported | reservation and writes by the holder | crates/igs-iec61850-mms/tests/buffered.rs::a_reservation_by_one_association_refuses_the_enabling_of_another |
| GetURCBValues, SetURCBValues | Supported | writes by the holder only | crates/igs-iec61850-mms/tests/reporting.rs::a_write_by_a_client_that_does_not_hold_the_block_is_refused |
| Report (buffered) | Supported | entries, general interrogation, resynchronisation | crates/igs-iec61850-mms/tests/buffered.rs::a_buffered_block_reports_its_changes_to_the_association_that_enabled_it |
| Report (unbuffered) | Supported | distinct subscriptions receive their own reports | crates/igs-iec61850-mms/tests/reporting.rs::two_clients_with_distinct_subscriptions_receive_only_their_own_reports |
| GetLCBValues, SetLCBValues | Not served on MMS | the log control block is a core state machine; its MMS services are open (X2) | crates/igs-iec61850-mms-core/tests/logs.rs::a_change_is_recorded_only_while_enabled_and_with_its_reason |
| QueryLogByTime, QueryLogAfter | Supported as ReadJournal on the journal of the log (the ranges are the ones of the MMS journal, D-056) | the entries of the journal are read; the ranges belong to the sibling's ReadJournal | crates/igs-iec61850-mms/tests/logs.rs::the_changes_of_the_data_set_are_read_from_the_journal_with_their_reason, D-056 |
| GetLogStatusValues | Not served on MMS | the status is in the core; the LCB over MMS is open (X2) | crates/igs-iec61850-mms-core/tests/logs.rs::the_status_of_an_empty_log_is_zero_and_the_status_of_a_filled_one_gives_its_ends |
| Select, SelectWithValue | Supported | normal and enhanced security | crates/igs-iec61850-mms/tests/controls_client.rs::a_select_before_operate_object_is_selected_and_operated_by_the_typed_api |
| Operate (direct and select before operate) | Supported | the checks of the application are answered | crates/igs-iec61850-mms-core/tests/control.rs::a_direct_operation_with_normal_security_is_checked_twice_and_executed |
| Cancel | Supported | releases the selection | crates/igs-iec61850-mms/tests/controls_client.rs::a_cancel_frees_the_selection_for_another_client |
| TimeActivatedOperate | Not supported | declared unsupported in the mapping of the controls | D-026 |
| CommandTermination | Supported | positive and negative outcomes | crates/igs-iec61850-mms/tests/controls_client.rs::a_deferred_positive_outcome_is_decoded_as_a_positive_termination |
| GetFile | Supported | content read through FileOpen, FileRead and FileClose | crates/igs-iec61850-mms/tests/files.rs::get_file_reads_the_content_of_a_stored_file |
| SetFile | Not supported | refused by the negotiation | D-054 |
| DeleteFile | Supported | a deleted file is no longer read | crates/igs-iec61850-mms/tests/files.rs::a_deleted_file_is_no_longer_read |
| GetFileAttributeValues | Supported | FileDirectory with the parts of the names | crates/igs-iec61850-mms/tests/files.rs::a_stored_file_is_listed_by_file_directory_with_its_parts |
| GetSGCBValues | Supported | the structure of the six attributes | crates/igs-iec61850-mms/tests/setting_groups.rs::the_sgcb_is_read_as_a_structure_of_its_attributes |
| SelectActiveSG | Supported | writes ActSG | crates/igs-iec61850-mms/tests/setting_groups.rs::the_sgcb_and_its_attributes_are_named_in_the_domain_with_their_structure |
| SelectEditSG, SetEditSGValue, ConfirmEditSGValues | Supported | one holder of the editing; release on loss | crates/igs-iec61850-mms/tests/setting_groups.rs::the_editing_is_held_by_one_association_and_its_values_are_confirmed_into_the_group |
| GetEditSGValue | Supported | reads of the values with FC SE | crates/igs-iec61850-mms/tests/setting_groups.rs::the_editing_is_held_by_one_association_and_its_values_are_confirmed_into_the_group |
| TimeStamp quality | Supported | leap second knowledge, clock failure, not synchronised, accuracy | crates/igs-iec61850-mms/tests/clock.rs::a_declared_synchronisation_reports_its_accuracy_and_leap_second_knowledge |
| SNTP client | Supported | mode 3 requests, the checks of each reply | crates/igs-iec61850-mms/tests/sntp.rs::a_reply_to_the_request_synchronises_the_clock_with_its_accuracy |
| Leap second announced | Supported | the midnight under an announced insertion is not known (7-2 Annex F.3); a deletion is not signalled | crates/igs-iec61850-mms-core/tests/sntp.rs::a_midnight_stamp_under_an_announced_leap_second_is_marked_as_the_leap_second |
| Secured transport | Supported with the inherited gaps G17 to G19 | mutual authentication and revocation of IEC 62351-3 | crates/igs-iec61850-mms/tests/secured.rs::a_mutually_authenticated_client_reads_the_model_over_tls |

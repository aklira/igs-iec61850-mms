// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The file services of the server (task X3; IEC 61850-8-1 clause 23): the files of the store are listed, read and deleted over MMS,
//! and SetFile (ObtainFile) is not a declared service of the profile (D-054).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::time::Duration;

use igs_iec61850_mms::demo::demo_model;
use igs_iec61850_mms::model::{client_config, profile, server_config, ServedModel};
use igs_iec61850_mms::server_identity;
use igs_iso9506_mms::server::Server;
use igs_iso9506_mms::{Client, Error};
use igs_iso9506_mms_codec::generated::types::{
    FileDeleteRequest, FileDirectoryRequest, FileName, ServiceError, ServiceErrorErrorClass,
    ServiceErrorErrorClassFile,
};
use igs_iso9506_mms_codec::mms::profile::Side;
use igs_iso9506_mms_link::config::LinkConfig;

const TIMEOUT: Duration = Duration::from_secs(10);
const REPORT: &[u8] = b"disturbance record: a line of text\n";

/// A server of the demonstration model, whose file store holds one file, and a client connected to it.
async fn start() -> (Server, Client) {
    let mut served = ServedModel::new(&demo_model().unwrap(), server_identity()).unwrap();
    served.add_file(&["records", "fault.txt"], REPORT.to_vec());
    let server = Server::bind_model("127.0.0.1:0", server_config(LinkConfig::default()), served)
        .await
        .expect("the server binds");
    let address = server.local_address().to_string();
    let (client, _events) = tokio::time::timeout(
        TIMEOUT,
        Client::connect(&address, client_config(LinkConfig::default())),
    )
    .await
    .expect("the association is in time")
    .expect("the association is accepted");
    (server, client)
}

fn name(parts: &[&str]) -> FileName {
    FileName(parts.iter().map(|part| (*part).to_string()).collect())
}

/// The file error class of a refused file service, when the error is one.
fn file_error(error: Error) -> Option<ServiceErrorErrorClassFile> {
    match error {
        Error::Service(ServiceError {
            error_class: ServiceErrorErrorClass::File(class),
            ..
        }) => Some(class),
        _ => None,
    }
}

#[tokio::test]
async fn a_stored_file_is_listed_by_file_directory_with_its_parts() {
    let (server, client) = start().await;
    let page = client
        .file_directory(FileDirectoryRequest {
            file_specification: None,
            continue_after: None,
        })
        .await
        .expect("the directory is given");
    let names: Vec<Vec<String>> = page
        .list_of_directory_entry
        .iter()
        .map(|entry| entry.file_name.0.clone())
        .collect();
    assert_eq!(
        names,
        vec![vec!["records".to_string(), "fault.txt".to_string()]]
    );
    client.close().await.unwrap();
    server.shutdown().await;
}

#[tokio::test]
async fn get_file_reads_the_content_of_a_stored_file() {
    let (server, client) = start().await;
    let content = tokio::time::timeout(TIMEOUT, client.get_file(&["records", "fault.txt"]))
        .await
        .unwrap()
        .expect("the file is read");
    assert_eq!(content, REPORT);
    client.close().await.unwrap();
    server.shutdown().await;
}

#[tokio::test]
async fn a_file_that_is_not_stored_is_refused_as_non_existent() {
    let (server, client) = start().await;
    let error = tokio::time::timeout(TIMEOUT, client.get_file(&["records", "missing.txt"]))
        .await
        .unwrap()
        .expect_err("the file does not exist");
    // FILE-NON-EXISTENT: class 7 of the file errors of ISO 9506-2.
    assert_eq!(
        file_error(error),
        Some(ServiceErrorErrorClassFile::FILE_NON_EXISTENT)
    );
    client.close().await.unwrap();
    server.shutdown().await;
}

#[tokio::test]
async fn a_deleted_file_is_no_longer_read() {
    let (server, client) = start().await;
    tokio::time::timeout(
        TIMEOUT,
        client.file_delete(FileDeleteRequest(name(&["records", "fault.txt"]))),
    )
    .await
    .unwrap()
    .expect("the file is deleted");
    let error = tokio::time::timeout(TIMEOUT, client.get_file(&["records", "fault.txt"]))
        .await
        .unwrap()
        .expect_err("the deleted file is not read");
    assert_eq!(
        file_error(error),
        Some(ServiceErrorErrorClassFile::FILE_NON_EXISTENT)
    );
    client.close().await.unwrap();
    server.shutdown().await;
}

#[test]
fn set_file_is_not_a_declared_service_of_the_server() {
    // The profile declares the file services of 8-1 clause 23 for the server, and not ObtainFile (SetFile): the codec has no
    // ObtainFile request, so a peer's ObtainFile is refused by the negotiation (X3, D-054).
    let required = profile().required_services(Side::Server);
    for service in ["fileOpen", "fileRead", "fileClose", "fileDirectory"] {
        assert!(
            required.contains(&service),
            "{service} is a declared file service"
        );
    }
    assert!(
        !required.contains(&"obtainFile"),
        "SetFile is refused, not declared"
    );
}

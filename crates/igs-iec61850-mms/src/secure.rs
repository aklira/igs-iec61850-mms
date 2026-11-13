// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The secured profile of the IEC 61850 station (task O4; IEC 62351-3): the TLS of `igs-iso9506-mms` built from PEM material, and
//! the server configuration that carries it. The station authenticates the peer (mutual authentication) against its trust anchors, and
//! refuses a certificate on the revocation list. IEC 62351-4 and IEC 62351-6 are not held (SEC-001, SEC-002): this module secures the
//! transport only.

use igs_iso9506_mms::tls::{
    revocation_from_pem, Identity as TlsIdentity, RustlsBackend, Secure, TlsError, TlsSettings,
    TrustAnchors,
};
use igs_iso9506_mms::ServerConfig;
use igs_iso9506_mms_link::config::LinkConfig;

use crate::model::server_config;

/// The TLS station of this end, from PEM material: its certificate and key (none for a client that only verifies the server), the trust
/// anchors of the peers, and the revocation list (none when there is no list). The backend is the one of `igs-iso9506-mms` (TLS 1.2 and
/// TLS 1.3, ring provider; decision D-030 of that repository, and its gaps G17 to G19).
pub fn secure_station(
    identity: Option<(&[u8], &[u8])>,
    trust_anchors: &[u8],
    revocation_list: Option<&[u8]>,
) -> Result<Secure, TlsError> {
    let identity = match identity {
        Some((certificate, key)) => Some(TlsIdentity::from_pem(certificate, key)?),
        None => None,
    };
    let mut settings = TlsSettings::new(identity, TrustAnchors::from_pem(trust_anchors)?);
    if let Some(list) = revocation_list {
        settings = settings.with_revocation(revocation_from_pem(list)?);
    }
    Ok(Secure::new(RustlsBackend::new(&settings)?))
}

/// The configuration of a server of the IEC 61850 profile that serves its model over TLS.
pub fn secured_server_config(link: LinkConfig, secure: Secure) -> ServerConfig {
    server_config(link).with_tls(secure)
}

// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The embedded SNTP client on the host (TIME-002, TIME-003; task O5; D-052).
//!
//! The protocol is the sans-I/O client of `igs-iec61850-mms-core` (`sntp`): requests, the checks of the replies, the
//! offset, and the accuracy from the root distance. This module adds the UDP socket and its poll loop, and an
//! [`SntpClock`] that the application gives to the server as its [`ClockSource`] (TIME-001). Until a reply passes the
//! checks, and while the last one is older than `MAX_AGE`, the clock reports that it is not synchronised.
//!
//! The loop holds no lock across an await: each step takes the client, makes its decision, and releases it.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use igs_iec61850_mms_core::sntp::{Client, NtpTime};
use igs_iec61850_mms_core::time::ClockSource;
use igs_iso9506_mms_codec::mms::UtcTimestamp;
use tokio::net::UdpSocket;
use tokio::time::{sleep, timeout};

/// How long the loop waits for a reply before it looks at the poll interval again.
const TICK: Duration = Duration::from_millis(250);

/// The clock of an embedded SNTP client. Its readings are the local time corrected by the offset of the last accepted
/// reply, with the quality of that reply; before a reply is accepted, and when it is too old, the readings are not
/// synchronised.
#[derive(Debug)]
pub struct SntpClock {
    /// The protocol state, shared by the poll loop and the readings.
    client: Mutex<Client>,
    /// The start of the clock: the durations of the protocol are measured from it, on the monotonic clock (RUN-003).
    started: Instant,
}

impl SntpClock {
    /// A clock that is not synchronised until a reply is accepted from the server its poll loop talks to.
    pub fn new() -> Self {
        Self {
            client: Mutex::new(Client::new()),
            started: Instant::now(),
        }
    }

    /// The protocol state. A poisoned lock is still used: the state is a plain value that the checks keep consistent.
    fn lock(&self) -> MutexGuard<'_, Client> {
        self.client.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The monotonic time since the start of the clock.
    fn monotonic(&self) -> Duration {
        self.started.elapsed()
    }

    /// Polls `server` until the socket fails. A request is sent at each poll interval, and each reply that passes the
    /// checks updates the clock; a reply that does not pass is ignored, as the protocol says. The task runs for as long as
    /// the caller keeps it, and is stopped by aborting it.
    pub async fn run(self: Arc<Self>, server: SocketAddr) -> std::io::Result<()> {
        let bind = if server.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        };
        let socket = UdpSocket::bind(bind).await?;
        socket.connect(server).await?;
        let mut buffer = [0_u8; 512];
        loop {
            let request = self.lock().poll(self.monotonic(), wall_now());
            if let Some(bytes) = request {
                if socket.send(&bytes).await.is_err() {
                    // A refused datagram (an ICMP unreachable) is not a reason to stop: the next poll tries again.
                    sleep(TICK).await;
                    continue;
                }
            }
            match timeout(TICK, socket.recv(&mut buffer)).await {
                Ok(Ok(length)) => {
                    if let Some(datagram) = buffer.get(..length) {
                        // A refused reply leaves the clock as it is: the reason is the protocol's, not the application's.
                        let _refused = self.lock().receive(self.monotonic(), wall_now(), datagram);
                    }
                }
                Ok(Err(_)) => sleep(TICK).await,
                Err(_elapsed) => {}
            }
        }
    }
}

impl Default for SntpClock {
    fn default() -> Self {
        Self::new()
    }
}

impl ClockSource for SntpClock {
    fn now(&self) -> UtcTimestamp {
        self.lock().stamp(self.monotonic(), wall_now())
    }
}

/// The local UTC time as an NTP timestamp. Before 1970 or out of range, the epoch of NTP, which the client stamps as not
/// synchronised.
fn wall_now() -> NtpTime {
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO);
    NtpTime::from_unix(since_epoch.as_secs(), since_epoch.subsec_nanos()).unwrap_or(NtpTime::ZERO)
}

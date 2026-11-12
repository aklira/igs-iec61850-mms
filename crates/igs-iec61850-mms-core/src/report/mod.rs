// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The report control blocks (IEC 61850-7-2 §17, 8-1 §17.1; tasks D3 and D4).
//!
//! The unbuffered block is a sans-I/O state machine ([`urcb`]). Its reports are laid out in MMS by [`encode`] (task D4); their
//! delivery is served by the crate `igs-iec61850-mms`.

pub mod brcb;
pub mod encode;
pub mod urcb;

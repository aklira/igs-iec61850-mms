// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The report control blocks (IEC 61850-7-2 §17, 8-1 §17.1; tasks D3 and D4).
//!
//! The unbuffered block is a sans-I/O state machine ([`urcb`]). The delivery of its reports is the task D4.

pub mod urcb;

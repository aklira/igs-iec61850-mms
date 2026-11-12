# SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
#
# SPDX-License-Identifier: Apache-2.0
"""D6, the forward rows of the reports: our client configures an unbuffered report control block of the reference example
(`server_example_basic_io`, the block `EventsRCB01` of the data set `LLN0$Events`), enables it, and receives the reports the reference
server sends: a general-interrogation report, then the periodic integrity reports. The reports are decoded by our client; the capture
must decode without an error in tshark (rule 1: the peer is a black box).

The reverse direction (the example client of the peer receives the reports of our server) needs our server to serve a configuration
read from SCL, which the demonstration model of `igs61850-server` does not have: it is a known gap, in `docs/interop.md`.
"""

from __future__ import annotations

import os
import subprocess
import time
from pathlib import Path

import pytest

from igs_bench import capture
from igs_bench.compose import Bench

IEC_REPO = Path(os.environ.get("IGS_IEC_REPO", Path(__file__).resolve().parents[2]))
PEER = "127.0.0.1:11102"
ERRORS = "_ws.malformed || _ws.expert.severity == error"
KNOWN_ARTEFACT = '_ws.expert.message == "Internal error, zero-byte MMS PDU"'
DEVICE = "simpleIOGenericIO"
NODE = "LLN0"
BLOCK = "EventsRCB01"


def _client_binary() -> Path:
    """The client of this repository, built in release mode."""
    subprocess.run(
        ["cargo", "build", "--release", "-q", "-p", "igs-iec61850-mms", "--bin", "igs61850-client"],
        cwd=IEC_REPO,
        check=True,
        timeout=900,
    )
    return IEC_REPO / "target" / "release" / "igs61850-client"


def _report(client: Path, *extra: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(client), "--server", PEER, "--lenient-parameter-cbbs", "report", DEVICE, NODE, BLOCK, *extra],
        capture_output=True,
        text=True,
        timeout=180,
    )


def _decode_errors(bench: Bench) -> list[str]:
    out = bench.exec(
        "capture", "tshark", "-r", f"/captures/{bench.capture_name}", "-Y", f"({ERRORS}) && !({KNOWN_ARTEFACT})"
    )
    return [line for line in out.stdout.splitlines() if line.strip()]


def _run_row(request, *extra: str) -> tuple[subprocess.CompletedProcess[str], list[str]]:
    capture.CAPTURE_DIR.mkdir(exist_ok=True)
    name = f"d6-{request.node.name}-{time.strftime('%Y%m%dT%H%M%S')}.pcap"
    bench = Bench(name, env={"REF_SERVER": "server_example_basic_io"})
    bench.down()
    try:
        bench.up("ref-server", "capture")
        capture.wait_live(bench)
        result = _report(_client_binary(), *extra)
        capture.stop(bench)
        errors = _decode_errors(bench)
    finally:
        bench.down()
    return result, errors


def test_a_general_interrogation_report_of_the_reference_server_is_decoded(request):
    """The general interrogation: one report of every member, with the reason general-interrogation."""
    result, errors = _run_row(request, "--gi", "--seconds", "5")
    print(result.stdout, result.stderr)
    assert errors == [], f"the capture does not decode cleanly: {errors[:5]}"
    assert result.returncode == 0, result.stdout + result.stderr
    assert "GeneralInterrogation" in result.stdout, result.stdout
    assert "report(s) received" in result.stdout, result.stdout


def test_the_integrity_reports_of_the_reference_server_are_decoded(request):
    """The integrity period: periodic reports of every member, with the reason integrity."""
    result, errors = _run_row(request, "--integrity-ms", "1000", "--seconds", "4")
    print(result.stdout, result.stderr)
    assert errors == [], f"the capture does not decode cleanly: {errors[:5]}"
    assert result.returncode == 0, result.stdout + result.stderr
    assert result.stdout.count("report Events1") >= 2, result.stdout
    assert "Integrity" in result.stdout, result.stdout

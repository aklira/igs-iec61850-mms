# SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
#
# SPDX-License-Identifier: Apache-2.0
"""Q4, the general interrogation of a buffered block against the reference peer: our client enables the block `EventsBRCB01` of
`server_example_basic_io` and asks for a general interrogation (figure 30, 17.2.3.2.3.4). The reference server answers with a report
of its members with the reason general-interrogation; the capture must decode without an error in tshark (rule 1: the peer is a
black box). The row is recorded in `docs/interop.md`.
"""

from __future__ import annotations

import os
import re
import subprocess
import time
from pathlib import Path

from igs_bench import capture
from igs_bench.compose import Bench

IEC_REPO = Path(os.environ.get("IGS_IEC_REPO", Path(__file__).resolve().parents[2]))
PEER = "127.0.0.1:11102"
ERRORS = "_ws.malformed || _ws.expert.severity == error"
KNOWN_ARTEFACT = '_ws.expert.message == "Internal error, zero-byte MMS PDU"'
DEVICE = "simpleIOGenericIO"
NODE = "LLN0"
BLOCK = "EventsBRCB01"


def _client_binary() -> Path:
    """The client of this repository, built in release mode."""
    subprocess.run(
        ["cargo", "build", "--release", "-q", "-p", "igs-iec61850-mms", "--bin", "igs61850-client"],
        cwd=IEC_REPO,
        check=True,
        timeout=900,
    )
    return IEC_REPO / "target" / "release" / "igs61850-client"


def _buffered(client: Path, *extra: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(client), "--server", PEER, "--lenient-parameter-cbbs", "buffered", DEVICE, NODE, BLOCK, *extra],
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
    name = f"b2-{request.node.name}-{time.strftime('%Y%m%dT%H%M%S')}.pcap"
    bench = Bench(name, env={"REF_SERVER": "server_example_basic_io"})
    bench.down()
    try:
        bench.up("ref-server", "capture")
        capture.wait_live(bench)
        result = _buffered(_client_binary(), *extra)
        capture.stop(bench)
        errors = _decode_errors(bench)
    finally:
        bench.down()
    return result, errors




def test_the_general_interrogation_of_the_reference_buffered_block_is_decoded(request):
    """The general interrogation requested once the block is enabled: a report whose members carry the reason general-interrogation."""
    result, errors = _run_row(request, "--general-interrogation", "--seconds", "4")
    print(result.stdout, result.stderr)
    assert errors == [], f"the capture does not decode cleanly: {errors[:5]}"
    assert result.returncode == 0, result.stdout + result.stderr
    assert "GeneralInterrogation" in result.stdout, result.stdout

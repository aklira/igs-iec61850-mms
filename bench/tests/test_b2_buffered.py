# SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
#
# SPDX-License-Identifier: Apache-2.0
"""B2, the buffered reports against the reference peer: our client configures the buffered block `EventsBRCB01` of the example
`server_example_basic_io` (the block of its data set `LLN0$Events`), enables it, and receives the buffered reports the reference server
sends for its integrity period. Each report carries an entry identifier and a sequence number; the capture must decode without an error
in tshark (rule 1: the peer is a black box). The reverse direction (the peer's client on our server) is a known gap, in `docs/interop.md`.
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


def test_the_integrity_entries_of_the_reference_buffered_block_are_decoded(request):
    """The integrity period: periodic buffered reports of the reference server, each with its entry and its sequence number."""
    result, errors = _run_row(request, "--integrity-ms", "1000", "--seconds", "4")
    print(result.stdout, result.stderr)
    assert errors == [], f"the capture does not decode cleanly: {errors[:5]}"
    assert result.returncode == 0, result.stdout + result.stderr
    entries = re.findall(r"buffered report \S+ : entry ([0-9a-f]{16}), sequence Some\((\d+)\)", result.stdout)
    assert len(entries) >= 2, result.stdout
    ids = [int(entry, 16) for entry, _ in entries]
    sequences = [int(sequence) for _, sequence in entries]
    assert ids == sorted(ids) and len(set(ids)) == len(ids), "the entries are in order, each once"
    assert sequences == list(range(sequences[0], sequences[0] + len(sequences))), "the sequence numbers follow"
    assert "Integrity" in result.stdout, result.stdout

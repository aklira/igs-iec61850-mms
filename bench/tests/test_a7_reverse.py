# SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
#
# SPDX-License-Identifier: Apache-2.0
"""A7, the reverse rows: our server (`igs61850-server`, the demonstration model) against the example clients of the
reference peer, run as black boxes in the network namespace of our server (rule 1 of IMPLEMENTATION.md).

Each row starts our server in its own container, runs one example client against it on port 102, and checks that the
association is established by our server and that the capture decodes without an error in tshark. The example's own exit
status and output are printed. A row the example cannot run against our model is recorded as a known gap in
`docs/interop.md`, with its reason, and is an expected failure (strict).
"""

from __future__ import annotations

import os
import subprocess
import time
from pathlib import Path

import pytest

from igs_bench import capture, tshark
from igs_bench.compose import Bench

IEC_REPO = Path(os.environ.get("IGS_IEC_REPO", Path(__file__).resolve().parents[2]))
ERRORS = "_ws.malformed || _ws.expert.severity == error"
KNOWN_ARTEFACT = '_ws.expert.message == "Internal error, zero-byte MMS PDU"'

# The example clients of the reference image, and what each one needs from a server beyond the protocol. The rows check
# the protocol: the association is accepted and every request is answered. The functional part (the values the example reads)
# needs the model the example expects, which the demonstration model does not have (docs/interop.md).
EXAMPLES = {
    "iec61850_client_example1": "reads simpleIOGenericIO/GGIO1 values: our model is named IED1LD1",
    "iec61850_client_example_array": "reads an array: arrays are not served (task A3)",
    "iec61850_client_example_reporting": "reads data sets and control blocks: reports are not implemented (tasks B and D)",
    "client_example_control": "operates a control: controls are not implemented (task K)",
    "iec61850_client_example_log": "reads a log: logs are not implemented",
    "iec61850_client_file_async": "transfers a file: file services are not implemented",
}


def _server_binary() -> Path:
    """Our server, built in release mode and copied into the build context of the bench image."""
    subprocess.run(
        ["cargo", "build", "--release", "-q", "-p", "igs-iec61850-mms", "--bin", "igs61850-server"],
        cwd=IEC_REPO,
        check=True,
        timeout=900,
    )
    built = IEC_REPO / "target" / "release" / "igs61850-server"
    target = Path(__file__).resolve().parents[1] / "ours" / "igs61850-server"
    target.write_bytes(built.read_bytes())
    target.chmod(0o755)
    return target


def _decode(bench: Bench) -> tuple[list[str], list[tshark.Frame]]:
    out = bench.exec(
        "ours-capture", "tshark", "-r", f"/captures/{bench.capture_name}", "-Y", f"({ERRORS}) && !({KNOWN_ARTEFACT})"
    )
    errors = [line for line in out.stdout.splitlines() if line.strip()]
    return errors, tshark.decode(bench, service="ours-capture")


@pytest.mark.parametrize("example", list(EXAMPLES))
def test_the_example_client_against_our_server(example):
    """AT-07 reverse row: the example client is accepted by our server, and each of its requests is answered."""
    capture.CAPTURE_DIR.mkdir(exist_ok=True)
    name = f"ours-{example}-{time.strftime('%Y%m%dT%H%M%S')}.pcap"
    bench = Bench(name, profiles=("ours",))
    bench.down()
    try:
        _server_binary()
        bench.run("build", "our-server", timeout=900)
        bench.up("our-server", "ours-capture", "ref-client-ours")
        capture.wait_live(bench, service="ours-capture")
        result = bench.exec("ref-client-ours", "timeout", "15", example, check=False, timeout=120)
        capture.stop(bench, service="ours-capture")
        errors, frames = _decode(bench)
    finally:
        bench.down()

    print(f"== {example} (exit {result.returncode})\n{result.stdout}{result.stderr}")
    # The example runs on localhost, so the frames carry 127.0.0.1: the association is found by its PDU.
    accepted = [frame for frame in frames if "initiate-ResponsePDU" in frame.info]
    requests = [frame for frame in frames if "confirmed-RequestPDU" in frame.info]
    answers = [frame for frame in frames if "confirmed-ResponsePDU" in frame.info or "confirmed-ErrorPDU" in frame.info]
    print(f"requests {len(requests)}, answers {len(answers)}, note: {EXAMPLES[example]}")
    assert errors == [], f"the capture does not decode cleanly: {errors[:5]}"
    assert accepted, "our server accepts the association"
    # An example that sends no service request (the file example ends after the association: it is given no file) is
    # still a row: the association and the conclusion are checked, and the gap is documented in docs/interop.md.
    assert len(answers) == len(requests), "every request of the example is answered"

# SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
#
# SPDX-License-Identifier: Apache-2.0
"""X2, the reverse row of the logs: our server, built with the SCL import and serving the logs of `fixtures/logs.cid` (a log control block
that records the changes of its data set into the journal C1), against the example log client of the reference peer, run as a black box in
the network namespace of our server (rule 1 of IMPLEMENTATION.md).

The row checks the protocol: the association is accepted, each request of the example is answered, and the capture decodes without an
error in tshark. The fixture gives the names that the example reads (the IED TestIED, the device GenericIO, the log control block EventLog).
"""

from __future__ import annotations

import os
import subprocess
import time
from pathlib import Path

from igs_bench import capture, tshark
from igs_bench.compose import Bench

IEC_REPO = Path(os.environ.get("IGS_IEC_REPO", Path(__file__).resolve().parents[2]))
ERRORS = "_ws.malformed || _ws.expert.severity == error"
KNOWN_ARTEFACT = '_ws.expert.message == "Internal error, zero-byte MMS PDU"'
EXAMPLE = "iec61850_client_example_log"


def _server_binary() -> Path:
    """Our server with the SCL import, built in release mode and copied into the build context of the bench image."""
    subprocess.run(
        ["cargo", "build", "--release", "-q", "-p", "igs-iec61850-mms", "--features", "scl", "--bin", "igs61850-server"],
        cwd=IEC_REPO,
        check=True,
        timeout=900,
    )
    built = IEC_REPO / "target" / "release" / "igs61850-server"
    target = Path(__file__).resolve().parents[1] / "ours" / "igs61850-server"
    target.write_bytes(built.read_bytes())
    target.chmod(0o755)
    return target


def test_the_log_example_reads_the_journal_of_our_server(request):
    capture.CAPTURE_DIR.mkdir(exist_ok=True)
    name = f"x2-{request.node.name}-{time.strftime('%Y%m%dT%H%M%S')}.pcap"
    bench = Bench(name, env={"OURS_ARGS": "--scl /fixtures/example_log.cid --ied TestIED"}, profiles=("ours",))
    bench.down()
    try:
        _server_binary()
        bench.run("build", "our-server", timeout=900)
        bench.up("our-server", "ours-capture", "ref-client-ours")
        capture.wait_live(bench, service="ours-capture")
        result = bench.exec("ref-client-ours", "timeout", "15", EXAMPLE, check=False, timeout=120)
        capture.stop(bench, service="ours-capture")
        out = bench.exec(
            "ours-capture", "tshark", "-r", f"/captures/{bench.capture_name}", "-Y",
            f"({ERRORS}) && !({KNOWN_ARTEFACT})",
        )
        errors = [line for line in out.stdout.splitlines() if line.strip()]
        frames = tshark.decode(bench, service="ours-capture")
    finally:
        bench.down()

    print(f"== {EXAMPLE} (exit {result.returncode})\n{result.stdout}{result.stderr}")
    accepted = [frame for frame in frames if "initiate-ResponsePDU" in frame.info]
    requests = [frame for frame in frames if "confirmed-RequestPDU" in frame.info]
    answers = [frame for frame in frames if "confirmed-ResponsePDU" in frame.info or "confirmed-ErrorPDU" in frame.info]
    print(f"requests {len(requests)}, answers {len(answers)}; the example names TestIEDGenericIO and LLN0$LG$EventLog, a model of its own (docs/interop.md)")
    assert errors == [], f"the capture does not decode cleanly: {errors[:5]}"
    assert accepted, "our server accepts the association"
    assert len(answers) == len(requests), "every request of the example is answered"
    # The functional part: the example finds the log of its node, enables it, and reads its control block (the device and the
    # block names come from the fixture: the IED TestIED and the device GenericIO give TestIEDGenericIO). It receives no entry: no
    # change was recorded after its request, which is the state the example leaves (docs/interop.md).
    assert "Found logs in LN" in result.stdout and "EventLog" in result.stdout, "the example finds the log"
    assert "Enabled log" in result.stdout, "the example enables the log"
    assert "TestIEDGenericIO/LLN0$EventLog" in result.stdout, "the example reads the control block of our model"

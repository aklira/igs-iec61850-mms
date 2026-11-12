# SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
#
# SPDX-License-Identifier: Apache-2.0
"""D6, the reverse rows of the reports: the reference example client `iec61850_client_example_reporting` runs in the network namespace of
our server, serving the synthetic IED of `fixtures/simpleio.cid` (the device `simpleIOGenericIO`, the data set `Events` and the block
`EventsRCB01` of LLN0). Our server changes the status `GGIO1.Ind1.stVal` every second, so that the block sends its reports. The capture
must decode without an error in tshark. The peer is a black box (rule 1); the fixture is synthetic.
"""

from __future__ import annotations

import os
import subprocess
import time
from pathlib import Path

from igs_bench import capture
from igs_bench.compose import Bench

IEC_REPO = Path(os.environ.get("IGS_IEC_REPO", Path(__file__).resolve().parents[2]))
ERRORS = "_ws.malformed || _ws.expert.severity == error"
KNOWN_ARTEFACT = '_ws.expert.message == "Internal error, zero-byte MMS PDU"'
EXAMPLE = "iec61850_client_example_reporting"
OURS_ARGS = "--scl /fixtures/simpleio.cid --toggle-ms 1000"


def _server_binary() -> Path:
    """Our server with the `scl` feature, in release mode, copied into the build context of the bench image."""
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


def _decode_errors(bench: Bench) -> list[str]:
    out = bench.exec(
        "ours-capture", "tshark", "-r", f"/captures/{bench.capture_name}", "-Y", f"({ERRORS}) && !({KNOWN_ARTEFACT})"
    )
    return [line for line in out.stdout.splitlines() if line.strip()]


def _report_count(bench: Bench) -> int:
    """The information reports that our server sent: the unconfirmed PDUs of the capture, in the order of the frames."""
    out = bench.exec(
        "ours-capture",
        "tshark",
        "-r",
        f"/captures/{bench.capture_name}",
        "-Y",
        'mms && _ws.col.info contains "unconfirmed-PDU"',
        "-T",
        "fields",
        "-e",
        "frame.number",
    )
    return len([line for line in out.stdout.splitlines() if line.strip()])


def test_the_reference_reporting_client_receives_the_reports_of_our_server(request):
    capture.CAPTURE_DIR.mkdir(exist_ok=True)
    name = f"d6-reverse-{time.strftime('%Y%m%dT%H%M%S')}.pcap"
    bench = Bench(name, profiles=("ours",), env={"OURS_ARGS": OURS_ARGS})
    bench.down()
    try:
        _server_binary()
        bench.run("build", "our-server", timeout=900)
        bench.up("our-server", "ours-capture", "ref-client-ours")
        capture.wait_live(bench, service="ours-capture")
        result = bench.exec("ref-client-ours", "timeout", "20", EXAMPLE, check=False, timeout=120)
        capture.stop(bench, service="ours-capture")
        errors = _decode_errors(bench)
        reports = _report_count(bench)
    finally:
        bench.down()
    print(f"== {EXAMPLE} (exit {result.returncode}), reports sent by our server: {reports}")
    print(result.stdout + result.stderr)
    assert errors == [], f"the capture does not decode cleanly: {errors[:5]}"
    # The example is waited for, and ends on its timeout (exit 124): the reports are in the capture, not in its output.
    assert reports >= 2, "our server sends the general-interrogation report and the reports of the changes"
    assert "service error" not in result.stdout + result.stderr, result.stdout

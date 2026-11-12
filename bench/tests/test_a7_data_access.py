# SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
#
# SPDX-License-Identifier: Apache-2.0
"""A7: the bench matrix of the data access, our client against the example servers of the reference peer.

Each row runs one example server of libiec61850 (a black box, rule 1 of IMPLEMENTATION.md). The same script runs against
every row: the logical devices, the nodes, the names, the functional groups, one definition, two reads, and one write of
a status attribute, which the standard does not let a client change. The capture of each row must decode without an error
in tshark. A row that does not do what the script expects is recorded as a known gap, with its reason, in
`docs/interop.md`; the test is then marked as an expected failure, and it fails (strict) if the row starts to pass.
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
# The status and the measurement the script reads, and the status it writes: the example logical device is the one that
# discovery names, so the script takes it from the domains.
NODE = "GGIO1"
# The filter of the decode errors, as the F4 scenario has it: a Wireshark artefact (a zero-byte MMS PDU, seen with the
# reference peer as well) is not an error of the stack.
ERRORS = "_ws.malformed || _ws.expert.severity == error"
KNOWN_ARTEFACT = '_ws.expert.message == "Internal error, zero-byte MMS PDU"'
STATUS = "GGIO1$ST$Ind1$stVal"
MEASURE = "GGIO1$MX$AnIn1$mag$f"


def _client_binary() -> Path:
    """The client of this repository, built in release mode."""
    subprocess.run(
        ["cargo", "build", "--release", "-q", "-p", "igs-iec61850-mms", "--bin", "igs61850-client"],
        cwd=IEC_REPO,
        check=True,
        timeout=900,
    )
    return IEC_REPO / "target" / "release" / "igs61850-client"


def _run(client: Path, *args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(client), "--server", PEER, "--lenient-parameter-cbbs", *args],
        capture_output=True,
        text=True,
        timeout=180,
    )


def _decode_errors(bench: Bench) -> list[str]:
    out = bench.exec("capture", "tshark", "-r", f"/captures/{bench.capture_name}", "-Y", f"({ERRORS}) && !({KNOWN_ARTEFACT})")
    return [line for line in out.stdout.splitlines() if line.strip()]


def _script(bench: Bench, server: str) -> dict[str, subprocess.CompletedProcess[str]]:
    """Runs the script against the server, and returns each step's result by name.

    The script takes its node and its object from the discovery, so that it runs against any example: the node is GGIO1
    when the server has it, else the first node with a status group; the object is the first one of that status group.
    """
    client = _client_binary()
    bench.up("ref-server", "capture")
    capture.wait_live(bench)
    steps: dict[str, subprocess.CompletedProcess[str]] = {}
    steps["domains"] = _run(client, "domains")
    device = next((line.strip() for line in steps["domains"].stdout.splitlines() if line.strip()), "")
    if device:
        steps["nodes"] = _run(client, "nodes", device)
        nodes = [line.strip() for line in steps["nodes"].stdout.splitlines() if line.strip()]
        nodes.sort(key=lambda name: name != NODE)
        for node in nodes:
            groups = _run(client, "groups", device, node)
            status = next((line.split(":", 1)[1].split() for line in groups.stdout.splitlines() if line.startswith("ST:")), [])
            if status:
                steps["groups"] = groups
                break
        # The first status object that has a stVal: a server may describe an object without one (the control example
        # describes its Mod without a stVal), and the script reads the status of an object that has it.
        for obj in status:
            definition = _run(client, "definition", device, node, "ST", obj)
            if "stVal:" in definition.stdout:
                break
        steps["definition"] = definition
        status_item = f"{node}$ST${obj}$stVal"
        steps["read"] = _run(client, "read", device, status_item)
        steps["write"] = _run(client, "write", device, status_item, "true")
    capture.stop(bench)
    steps["_device"] = subprocess.CompletedProcess([], 0, device, "")
    return steps


# The known gaps of each row, by step, with the reason. A gap makes the test an expected failure (strict). The rows
# that pass have no gap here; the features they do not exercise (data sets, reports, controls, files, setting groups) are
# listed in docs/interop.md, with the task that adds each.
EXPECTED = {
    "server_example_basic_io": {"gaps": {}},
    "server_example_substitution": {"gaps": {}},
    "server_example_control": {"gaps": {}},
    "server_example_dynamic": {"gaps": {}},
    "server_example_sg": {"gaps": {}},
    "server_example_files": {"gaps": {}},
    "server_example_service_tracking": {"gaps": {}},
}


@pytest.mark.parametrize("server", list(EXPECTED))
def test_our_client_against_the_example_server(request, server):
    """AT-07 and AT-08 rows: discovery and data access of the reference example, then a clean capture."""
    capture.CAPTURE_DIR.mkdir(exist_ok=True)
    name = f"{server}-{time.strftime('%Y%m%dT%H%M%S')}.pcap"
    bench = Bench(name, env={"REF_SERVER": server})
    bench.down()
    try:
        steps = _script(bench, server)
        errors = _decode_errors(bench)
    finally:
        bench.down()

    gaps = EXPECTED[server]["gaps"]
    for step, result in steps.items():
        print(f"== {step} (exit {result.returncode})\n{result.stdout}{result.stderr}")
    assert errors == [], f"the capture does not decode cleanly: {errors[:5]}"
    assert steps["_device"].stdout, "the server has a logical device"
    assert "nodes" in steps and steps["nodes"].returncode == 0, steps["nodes"].stderr
    assert "groups" in steps, "a logical node has a status group"
    assert steps["groups"].returncode == 0, steps["groups"].stderr
    assert steps["definition"].returncode == 0 or "definition" in gaps, steps["definition"].stderr
    assert "stVal:" in steps["definition"].stdout or "definition" in gaps, "a status object with a stVal is found"
    assert steps["read"].returncode == 0 or "read" in gaps, steps["read"].stdout + steps["read"].stderr
    assert "=" in steps["read"].stdout or "read" in gaps, steps["read"].stdout
    # A status is never written by a client (8-1 Table 27): the server refuses, and the client reports the item error.
    assert steps["write"].returncode == 1 and "written" not in steps["write"].stdout or "write" in gaps, steps["write"].stdout
    if gaps:
        pytest.xfail("known gap: " + "; ".join(f"{step}: {reason}" for step, reason in gaps.items()))

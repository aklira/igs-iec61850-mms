# SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
#
# SPDX-License-Identifier: Apache-2.0
"""F4: the MMS client of igs-iso9506-mms, run on the host, against an example IEC 61850 server of the
reference peer; the capture must decode cleanly in tshark.

The IEC 61850 profile of the MMS crates comes with task N2. Until then the client runs with the
settings closest to it: nesting level 6 (8-1 Tables 124 and 125) and the lenient parameter CBBs of the
MMS project (its gap G8). The listing of the variables of a logical device is expected to fail while
the client runs the ISO 9506 string limits: since N1 the codec reads identifiers of up to 64 characters,
and the IEC 61850 profile of N2 allows them. The test is a strict expected failure, so that it reports
when N2 lands.
"""

from __future__ import annotations

import os
import subprocess
from pathlib import Path

import pytest

from igs_bench import capture, tshark

MMS_REPO = Path(os.environ.get("IGS_MMS_REPO", Path(__file__).resolve().parents[3] / "igs-iso9506-mms"))
PEER = "127.0.0.1:11102"
SERVER = "172.31.103.10"
# Artefact of Wireshark already recorded by the MMS bench: a release without user information makes the
# MMS dissector run on zero bytes. Filtered and counted.
KNOWN_ARTEFACT = '_ws.expert.message == "Internal error, zero-byte MMS PDU"'
ERRORS = "_ws.malformed || _ws.expert.severity == error"


@pytest.fixture(scope="module")
def client() -> Path:
    """The igsmms-client binary, built from the MMS repository at its current commit."""
    subprocess.run(
        ["cargo", "build", "--release", "-q", "-p", "igs-iso9506-mms", "--bin", "igsmms-client"],
        cwd=MMS_REPO,
        check=True,
        timeout=900,
    )
    return MMS_REPO / "target" / "release" / "igsmms-client"


def _run(client: Path, *args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(client), *args, "--server", PEER, "--nesting", "6", "--lenient-parameter-cbbs"],
        capture_output=True,
        text=True,
        timeout=60,
    )


def _lines(bench, display_filter: str) -> list[str]:
    out = bench.exec("capture", "tshark", "-r", f"/captures/{bench.capture_name}", "-Y", display_filter)
    return [line for line in out.stdout.splitlines() if line.strip()]


def test_mms_client_identifies_and_lists_the_logical_devices(bench, client):
    bench.up("ref-server", "capture")
    capture.wait_live(bench)

    identify = _run(client, "identify")
    domains = _run(client, "names", "--class", "domain")
    capture.stop(bench)

    print(identify.stdout, identify.stderr, domains.stdout, domains.stderr, sep="\n")
    assert identify.returncode == 0, identify.stderr
    assert domains.returncode == 0, domains.stderr
    logical_devices = [line for line in domains.stdout.splitlines() if line.strip()]
    assert logical_devices, "the example server has at least one logical device"

    frames = tshark.decode(bench)
    for frame in frames:
        print(frame.number, frame.src, ":".join(frame.protocols), frame.info)
    assert _lines(bench, f"({ERRORS}) && !({KNOWN_ARTEFACT})") == []
    assert len(tshark.find(frames, src=SERVER, info="initiate-ResponsePDU")) == 2
    assert len(tshark.find(frames, src=SERVER, info="confirmed-ResponsePDU")) >= 2


@pytest.mark.xfail(strict=True, reason="ISO 9506 limit of 32 characters until the IEC 61850 profile (N2)")
def test_mms_client_lists_the_variables_of_a_logical_device(bench, client):
    bench.up("ref-server", "capture")
    capture.wait_live(bench)

    domains = _run(client, "names", "--class", "domain")
    first = next(line.strip() for line in domains.stdout.splitlines() if line.strip())
    variables = _run(client, "names", "--domain", first)
    capture.stop(bench)

    print(variables.stdout, variables.stderr, sep="\n")
    assert variables.returncode == 0, variables.stderr
    assert _lines(bench, f"({ERRORS}) && !({KNOWN_ARTEFACT})") == []

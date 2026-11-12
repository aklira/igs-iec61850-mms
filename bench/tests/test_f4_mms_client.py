# SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
#
# SPDX-License-Identifier: Apache-2.0
"""F4 and N2: the MMS client of igs-iso9506-mms, run on the host, against an example IEC 61850 server of
the reference peer; the captures must decode cleanly in tshark.

F4 ran the client with the ISO 9506 limits and the lenient parameter CBBs. N2 (D-033) adds the IEC
61850-8-1 profile: identifiers of 64 characters, a nesting of six, and the CBBs and services of clause 24
for the declared model. The reference server negotiates no parameter CBB at all (gap G8, observed in the
capture of N2): the strict profile refuses it, naming str2, and the explicit lenient option accepts it.
"""

from __future__ import annotations

import os
import subprocess
from pathlib import Path

from igs_bench import capture, tshark

MMS_REPO = Path(os.environ.get("IGS_MMS_REPO", Path(__file__).resolve().parents[3] / "igs-iso9506-mms"))
PEER = "127.0.0.1:11102"
SERVER = "172.31.103.10"
# Artefact of Wireshark already recorded by the MMS bench: a release without user information makes the
# MMS dissector run on zero bytes. Filtered and counted.
KNOWN_ARTEFACT = '_ws.expert.message == "Internal error, zero-byte MMS PDU"'
ERRORS = "_ws.malformed || _ws.expert.severity == error"
# The IEC 61850 profile of the client: the logical-node model is declared (N2).
IEC_PROFILE = ["--profile", "iec61850", "--logical-nodes"]


def _binary() -> Path:
    """The igsmms-client binary, built from the MMS repository at its current commit."""
    subprocess.run(
        ["cargo", "build", "--release", "-q", "-p", "igs-iso9506-mms", "--bin", "igsmms-client"],
        cwd=MMS_REPO,
        check=True,
        timeout=900,
    )
    return MMS_REPO / "target" / "release" / "igsmms-client"


def _run(client: Path, *args: str, lenient: bool = False) -> subprocess.CompletedProcess[str]:
    extra = ["--lenient-parameter-cbbs"] if lenient else []
    return subprocess.run(
        [str(client), "--server", PEER, *IEC_PROFILE, *extra, *args],
        capture_output=True,
        text=True,
        timeout=180,
    )


def _lines(bench, display_filter: str) -> list[str]:
    out = bench.exec("capture", "tshark", "-r", f"/captures/{bench.capture_name}", "-Y", display_filter)
    return [line for line in out.stdout.splitlines() if line.strip()]


def _client_build():
    return _binary()


def test_strict_profile_refuses_the_peer_and_names_the_cbb(bench):
    """AT-04: without the lenient option, the peer that negotiates no CBB is refused, with the CBB named."""
    client = _client_build()
    bench.up("ref-server", "capture")
    capture.wait_live(bench)

    identify = _run(client, "identify")
    capture.stop(bench)

    assert identify.returncode != 0
    assert "str2" in identify.stderr, identify.stderr
    assert "not negotiated" in identify.stderr, identify.stderr


def test_lenient_profile_identifies_and_lists_the_logical_devices(bench):
    client = _client_build()
    bench.up("ref-server", "capture")
    capture.wait_live(bench)

    identify = _run(client, "identify", lenient=True)
    domains = _run(client, "names", "--class", "domain", lenient=True)
    capture.stop(bench)

    print(identify.stdout, identify.stderr, domains.stdout, domains.stderr, sep="\n")
    assert identify.returncode == 0, identify.stderr
    assert domains.returncode == 0, domains.stderr
    assert [line for line in domains.stdout.splitlines() if line.strip()], "the server has a logical device"

    frames = tshark.decode(bench)
    for frame in frames:
        print(frame.number, frame.src, ":".join(frame.protocols), frame.info)
    assert _lines(bench, f"({ERRORS}) && !({KNOWN_ARTEFACT})") == []
    assert len(tshark.find(frames, src=SERVER, info="initiate-ResponsePDU")) == 2


def test_lenient_profile_lists_the_variables_with_long_names(bench):
    """AT-02: names of up to 39 characters, from the server, are accepted under the IEC limits (N1, N2)."""
    client = _client_build()
    bench.up("ref-server", "capture")
    capture.wait_live(bench)

    domains = _run(client, "names", "--class", "domain", lenient=True)
    first = next(line.strip() for line in domains.stdout.splitlines() if line.strip())
    variables = _run(client, "names", "--domain", first, lenient=True)
    capture.stop(bench)

    print(variables.stdout[:2000], variables.stderr, sep="\n")
    assert variables.returncode == 0, variables.stderr
    names = [line for line in variables.stdout.splitlines() if line.strip()]
    assert len(names) > 100, "the example server lists its variables over several pages"
    assert max(len(name) for name in names) > 32, "a name longer than the ISO 9506 limit"
    assert _lines(bench, f"({ERRORS}) && !({KNOWN_ARTEFACT})") == []

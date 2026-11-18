# SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
#
# SPDX-License-Identifier: Apache-2.0
"""X4, the names of the setting group control block (SGCB, 8-1 table 71 and 8-1 §16.1): the MMS client of igs-iso9506-mms lists the
named variables of the domain, against the reference peer's example server with setting groups (`server_example_sg`) and against our
server (`igs61850-server --scl` with the settings file of the tests). The SGCB and its attributes must be listed by both.

The reference peer does not list `ResvTms`, which table 71 gives to the SGCB; the test records that difference and does not require it.
Its capture must decode without an error in tshark (rule 1: the peer is a black box).
"""

from __future__ import annotations

import os
import socket
import subprocess
import time
from pathlib import Path

from igs_bench import capture
from igs_bench.compose import Bench

IEC_REPO = Path(os.environ.get("IGS_IEC_REPO", Path(__file__).resolve().parents[2]))
MMS_REPO = Path(os.environ.get("IGS_MMS_REPO", Path(__file__).resolve().parents[3] / "igs-iso9506-mms"))
PEER = "127.0.0.1:11102"
OURS = "127.0.0.1:11110"
SETTINGS = IEC_REPO / "crates" / "igs-iec61850-mms" / "tests" / "data" / "settings.cid"
ERRORS = "_ws.malformed || _ws.expert.severity == error"
KNOWN_ARTEFACT = '_ws.expert.message == "Internal error, zero-byte MMS PDU"'
# The IEC 61850 profile of the client: the logical-node model is declared, and the peer's parameter CBBs are lenient (G8).
IEC_PROFILE = ["--profile", "iec61850", "--logical-nodes", "--lenient-parameter-cbbs"]
SGCB = "LLN0$SP$SGCB"
COMMON = ["NumOfSG", "ActSG", "EditSG", "CnfEdit", "LActTm"]


def _binary(repo: Path, package: str, name: str, *features: str) -> Path:
    """A release binary of a repository, built at its current commit."""
    command = ["cargo", "build", "--release", "-q", "-p", package, "--bin", name]
    if features:
        command += ["--features", ",".join(features)]
    subprocess.run(command, cwd=repo, check=True, timeout=900)
    return repo / "target" / "release" / name


def _names(client: Path, address: str, domain: str | None = None) -> list[str]:
    """The named variables of a domain (or of the server when there is no domain), as the client lists them."""
    args = [str(client), "--server", address, *IEC_PROFILE, "names", "--class", "named-variable"]
    if domain is not None:
        args += ["--domain", domain]
    result = subprocess.run(args, capture_output=True, text=True, timeout=120)
    assert result.returncode == 0, result.stdout + result.stderr
    return [line.strip() for line in result.stdout.splitlines() if line.strip()]


def _domain_of_peer(client: Path) -> str:
    """The first logical device of the peer (its domains are listed by the client)."""
    args = [str(client), "--server", PEER, *IEC_PROFILE, "names", "--class", "domain"]
    result = subprocess.run(args, capture_output=True, text=True, timeout=120)
    assert result.returncode == 0, result.stdout + result.stderr
    domains = [line.strip() for line in result.stdout.splitlines() if line.strip()]
    assert domains, "the peer lists at least one domain"
    return domains[0]


def _wait_port(address: str, seconds: float = 10.0) -> None:
    host, port = address.rsplit(":", 1)
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        with socket.socket() as probe:
            if probe.connect_ex((host, int(port))) == 0:
                return
        time.sleep(0.1)
    raise AssertionError(f"nothing listens on {address}")


def _decode_errors(bench: Bench) -> list[str]:
    out = bench.exec(
        "capture", "tshark", "-r", f"/captures/{bench.capture_name}", "-Y", f"({ERRORS}) && !({KNOWN_ARTEFACT})"
    )
    return [line for line in out.stdout.splitlines() if line.strip()]


def test_the_sgcb_is_named_by_the_reference_peer_and_by_our_server(request):
    """Both servers list the SGCB, its structure and its attributes; the capture of the peer decodes cleanly."""
    capture.CAPTURE_DIR.mkdir(exist_ok=True)
    name = f"x4-{request.node.name}-{time.strftime('%Y%m%dT%H%M%S')}.pcap"
    bench = Bench(name, env={"REF_SERVER": "server_example_sg"})
    bench.down()
    ours = None
    try:
        mms = _binary(MMS_REPO, "igs-iso9506-mms", "igsmms-client")
        served = _binary(IEC_REPO, "igs-iec61850-mms", "igs61850-server", "scl")
        bench.up("ref-server", "capture")
        capture.wait_live(bench)

        domain = _domain_of_peer(mms)
        peer_names = _names(mms, PEER, domain)
        capture.stop(bench)
        errors = _decode_errors(bench)

        ours = subprocess.Popen(
            [str(served), "--listen", OURS, "--scl", str(SETTINGS)],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        _wait_port(OURS)
        our_names = _names(mms, OURS, "IED1LD1")
    finally:
        if ours is not None:
            ours.terminate()
            ours.wait(timeout=10)
        bench.down()

    print("peer:", [n for n in peer_names if "SGCB" in n])
    print("ours:", [n for n in our_names if "SGCB" in n])
    assert errors == [], f"the capture does not decode cleanly: {errors[:5]}"
    for attribute in [SGCB, *(f"{SGCB}${a}" for a in COMMON)]:
        assert attribute in peer_names, f"the peer lists {attribute}"
        assert attribute in our_names, f"our server lists {attribute}"
    assert f"{SGCB}$ResvTms" in our_names, "our server lists ResvTms, which table 71 gives to the SGCB"
    assert f"{SGCB}$ResvTms" not in peer_names, "recorded difference: the peer does not list ResvTms (docs/interop.md)"

# Interop bench

Black-box tests of `igs-iec61850-mms` (and of the MMS crates it builds on) against a reference peer,
with captures decoded by `tshark`. Same approach as the bench of `igs-iso9506-mms`.

## Reference peer

The example programs of libiec61850 (GPL-3.0), tag `v1.6.2.1`, commit `c28c27c`, built by
`peer/Dockerfile`. They run as separate processes only. **Nobody reads their source**: it stays in the
build stage of the image, and only executables reach the final image (rule 1 of `IMPLEMENTATION.md`).
The image is built locally and is not distributed.

`REF_SERVER` selects the example server (default `server_example_basic_io`); the image also holds
`server_example_control`, `server_example_dynamic`, `server_example_files`, `server_example_sg`,
`server_example_substitution`, `server_example_service_tracking` and the IEC 61850 client examples.

## Needs

- Docker with the compose plugin, or Podman with `podman-compose` (`BENCH_COMPOSE=podman-compose`).
- Python 3.10 or later with `pytest`. `tcpdump` and `tshark` run in the `capture` container.
- For the F4 scenario, a checkout of `igs-iso9506-mms` next to this repository (or `IGS_MMS_REPO`).

## Run

```bash
cd bench
python3 -m venv .venv
.venv/bin/pip install pytest
podman-compose -p igs61850bench build ref-server
BENCH_COMPOSE=podman-compose .venv/bin/pytest
```

Captures stay in `captures/`, which git ignores. The reference server publishes its port 102 on
`127.0.0.1:11102` (the MMS bench uses 10102, so both can run).

## Layout

| Path | Content |
| --- | --- |
| `docker-compose.yml` | Reference server, reference client, capture container |
| `peer/Dockerfile` | Build of the reference peer |
| `igs_bench/` | Compose wrapper, capture and tshark helpers (from the MMS bench) |
| `tests/` | Scenarios, one file per task |

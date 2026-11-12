# SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
#
# SPDX-License-Identifier: Apache-2.0
#
# Our server, for the reverse rows of the bench: the reference clients of the peer run in the same network namespace and
# connect to it on port 102. The binary is copied here by the test that builds the image (it is not committed).

FROM docker.io/library/debian:bookworm-slim
COPY igs61850-server /usr/local/bin/igs61850-server
RUN chmod 0755 /usr/local/bin/igs61850-server

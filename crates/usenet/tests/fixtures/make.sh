#!/bin/sh
# Rebuilds the repair/unpack fixtures with the standard tools. Run from the
# repo root:
#   docker run --rm -v "$PWD/crates/usenet/tests/fixtures:/out" debian:bookworm-slim sh /out/make.sh
set -eu
sed -i 's/Components: main/Components: main non-free/' /etc/apt/sources.list.d/debian.sources
apt-get update -qq && apt-get install -y -qq par2 rar p7zip-full python3 >/dev/null
cd /tmp
# A deterministic 600 KB payload the tests can regenerate and compare.
python3 -c "
import sys
x=1
out=bytearray()
for _ in range(600_000):
    x^=(x<<13)&0xffffffff; x^=x>>17; x^=(x<<5)&0xffffffff
    out.append(x&0xff)
open('payload.bin','wb').write(out)
"
rm -rf /out/rarset /out/sevenz /out/password && mkdir -p /out/rarset /out/sevenz /out/password
# RAR in 200 KB volumes, stored (-m0) so the sizes are predictable, plus 20% PAR2.
rar a -m0 -v200k -idq /out/rarset/show.rar payload.bin
cd /out/rarset && par2 create -q -q -r20 -n2 show.par2 show.part*.rar && cd /tmp
# A 7z archive.
7z a -mx=1 -bd /out/sevenz/movie.7z payload.bin >/dev/null
# A password-protected RAR (password: swarmy).
rar a -m0 -pswarmy -idq /out/password/locked.rar payload.bin
ls -la /out/rarset /out/sevenz /out/password

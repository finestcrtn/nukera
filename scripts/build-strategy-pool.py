#!/usr/bin/env python3
"""build-strategy-pool.py — turn proxytest_strategies.list (one raw argv per line)
into individual .strat files, plus a hand-crafted set named by purpose."""
import os, sys, re

POOL = "config/zapret/strategies/pool"

# 1) split proxytest_strategies.list into 59 individual .strat files
src = f"{POOL}/proxytest_strategies.list"
out_dir = f"{POOL}/byebyedpi"
os.makedirs(out_dir, exist_ok=True)
i = 0
for line in open(src):
    line = line.strip()
    if not line:
        continue
    i += 1
    with open(f"{out_dir}/p_{i:02d}.strat", "w") as f:
        f.write(line + "\n")

# 2) hand-crafted proven winners, named for purpose
NAMED = {
    "00_default_hostfakesplit.strat":
        "--filter-tcp=80,443 --dpi-desync=hostfakesplit "
        "--dpi-desync-fooling=ts --dpi-desync-autottl=2",

    "01_quic_fake.strat":
        "--filter-udp=443 --dpi-desync=fake --dpi-desync-repeats=6 "
        "--dpi-desync-autottl=2 --dpi-desync-any-protocol=1",

    "02_syndata_disorder.strat":
        "--filter-tcp=80,443 --dpi-desync=syndata,disorder "
        "--dpi-desync-fooling=badseq --dpi-desync-autottl=2",

    "03_multisplit_md5sig.strat":
        "--filter-tcp=80,443 --dpi-desync=multisplit "
        "--dpi-desync-split-pos=method+2 "
        "--dpi-desync-fooling=md5sig --dpi-desync-autottl=2",

    "04_tlsrec_split.strat":
        "--filter-tcp=443 --dpi-desync=multisplit "
        "--dpi-desync-split-pos=sniext+1 "
        "--dpi-desync-fooling=md5sig --dpi-desync-autottl=2",

    "05_disorder_fake.strat":
        "--filter-tcp=443 --dpi-desync=disorder,fake "
        "--dpi-desync-repeats=4 --dpi-desync-fooling=ts,badseq "
        "--dpi-desync-autottl=2",

    "06_ipfrag2.strat":
        "--filter-tcp=80,443 --dpi-desync=ipfrag2 "
        "--dpi-desync-fooling=ts --dpi-desync-autottl=2",

    "07_fakeddisorder.strat":
        "--filter-tcp=80,443 --dpi-desync=multidisorder "
        "--dpi-desync-split-pos=1,midsld "
        "--dpi-desync-fooling=ts,badseq,md5sig "
        "--dpi-desync-fakeddisorder-pattern=0x00000000 "
        "--dpi-desync-autottl=2",

    "50_social_hostfakesplit_alt.strat":
        "--filter-tcp=443 "
        "--hostlist=/etc/unblocker/hostlists/social.sites "
        "--hostlist-auto=/etc/unblocker/hostlists/auto.list "
        "--dpi-desync=hostfakesplit --dpi-desync-fooling=ts,badseq "
        "--dpi-desync-hostfakesplit-mod=host=cdninstagram.com "
        "--dpi-desync-autottl=2",

    "51_quic_social.strat":
        "--filter-udp=443 --filter-l7=quic "
        "--hostlist=/etc/unblocker/hostlists/social.sites "
        "--dpi-desync=fake --dpi-desync-repeats=6 "
        "--dpi-desync-autottl=2 --dpi-desync-any-protocol=1",

    "52_yt_general.strat":
        "--filter-tcp=80,443 --hostlist=/etc/unblocker/hostlists/youtube.sites "
        "--dpi-desync=hostfakesplit --dpi-desync-fooling=ts,badseq "
        "--dpi-desync-autottl=2",
}
for name, args in NAMED.items():
    with open(f"{POOL}/{name}", "w") as f:
        f.write(f"--filter-tcp=80,443 --hostlist=/etc/unblocker/hostlists/general.sites\n")
        f.write(f"# {name}\n")
        f.write(f"# {args}\n")
        f.write(args + "\n")

print(f"Wrote {i} .strat files in {out_dir}")
print(f"Wrote {len(NAMED)} named strategy files in {POOL}")
print(f"Total pool size: {i + len(NAMED)} strategies")

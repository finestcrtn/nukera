# Agent Workflow Rules

## 1. Keep `project.md` in sync

Update `project.md` after **every** structural change (new/removed/renamed file,
new crate/binary, config path change, script behavior change, dependency change).

Quick checklist:
- File added/removed/renamed → File Index (Section 12)
- New crate/binary → Sections 3, 13
- New bypass layer/flow → Sections 1, 3, 8
- Config path/state file → Section 10
- Script behavior changed → Section 7
- Upstream dependency changed → Sections 5, 11

## 2. Git snapshots before experimenting

Commit the known-working state before experiments.

## 3. Rebuild the app after every change

**Always rebuild after changing the GUI or CLI source.** The installed
binary must reflect the latest code — never leave a stale build.
Stale artifacts cause subtle bugs — the GUI works but the daemon is
out of date, or vice versa. Build both, then reinstall if needed

## 4. Build and test new features immediately

**Never ship untested code.** After implementing a new feature:
1. `cargo build --release` (Rust core + CLI)
2. `flutter build linux --release` (Flutter GUI)
3. `pkexec cp ...` to install binaries
4. **Run the feature end-to-end** — test with real data, real sites.
   - Hosts changes: run enable, verify `/etc/hosts` has the new entries
   - DNS changes: test resolution against real domains
   - UI changes: launch the GUI, click through the flow
5. Only after passing real-world test: commit and report to user.


## 4. Never leave the network broken

- Test scripts: `trap` EXIT/INT/TERM, flush nft tables.
- Disable path: nuclear cleanup — flush all `unblocker*` nft tables,
  strip `/etc/hosts`, kill proxy daemons.
- If user reports broken net: run `bash /tmp/unblocker-kill.sh`
  immediately. Never claim "it works" when they say it doesn't.

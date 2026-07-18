---
level: patch
changelog: The deploy ledger is written atomically and a truncated ledger is rejected rather than re-minting every namespace
---

## ADR: atomic-deploy-ledger-write
title: The deploy ledger is written atomically and a truncated one is corruption
summary: Temp-file + rename for the ledger write, and a zero-byte ledger fails the read

**Context.** The `bynk.deploy.lock` ledger is the sole record of the persistent
Cloudflare identity a project has provisioned (KV namespace ids, deployed
Workers, queues), and its whole reason to exist is that an interrupted run must
never mint a *second* namespace for a resource that already has one.

Two flaws let it do exactly that (#736). `write_lock` replaced the file with a
plain `fs::write`, so a power loss or kill mid-write could leave a truncated —
including zero-byte — file on disk. And `DeployLock::version` carried a serde
`default`, so a zero-byte or version-less file *parsed* as a perfectly valid v1
ledger with no environments. The next `deploy` then saw no recorded KV id,
minted a fresh namespace, and silently detached the live Worker from its stored
data — the precise outcome the ledger exists to prevent (ADR 0180).

**Decision.** Write the ledger atomically *and durably*: serialise to an
exclusively-created sibling temp file, `sync_all` its data, preserve the existing
file's permissions, `rename` it over the ledger, then fsync the directory so the
new name itself survives a crash. Atomicity for a concurrent reader (the rename)
is not enough on its own — after a power loss the rename can be journaled while
the temp's data blocks are still only in the page cache — so the data fsync is
what makes the crash guarantee real. Any failure removes the temp so a full disk
never litters the project with `.tmp` files. And treat a malformed ledger as
corruption, not emptiness: an existing file that is zero-byte, whitespace-only,
or unparseable fails the read with a clear "restore from version control"
message, and `version` loses its serde default so a file that carries no version
is likewise rejected. A genuinely *absent* file remains a fresh project and reads
clean; a *version we do not support* stays a distinct, non-corruption error.

**Consequences.** An interrupted deploy no longer orphans namespaces: the read
either finds the prior intact ledger or refuses to proceed, and never invents an
empty one. The only behaviour change for an operator is that a hand-truncated or
externally-corrupted ledger now errors and asks to be restored from version
control instead of silently starting over. A version-less ledger has never been
written by any released `deploy`, so removing the default rejects only corruption.

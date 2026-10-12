# 0456 — Static assets build from the project's `assets/` folder, at content-hashed URLs, per Worker

- **Status:** Accepted (v0.328)

**Context.** Slice 1a checked `asset` declarations but refused to build them
(#1883). Building one needs answers the checker never had to give: where the
file may come from, how its URL is formed, which Worker serves it, and how
`bynk-emit` reads it without touching the disk.

**Decision.**

- **Assets come only from `<project root>/assets/`.** A declared path must lie
  inside that folder (`bynk.asset.outside_assets_dir`), so a declaration can
  never publish `.env` or a source file. Raised in the review of #1903; a
  denylist was the alternative, and a folder is simpler to reason about and
  easier to relax later.
- **The caller pre-reads the folder.** `bynk-driver` and `bynk-testkit` read
  every regular file under `assets/` (symlinks are not followed) into
  `CompileOptions::asset_files`, as `sources` and the schema lock are, so
  `bynk-emit` stays disk-free. The LSP lists the folder for the existence check
  (`bynk.asset.missing_file`). With no folder read, a check skips that and a
  build treats the folder as empty, so no URL is made up for an unread file.
- **The URL is `/_assets/<stem>.<hash>.<ext>`**, `<hash>` the FNV-1a 64 of the
  content (`contract::fnv1a64_hex`, the contract hash's algorithm over bytes:
  reproducible and dependency-free; it only has to tell one project's files
  apart). Only the file name is kept, made URL-safe. One function
  (`assets::served_path`) defines it, for slice 2's route checks to share.
- **A Worker ships its transitive closure.** Name visibility stays one level
  (as for functions), but a Worker serves the files of every commons its
  `uses` reach, since a used commons may return another's `.url`.
- **Emission.** Each asset is an exported constant
  `{ readonly url: string }` in its declaring module, imported like a function.
  Each Worker with assets gets them under its own `assets/` directory, beside
  its code, a `_headers` file (`/_assets/*` immutable for a year), and an
  `[assets]` block with `run_worker_first = false`, `not_found_handling =
  "none"` and `html_handling = "none"` set explicitly. The Bundle target ships
  no files; a deployable Bundle build warns about each asset it cannot serve
  (`bynk.asset.not_served`), while `bynkc test`'s Bundle build stays quiet.
- **Single files cannot build an asset** (`bynk.asset.needs_project`): no
  project root, no Worker. Checking one is unaffected.
- **Tooling.** `bynk dev` watches `assets/`; `bynk deploy` records each
  Worker's served paths in `bynk.deploy.lock` (the hash is in the path, so a
  changed file is a changed line).

**Consequences.** compat-board can declare its stylesheet and deploy it.
Verified against `wrangler dev` 4.107.0: the asset is served as `text/css`
with the immutable cache header and without running the Worker, `_headers` is
not served, and an unknown `/_assets/` path falls through to the Worker. Fixed
paths (`at "/favicon.ico"`), route-conflict checks and suite access remain for
later slices of #1883.

# 0196 — What a Worker reads: a warning where the compiler loses sight, not a rule that forbids it

- **Status:** Accepted (v0.173)
- **Provenance:** #632, follow-up to the deploy track (spine #558) named as
  deferred work by [[0195]]. Not a deploy slice — the track's scope is driver
  behaviour; this changes the diagnostic surface.
- **Relates:** [[0195]] (secrets at deploy — the manifest this extends, and the
  deferral note this corrects), [[0018]] (config-as-capability — how a secret
  reaches a binding), [[0117]] (the non-failing warning channel this uses),
  [[0023]] (each increment stays single-purpose).

## Context

[[0195]] shipped `deploy`'s secret step with a boundary it stated honestly: the
compiler speaks **totally** for an `actor`'s auth secret, and **not at all** for
a `bynk.Secrets` name. It named the cost — *"a `Secrets.get("API_KEY")` the user
forgets to supply is still a production `None`"* — and deferred the fix with a
one-paragraph sketch:

> a static rule requiring `Secrets.get`'s argument to be a string literal would
> turn the floor into a census. The precedent exists (`cors` `origins`,
> `@cache`, `@limit` all require literal arguments) and all six call sites in the
> repo already comply, so the break is empty in practice.

**Both halves of that justification are wrong**, and the correction is this
record's content. The deferral note proposed a *harder* mechanism than the
problem needs and a *stronger* guarantee than the language permits.

**The precedent is not precedent.** `cors` `origins`, `@cache` `maxAge` and
`@limit` `maxBody` do require literals — but every one is **declaration-site
config**, validated where a policy or annotation is *written*
(`bynk-emit/src/project/validate.rs`). None constrains an **expression in an
ordinary argument position**. `Secrets.get(name)` is a capability-operation call,
checked for arity and type like any other (`bynk-check/src/checker/calls.rs`).
Constraining it would be the language's **first** rule about the shape of an
ordinary argument, sitting oddly beside `Fetch.send(req)` and `Logger.info(msg)`,
which constrain nothing.

**A census could not carry the semantics implied.** The signature is
`fn get(name: String) -> Effect[Option[String]]`. It returns an **`Option`**, and
absence is a legitimate, handled outcome — a fixture matches `None => "missing"`
and is a correct program. So a collected name **cannot** be *required* the way a
declared auth secret is. An unset auth secret is fail-closed and wrong (401 on
every request); an unset `Secrets.get` name is a `None` the program may be
entirely happy about. Erroring on one would break legal programs;
`Secrets.get("OPTIONAL_FLAG")` is a reasonable thing to write.

So the available prize is smaller than the note claimed, and worth taking anyway:
`deploy` can list what a Worker reads, and — the part that makes the list safe —
say when the list is not everything.

## Decision

**(A) A computed name warns; it is not forbidden.** `Secrets.get(<StrLit>)` is
collected; any other argument raises `bynk.secrets.computed_name`, a **non-failing
warning** ([[0117]]). The error the deferral note anticipated is refused: it would
buy a guarantee the language should not sell, making `Secrets.get(pickName())` a
compile failure to serve a *driver*'s convenience. The warning buys nearly all of
the value — every program in the tree today is warning-free, so its list is a
census — and it puts the fact in the one place that can state it. *Consequence:*
completeness is not enforced by the type system, so it must be carried as data —
which is (B). The escape hatch stays open and stays visible.

**(B) The manifest records completeness, not just names.** `bynk-secrets.json`
gains `read: [...]` and `read_complete: bool`, and `version` → **2**. A reader
seeing `read_complete: false` must not present its list as exhaustive; the plan
says so in both formats (`secrets incomplete <worker>`; JSON `secrets_complete`).
The version bump is deliberate rather than a default-on-absence read: a v1
manifest carries **no evidence either way** about computed names, so defaulting
`read_complete` to `true` for it would be the manifest's one claim that could be
silently wrong. *Consequence:* without this flag, a warning-based collection
would be exactly the "usually right, therefore trusted" list [[0195]] B refused to
build. The flag is what makes (A)'s warning honest downstream rather than only at
the moment of compiling.

**(C) A read is advisory; only a declared secret is required.** A missing `read`
value **warns** and deploys; a missing `declared` value stays a hard error. The
plan marks every line `declared` / `read` / `supplied`, and where a name is in
more than one class the strongest is shown — `declared` last and winning, since
it is the only one that makes a missing value fatal. *Consequence:* the two
classes keep the different semantics **their types already give them**, and
[[0195]] D2's floor/census distinction survives rather than collapsing: the census
sits *beside* the floor as explicitly weaker knowledge. This is the decision that
keeps the increment from quietly promoting `Option` into "required".

**(D) Collection resolves the capability to its declaring unit, never the
spelling.** A call site is collected only where its receiver resolves to
**`bynk`**'s `Secrets` — `unit_flattened` maps a context's capability to the unit
providing it, checked against `firstparty::BYNK_UNIT`. *Consequence:* an author
may declare their own capability (an adapter's `capability Jwt { … }` is ordinary),
so nothing stops one named `Secrets`. Matching the identifier would collect *that*
capability's names, and `deploy` would set them on Cloudflare — a real secret
written to a real account for a store that was never Cloudflare's.

> **D was corrected before implementation.** #632's accepted text read *"resolves
> through `consumes` aliases: `consumes bynk { Secrets as S }` … the pass belongs
> in the checker, which already has `unit_consumes_aliases`."* Every clause was
> wrong. **`consumes bynk { Secrets as S }` is not legal Bynk** — `as <alias>`
> and `{ Cap, … }` are a `choice` in the grammar, mutually exclusive, so a
> capability inside braces is a bare identifier; the rule described a program
> nobody can write. **`unit_consumes_aliases` is neither the checker's nor about
> capabilities** — it lives in `bynk-emit`'s project phase and maps
> `consumes a.b as Alias`, for cross-context calls. And **the real hazard —
> capability shadowing — went unnoticed**. The substance ("resolve, don't match
> the spelling") survives; the mechanism and the justification are replaced.

**(E) The manifest is emitted when anything is known, not only when a secret is
declared.** [[0195]] E emitted the file only for a non-empty `declared`
("absent rather than empty"). That rule stops working here: a context with **no
`actor` at all** can read `API_KEY`, and it is exactly the context this file now
exists to describe. Emit when `declared` is non-empty, **or** `read` is non-empty,
**or** `read_complete` is false; a context with no secrets of any kind still emits
nothing. *Consequence:* the file stops meaning "this context has declared
secrets" and starts meaning "here is what is known about this context's secrets,
including that something is not". A manifest whose `declared` is empty and whose
`read` is not is the shape most likely to be misread as "nothing required" — it
*is* nothing required, and the plan says `read` on every line of it.

**(F) The warning is raised on the check path and gated to the Workers target.**
It is raised in `run_checks`, which `bynk check` and the LSP both run and neither
builds — so the author sees it while typing, not only when they deploy. And only
under `--target workers`: the whole consequence is about `bynk deploy`, which no
other target has, so warning a bundle project about a deploy plan it will never
produce would be noise. *Consequence:* the collection runs twice — once in
`run_checks` for the warning, once in `build_output` for the manifest — over the
same pure function. That is a cheap AST walk beside emitting TypeScript, and one
function with one rule cannot disagree with itself; the alternative was threading
a new field through `RunChecks` for no behavioural gain.

## Consequences

`deploy` now says what a Worker reads, warns when a read name has no value, and
states plainly when its list is incomplete. The forgot-to-supply footgun [[0195]]
named is closed for every program that names its secrets with literals — which is
every program in the tree — and *visible* for the ones that do not.

The cost is that `read_complete` is now a load-bearing field a reader must
actually read. A driver, a CI job, or a human who takes `read` for a census
without checking it is back to the failure this record exists to prevent. That is
why the flag rides both plan formats rather than only the JSON, and why the short
plan states it **before** the lines it qualifies.

The language gains a warning and no restriction. `Secrets.get(pickName())` still
compiles, still runs, and still works — it is simply not plannable, and now says
so.

**Not taken, and named so it is not re-derived:** a `read` name is **not**
promoted to required, now or later, on the strength of being collected. The type
says absence is legal (`Option`), and no amount of static knowledge about the
*name* changes what the *program* is entitled to do with the value. An author who
wants a required secret has `actor … auth` today; an explicit spelling for
"required config" would be a different increment with a different type.

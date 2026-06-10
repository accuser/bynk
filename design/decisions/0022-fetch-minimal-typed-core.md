# 0022 — Fetch ships a minimal typed core; the header list waits for sequence types

- **Status:** Accepted (v0.18)
- **Spec:** §7.3.6 (the surface set)

## Context
`karn.Fetch` wants `headers: List[Header]`, but Karn has no sequence type —
`TypeRef` supports only `Result`/`Option`/`Effect`/`HttpResult` generics,
records, and enum sums.

## Decision
A single `send(req: Request) -> Effect[Result[Response, FetchError]]` with
`Method`/`FetchError` enums and a `Request` carrying the two headers the
exemplars need — `contentType`/`authorization` — as `Option[String]` fields.
A general header list is **deferred until Karn grows a sequence type**;
widening `Request` later is additive.

## Consequences
Honest, extensible, no premature stringly-typed API. Retiring this compromise
is an explicit goal of the collections increment (0023).

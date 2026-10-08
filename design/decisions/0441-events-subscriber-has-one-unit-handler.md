# 0441 — A `from Events` service has exactly one `on event` handler, returning `Effect[()]`

- **Status:** Accepted (v0.311)

**Context.** The events guide and the grammar reference both described a `from Events(E)` service as having one handler, `on event(e: E) -> Effect[()]`, but the checker enforced neither half (#1781). A second `on event` passed every check and was emitted as a second `event` member of one object literal: `tsc --strict` rejects that (`TS2300: Duplicate identifier 'event'`), and on the bundle path, which does not type-check, the later member silently replaced the earlier, so the first handler never ran. A return of `Effect[Int]` compiled, and its value was discarded: `Events.emit` is fire-and-forget, and the fan-out ignores what a subscriber returns.

**Decision.** A `from Events` service MUST have exactly one `on event` handler (`bynk.event.duplicate_handler`, reported at the second). That handler MUST return `Effect[()]` (`bynk.event.return_not_effect_unit`); a return that is not an `Effect` at all stays `bynk.service.return_not_effect`'s. These pin the subscriber's shape as every other protocol's already is: `on message` returns `Effect[QueueResult]`, a cron handler `Effect[Result[(), E]]`, and a `from websocket` service has exactly one `on open`.

**Consequences.** Source with two `on event` handlers, or a non-unit return, stops compiling. The first could never pass `tsc` and silently misbehaved without it; none in this repository did either. Two reactions to one event are two services, each delivered to independently, with failures isolated per subscriber. Rejected: allowing several handlers and delivering to each. That is a second spelling of what two services already say, and it would need the emitter to name the handlers apart.

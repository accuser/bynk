# Anatomy of a Bynk service

[What is Bynk?](what-is-bynk.md) shows the ideas in three-line fragments. This
page shows a **complete, runnable program in one piece** — refined types, a
context, a capability, a stateful agent, and an HTTP service, wired together — so
you can judge the shape and the verbosity at a glance before committing to the
tutorials. Read it once top to bottom; the walkthrough underneath names each
part.

```bynk
context analytics

-- A refined domain type: a page id that has already been validated, so nothing
-- downstream ever has to re-check it.
type Page = String where MinLength(1) and MaxLength(64)

-- A capability: the one outside-world effect this context needs. The interface
-- is declared here; the implementation is a provider.
capability Clock {
  fn now() -> Effect[Int]
}

provides Clock = SystemClock {
  fn now() -> Effect[Int] {
    0
  }
}

-- A keyed, stateful agent: one independent counter per page. State fields are
-- persisted between calls; a never-seen key starts from their zero values.
agent Counter {
  key page: Page

  state {
    hits: Int,
    lastSeen: Int,
  }

  on call bump(at: Int) -> Effect[Int] {
    let next = self.state.hits + 1
    commit { ...self.state, hits: next, lastSeen: at }
    next
  }
}

-- A service: the HTTP entry point. It asks for the `Clock` capability with
-- `given`, addresses the agent by key, and runs both as effects.
service api from http {
  on GET("/hits/:page") by Visitor (page: Page) -> Effect[HttpResult[Int]] given Clock {
    let at <- Clock.now()
    let counter = Counter(page)
    let total <- counter.bump(at)
    Ok(total)
  }
}
```

That is the whole program. Compiling it with `bynkc` produces TypeScript you can
read and deploy to Cloudflare Workers.

## What each part is

- **`context analytics`** — a *bounded context*: the unit Bynk deploys. On the
  `workers` target it becomes one Worker; the agent inside it becomes a Durable
  Object. See [How a Bynk program is shaped](../guides/program-structure/how-a-program-is-shaped.md).
- **`type Page = String where …`** — a *refined type*. The predicate is checked
  once, at the boundary, so every `Page` that exists downstream is already valid
  and `bump` never re-validates. This is the "make illegal states
  unrepresentable" idea in one line. See
  [the type-system philosophy](../guides/type-system/philosophy.md).
- **`capability Clock` + `provides … = SystemClock`** — an *effect* made
  explicit. A handler cannot read the clock unless it is granted `Clock`; the
  provider is the implementation that actually does. See
  [Understand the capability model](../guides/effects-and-capabilities/understand-the-capability-model.md).
- **`agent Counter`** — a *keyed, stateful entity*. Each `Page` is its own
  counter with its own persisted `state`; `commit` writes the next state. See
  [The agent model](../guides/agents-and-state/the-agent-model.md).
- **`service api from http`** — the *entry point*. It declares its needs
  with `given Clock`, addresses the agent by key (`Counter(page)`), and sequences
  effects with `let x <- …`. The `Effect[HttpResult[Int]]` return type makes both
  the effect and the HTTP outcome part of the contract. See
  [Handle an HTTP request](../guides/entry-points/http.md).

## How it fits together

The route binds `:page` to a `Page`, validating it at the edge. The handler runs
`Clock.now()` for a timestamp, addresses the `Counter` agent for that page, and
calls `bump` — which increments the persisted `hits` and returns the new total. Pure
domain types, an explicit effect, persistent state, and a typed HTTP boundary —
each named in the language, not left to convention.

Ready to build one yourself? Start at
[Tutorial 1](../tutorials/01-first-program.md); the stateful-agent and HTTP
pieces arrive in [Tutorial 5](../tutorials/05-stateful-agent.md).

# 5. Add a stateful agent

Everything so far has been stateless: a request comes in, a value goes out,
nothing is remembered. A URL shortener has to remember — which code maps to which
URL, and how often each was followed. In Karn, the unit of state is an **agent**:
a named thing, identified by a key, that owns some state and exposes handlers to
read and change it.

We give the shortener a `Link` agent. Keep editing `shortener.karn`.

## Declare an agent

```karn,ignore
agent Link {
  key code: ShortCode

  state {
    target: Option[Url],
    hits: Int,
  }

  on call register(url: Url) -> Effect[Result[(), LinkError]] {
    match self.state.target {
      Some(_) => Err(AlreadyExists)
      None => {
        commit { ...self.state, target: Some(url) }
        Ok(())
      }
    }
  }
}
```

Three parts make up the agent:

- `key code: ShortCode` — the identity. Each distinct `ShortCode` is a separate
  link with its own state. The refined `ShortCode` from
  [Tutorial 4](04-refined-types.md) means a link can only ever be keyed by a
  valid code.
- `state { target: Option[Url], hits: Int }` — the data this agent owns: the
  target URL (once registered) and a hit counter.
- `on call register(...)` — a handler. Handlers return an `Effect[T]` because
  they touch state. `register` stores the target the first time, and reports
  `AlreadyExists` if the code is taken.

## State must be zeroable

Here is the rule that shapes agent state: **every state field must have a zero
value**. When you address a link whose code has never been seen, Karn initialises
its state automatically — there is no constructor to call first — so each field
needs a well-defined starting value. `Int` starts at `0`, `Bool` at `false`,
`String` at `""`, and `Option[T]` at `None`.

Both our fields are fine: a brand-new link starts with `target: None` (no URL yet)
and `hits: 0`. That `None` is doing real work — it *means* "this code has never
been registered", which is exactly what `resolve` will check.

A field that *excludes* its natural zero is rejected. You might reach for
`Int where Positive` on the hit count — but a fresh link has had `0` hits, and
`Positive` excludes `0`:

```karn,ignore
state {
  target: Option[Url],
  hits: Int where Positive,   -- no honest starting value
}
```

```text
[karn.agents.non_zeroable_state_field] agent `Link` state field `hits` has no
defined zero value, so a fresh key cannot be initialised
```

When you genuinely need "not set yet", reach for `Option` — as we did for
`target`.

## Read and update state

Inside a handler, read state through `self.state`. To change it, build a new
state value and `commit` it. Add a `resolve` handler that returns the target and
counts the hit:

```karn,ignore
  on call resolve() -> Effect[Result[ResolveView, LinkError]] {
    match self.state.target {
      Some(url) => {
        let next = self.state.hits + 1
        commit { ...self.state, hits: next }
        Ok(ResolveView { target: url, hits: next })
      }
      None => Err(NotFound)
    }
  }
```

`commit { ...self.state, hits: next }` is the record-spread form from
[Tutorial 3](03-modelling-data.md): copy the current state, override `hits`, and
persist the result. State is never mutated in place; you commit a new value.

## See what it compiles to

The agent becomes a class that loads its state on entry and persists on `commit`.
The zero value is baked in as `__zeroOfLinkState`:

```typescript
const __LinkRegistry = new StateRegistry();
function __zeroOfLinkState(): LinkState { return { target: None, hits: 0 }; }

export class Link {
  // ...
  private async loadState(): Promise<LinkState> {
    const stored = await this.state.storage.get<LinkState>("state");
    return stored ?? __zeroOfLinkState();   // a fresh code starts from zero
  }

  async register(url: Url, deps: {}): Promise<Result<void, LinkError>> {
    const currentState = await this.loadState();
    switch (currentState.target.tag) {
      case "Some": {
        return Err(LinkError.AlreadyExists);
      }
      case "None": {
        await this.commitState({ ...currentState, target: Some(url) });
        return Ok(undefined);
      }
    }
    throw new Error("non-exhaustive match");
  }
}
```

That `?? __zeroOfLinkState()` is fresh-state initialisation in action: a code with
no stored state falls back to the zero value (`target: None`). On the `workers`
target the same agent compiles to a Cloudflare Durable Object instead, but the
handler logic you wrote is identical.

## Wire it into the API

Now the API can do real work. A small `CodeGen` capability mints new codes, and
the handlers store and resolve through the `Link` agent:

```karn,ignore
capability CodeGen {
  fn next() -> Effect[String]
}

provides CodeGen = FixedCodeGen {
  fn next() -> Effect[String] {
    "abc123"
  }
}
```

A **capability** is a dependency a handler asks for with `given`; a **provider**
supplies it. They are a topic in their own right — see
[Compose a provider](../how-to/capabilities/compose-a-provider.md) — but the shape
above is all we need: mint a raw string, then validate it into a `ShortCode`.

## The whole file

```karn
context shortener

type ShortCode = String where MinLength(6) and MaxLength(8)
type Url = String where MinLength(1) and MaxLength(2048)

type LinkError = enum {
  AlreadyExists,
  NotFound,
  Invalid,
}

fn describe(error: LinkError) -> String {
  match error {
    AlreadyExists => "code already in use"
    NotFound => "no such code"
    Invalid => "invalid code"
  }
}

type CreateLinkRequest = {
  target: Url,
}

type CreatedView = {
  code: ShortCode,
  target: Url,
}

type ResolveView = {
  target: Url,
  hits: Int,
}

capability CodeGen {
  fn next() -> Effect[String]
}

provides CodeGen = FixedCodeGen {
  fn next() -> Effect[String] {
    "abc123"
  }
}

agent Link {
  key code: ShortCode

  state {
    target: Option[Url],
    hits: Int,
  }

  on call register(url: Url) -> Effect[Result[(), LinkError]] {
    match self.state.target {
      Some(_) => Err(AlreadyExists)
      None => {
        commit { ...self.state, target: Some(url) }
        Ok(())
      }
    }
  }

  on call resolve() -> Effect[Result[ResolveView, LinkError]] {
    match self.state.target {
      Some(url) => {
        let next = self.state.hits + 1
        commit { ...self.state, hits: next }
        Ok(ResolveView { target: url, hits: next })
      }
      None => Err(NotFound)
    }
  }
}

service create {
  on call(target: Url) -> Effect[Result[ShortCode, LinkError]] given CodeGen {
    let raw <- CodeGen.next()
    match ShortCode.of(raw) {
      Err(_) => Err(Invalid)
      Ok(code) => {
        let link = Link(code)
        let outcome <- link.register(target)
        match outcome {
          Ok(_) => Ok(code)
          Err(e) => Err(e)
        }
      }
    }
  }
}

service api {
  on http POST "/links" (body: CreateLinkRequest) -> Effect[HttpResult[CreatedView]] given CodeGen {
    let raw <- CodeGen.next()
    match ShortCode.of(raw) {
      Err(_) => ServerError("generated an invalid code")
      Ok(code) => {
        let link = Link(code)
        let outcome <- link.register(body.target)
        match outcome {
          Ok(_) => Created(CreatedView { code: code, target: body.target })
          Err(linkError) => match linkError {
            AlreadyExists => Conflict("code already in use")
            NotFound => ServerError("unexpected state")
            Invalid => ServerError("invalid code")
          }
        }
      }
    }
  }

  on http GET "/links/:code" (code: ShortCode) -> Effect[HttpResult[ResolveView]] {
    let link = Link(code)
    let outcome <- link.resolve()
    match outcome {
      Ok(view) => Ok(view)
      Err(linkError) => match linkError {
        NotFound => NotFound
        AlreadyExists => ServerError("unexpected state")
        Invalid => ServerError("invalid code")
      }
    }
  }
}
```

```sh
karnc compile . --output out --target workers
```

The shortener now creates real links and resolves them, counting hits as it goes.

## What you have done

You gave the shortener a memory: a `Link` agent keyed by `ShortCode`, with
zeroable state (`target: Option[Url]`, `hits: Int`), handlers that read with
`self.state` and update with `commit`, and an API wired to store and resolve. You
saw fresh-state initialisation in the emitted code.

We have asserted that all this works — now let us prove it.

➡️ **[Tutorial 6: Test it](06-testing.md)**

---

*For what an agent really is and why state must be zeroable, see
[The agent model](../explanation/the-agent-model.md). For exact rules, see the
[agents reference](../reference/agents.md). For capabilities and providers, see
the [how-to guides](../how-to/capabilities/index.md).*

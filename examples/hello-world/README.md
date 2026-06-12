# Hello, World!

A complete Karn project in two source files and a test — run it locally,
then deploy it to Cloudflare, from the same build.

It is small, but it is not a toy: it shows the things Karn is *for*.

- **A refined type** — `Subject` is a `String` that is provably non-empty
  and at most 40 characters. `greeting` takes a `Subject`, so it never
  validates input: invalid subjects cannot exist.
- **Honest effects and capabilities** — the HTTP handlers declare
  `given Logger`; the dependency is visible in the signature, supplied by
  the platform (`consumes karn { Logger }`), and mockable in tests.
- **Typed HTTP** — `on http` handlers return `HttpResult`; the compiler
  generates the router, boundary validation, and the Worker entry point.
- **Tests in the language** — `karn test` fabricates a pinned
  `Mock[Subject]` and asserts behaviour, no harness code.

## Layout

```text
hello-world/
├── karn.toml               # project manifest ([paths])
├── src/
│   └── hello/
│       ├── text.karn       # commons hello.text — Subject + greeting
│       └── web.karn        # context hello.web — the HTTP service
└── tests/
    └── hello/
        └── text.karn       # tests targeting hello.text
```

A unit's dotted name mirrors its path: `commons hello.text` lives at
`src/hello/text.karn`. The `context` is the unit of deployment — it
becomes one Cloudflare Worker.

## Prerequisites

- `karnc` on your `PATH` (see the book's install page; from this
  repository: `cargo build --release -p karnc` →
  `target/release/karnc`).
- Node.js (for `karn test` and for Wrangler).

## Check and test

From this directory:

```sh
karnc check src
karnc test .
```

```text
hello.text:
  ✓ greets the world
  ✓ greets any valid subject
  ✓ rejects an empty subject

3 passed, 0 failed.
```

## Build the Worker

```sh
karnc compile src --output out --target workers
```

This emits a complete, standard Cloudflare Worker under
`out/workers/hello-web/` (entry point, router, dependency wiring, and
`wrangler.toml`), plus the shared runtime and the platform binding for
the `karn` surface.

## Run it locally

```sh
cd out/workers/hello-web
npx wrangler dev
```

Then, in another terminal:

```sh
curl http://localhost:8787/
# "Hello, World!"

curl http://localhost:8787/hello/Karn
# "Hello, Karn!"

curl http://localhost:8787/hello/this-name-is-way-too-long-to-be-a-valid-subject
# {"error":"a name must be non-empty and at most 40 characters"}  (HTTP 400)
```

The `Logger.info` lines appear in the `wrangler dev` console — that is
the `karn { Logger }` capability, bound by the toolchain for the
Cloudflare platform.

## Deploy it

From the same directory (a free Cloudflare account is enough;
`wrangler` will prompt you to log in the first time):

```sh
cd out/workers/hello-web
npx wrangler deploy
```

Wrangler prints the deployed URL — your greeting is live at
`https://hello-web.<your-subdomain>.workers.dev/hello/World`.

## Where to go next

- [Tutorials](../../docs/src/tutorials/01-first-program.md) — the URL
  shortener series grows these same ideas into a stateful service.
- [How a Karn program is shaped](../../docs/src/explanation/how-a-karn-program-is-shaped.md)
  — why contexts, capabilities, and boundaries look the way they do.

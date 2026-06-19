# Todo

A per-user todo list. The interesting part is the key: the agent is keyed by the
caller's **verified identity**, so every user transparently gets their own
private list with no `userId` parameter to pass — or to forge.

What it shows:

- **An agent keyed by a sealed identity** — `User` is a `Bearer` actor; its
  `UserId` is minted at the boundary and becomes the `Todos` key. `Todos(u.identity)`
  always addresses *the caller's* list.
- **List state with an initialiser** — a `List` has no implicit zero, so
  `items: List[TodoItem] = []` gives a fresh user an empty list; the `lastId`
  counter zeroes to `0`.
- **List combinators** — `any` and `map` (from `uses bynk.list`) check existence
  and flip an item to done, rebuilding state immutably.
- **Tests with no harness** — `bynkc test .` constructs the agent by key, calls
  its handlers, and asserts on the results.

## Layout

```text
todo/
├── bynk.toml
├── src/
│   └── todos.bynk       # context todos — agent + HTTP service
└── tests/
    └── todos.bynk       # tests targeting the todos context
```

## Check and test

```sh
bynkc check src
bynkc test .
```

```text
todos:
  ✓ add returns the item, freshly created and not done
  ✓ completing a known id succeeds
  ✓ completing an unknown id is NotFound

3 passed, 0 failed.
```

## Run it

```sh
bynkc compile src --output out --target workers
cd out/workers/todos
#   echo 'AUTH_JWT_SECRET = "dev-secret"' > .dev.vars
npx wrangler dev
```

```sh
# every request carries a Bearer JWT signed with AUTH_JWT_SECRET; the `sub`
# claim becomes the list owner
curl -XPOST localhost:8787/todos -H "Authorization: Bearer $JWT" -d '{"title":"Buy milk"}'
# {"id":"1","title":"Buy milk","done":false}  (HTTP 201)

curl localhost:8787/todos -H "Authorization: Bearer $JWT"
# [{"id":"1","title":"Buy milk","done":false}]

curl -XPOST localhost:8787/todos/1/complete -H "Authorization: Bearer $JWT"
# (HTTP 204)
```

Deploy with `npx wrangler deploy`; set the real secret with
`npx wrangler secret put AUTH_JWT_SECRET`.

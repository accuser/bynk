---
level: patch
changelog: "An agent handler can read its own key as `self.<key>` (#1818). It passed `bynk check` but never worked. The output cast the value to the key's *name* (`as id`), which `tsc` rejected, and the value was not the key anyway: `\"[object Object]\"` on the bundle target and the Durable Object's hex id on workers. Now an agent that reads its key is handed it: by its factory on the bundle target, and on workers with each call, encoded by the key type's codec and decoded by the Durable Object. Agents that don't read their key emit exactly as before"
---

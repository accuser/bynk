## Write your first context

Inside a `.bynk` file, type a snippet prefix and press <kbd>Tab</kbd> to scaffold
a construct — tab through the highlighted placeholders to fill it in:

- `context` / `commons` — a unit header
- `type` / `enum` — a record or sum type
- `capability` / `provides` — an effectful interface and its implementation
- `service` / `on http` / `on cron` — a service and its handlers
- `agent` — durable keyed state with handlers

As you type, the server reports errors inline and in the **Problems** panel,
offers completions and signature help, and shows inferred-type inlay hints.

Press <kbd>F12</kbd> to go to a definition, <kbd>Shift</kbd>+<kbd>F12</kbd> for
references, and <kbd>F2</kbd> to rename across the project.

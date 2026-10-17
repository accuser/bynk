---
level: patch
changelog: A Worker's fault handlers (`fetch`, which covers `/_bynk/call/` and `/_bynk/event/`, and the queue consumer) log the error's stack, `e instanceof Error ? (e.stack || String(e)) : e`, instead of the error as a value, so `wrangler tail` shows frames an uploaded source map can map back to `.bynk` lines; the Workers guide says so (#2053).
---

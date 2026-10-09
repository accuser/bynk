---
level: patch
changelog: "A Workers entry point now logs a fault before it answers `500` (#1825). An HTTP route, `on call` service, event delivery or WebSocket upgrade that faulted (an adapter that threw, a host exception, a failed cross-context call) was caught by a bare `catch` and left no log line, and being caught it was no uncaught exception the platform would record. The catch now runs `console.error` with the context, the dispatch (a route's pattern, `call <service>`, `event <service>`, `ws <service>`) and the error, never the request, then answers the same `500`. A boundary or rehydration fault's payload is no longer an enumerable property of the error, so logging the error doesn't print a callee's response body or an offending value"
---

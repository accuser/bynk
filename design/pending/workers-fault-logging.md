---
level: patch
changelog: "A Workers entry point now logs a fault before it answers `500` (#1825). An HTTP route or `on call` service that faulted (an adapter that threw, a host exception, a failed cross-context call) was caught by a bare `catch` and left no log line, and being caught it was no uncaught exception the platform would record. The catch now runs `console.error` with the context, the route's pattern (or `call <service>`), and the error, never the request, then answers the same bare `500`"
---

---
level: patch
changelog: Names the compiler generates from your declarations no longer collide with names you declare yourself. A capability's token, a provider's binding, a unit's surface factory and a message bundle's tables are now `__`-prefixed (`__ClockToken`, `__SystemClockProvider`, `__makeSurface`, `__messagesLocales`), and a bundle's generated `render` reaches `LocaleTag`, `Message`, `MessageArg` and `renderArg` under private aliases, so a `type ClockToken`, a `fn makeSurface` or a bundle's own `type Message` now compiles and type-checks (#1697)
---

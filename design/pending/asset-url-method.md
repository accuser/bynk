---
level: patch
changelog: 'A method on an asset''s `url` written on the chain (`sheet.url.startsWith("/")`) checks in a context and its test suites, as it already did in a commons. It was misread as a call into a context named `sheet.url` (`bynk.resolve.unconsumed_context`) (#1930)'
---

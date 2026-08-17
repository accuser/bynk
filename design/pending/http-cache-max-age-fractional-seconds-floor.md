---
level: minor
changelog: "`@cache`'s `maxAge` must now resolve to an exact whole number of seconds — a positive-but-fractional value (e.g. `500.milliseconds`, or `1500.milliseconds`, which resolves to a real but wrong `max-age=1`) previously type-checked cleanly and then silently dropped the sub-second remainder at emit time rather than honouring it, with no diagnostic anywhere in the pipeline; now rejected at check time with a new `bynk.http.cache_max_age_fractional_seconds` diagnostic (#1230)"
---

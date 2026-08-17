---
level: minor
changelog: "`@cache`'s `maxAge` must now resolve to at least one whole second — a positive-but-sub-second value (e.g. `500.milliseconds`) previously type-checked cleanly and then silently rounded down to `Cache-Control: max-age=0` at emit time, disabling the freshness window the annotation exists to declare, with no diagnostic anywhere in the pipeline; now rejected at check time with a new `bynk.http.cache_max_age_sub_second` diagnostic (#1230)"
---

---
level: patch
changelog: "A Worker whose `on call` boundary carries a consumed adapter's exported type now imports that adapter's codecs (#1845). Its `handlers.ts` built `__serialise_Result_Report_WeatherError` from `__serialise_Report` and `__serialise_WeatherError` without importing either, so `tsc` failed with TS2304. An adapter's types are recorded as a consumed context's, which never lend their codecs; a consumed adapter's now do, the same way a used commons' do, since the adapter is linked into each Worker that consumes it"
---

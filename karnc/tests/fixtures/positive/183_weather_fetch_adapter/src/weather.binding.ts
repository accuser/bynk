import type { Weather, Report } from "./weather.js";
import { WeatherError } from "./weather.js";
import type { Fetch, Secrets } from "./karn.js";
import { Method } from "./karn.js";
import { Ok, Err, None, Some, type Result } from "./runtime.js";

export class FetchWeather implements Weather {
  // v0.18 ([M]/[N]): outbound IO and the API key are capability
  // dependencies (karn.Fetch / karn.Secrets), not raw fetch + params.
  constructor(private deps: { Fetch: Fetch; Secrets: Secrets }) {}

  async current(city: string): Promise<Result<Report, WeatherError>> {
    const key = await this.deps.Secrets.get("WEATHER_API_KEY");
    const res = await this.deps.Fetch.send({
      method: Method.Get,
      url: `https://api.example.com/weather?city=${encodeURIComponent(city)}`,
      contentType: None,
      authorization: key.tag === "Some" ? Some(`Bearer ${key.value}`) : None,
      body: None,
    });
    if (res.tag === "Err") {
      return Err(WeatherError.Upstream);
    }
    if (res.value.status === 404) {
      return Err(WeatherError.NotFound);
    }
    if (res.value.status !== 200) {
      return Err(WeatherError.Upstream);
    }
    const body = JSON.parse(res.value.body) as { temp_c: number; summary: string };
    return Ok({ tempC: Math.trunc(body.temp_c), summary: body.summary });
  }
}

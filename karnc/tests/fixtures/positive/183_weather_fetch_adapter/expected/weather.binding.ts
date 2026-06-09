import type { Weather, Report } from "./weather.js";
import { WeatherError } from "./weather.js";
import { Ok, Err, type Result } from "./runtime.js";

export class FetchWeather implements Weather {
  async current(city: string): Promise<Result<Report, WeatherError>> {
    const res = await fetch(
      `https://api.example.com/weather?city=${encodeURIComponent(city)}`,
    );
    if (res.status === 404) {
      return Err(WeatherError.NotFound);
    }
    if (!res.ok) {
      return Err(WeatherError.Upstream);
    }
    const body = (await res.json()) as { temp_c: number; summary: string };
    return Ok({ tempC: Math.trunc(body.temp_c), summary: body.summary });
  }
}

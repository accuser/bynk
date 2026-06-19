import type { Jwt } from "./tokens.js";
import type { Log } from "./logging.js";

export class StubJwt implements Jwt {
  constructor(private deps: { Log: Log }) {}
  async sign(sub: string): Promise<string> {
    await this.deps.Log.info(`signing for ${sub}`);
    return `token:${sub}`;
  }
}

import type { Slug } from "./normalise.js";

export class JavaScriptSlug implements Slug {
  async make(value: string): Promise<string> {
    return value.trim().toLowerCase().replace(/\s+/g, "-");
  }
}

export type Event = {
  readonly kind: string;
  readonly payload: unknown;
};

export type Plugin = {
  readonly name: string;
  handles(kind: string): boolean;
  run(event: Event): Promise<void>;
};

const installed: Plugin[] = [];

export function install(plugin: Plugin): () => void {
  installed.push(plugin);

  return () => {
    const index = installed.indexOf(plugin);
    if (index >= 0) {
      installed.splice(index, 1);
    }
  };
}

export async function dispatch(event: Event): Promise<void> {
  for (const plugin of installed) {
    if (plugin.handles(event.kind)) {
      await plugin.run(event);
    }
  }
}

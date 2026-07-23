type BasketId = string;
type Sku = string;

type BasketState = {
  readonly lines: Readonly<Record<Sku, number>>;
  readonly note: string | null;
  readonly revision: number;
};

interface BasketStore {
  load(id: BasketId): Promise<BasketState | undefined>;
  save(id: BasketId, state: BasketState): Promise<void>;
}

declare const store: BasketStore;

function emptyBasket(): BasketState {
  return { lines: {}, note: null, revision: 0 };
}

export async function setLine(
  id: BasketId,
  sku: Sku,
  quantity: number,
): Promise<void> {
  const before = (await store.load(id)) ?? emptyBasket();
  await store.save(id, {
    ...before,
    lines: { ...before.lines, [sku]: quantity },
    revision: before.revision + 1,
  });
}

export async function leaveNote(
  id: BasketId,
  message: string,
): Promise<void> {
  const before = (await store.load(id)) ?? emptyBasket();
  await store.save(id, { ...before, note: message });
}

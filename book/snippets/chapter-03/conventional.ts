type Order = { id: string; cents: number };
type PlacedOrder = { id: string; authorisation: string };

declare function findOrder(id: string): Promise<Order | undefined>;

// Rejects with PaymentDeclined or ProviderUnavailable.
declare function authorise(cents: number): Promise<string>;

declare function save(order: PlacedOrder): Promise<void>;

export async function placeOrder(id: string): Promise<PlacedOrder | undefined> {
  const order = await findOrder(id);
  if (!order) return undefined;

  const authorisation = await authorise(order.cents);
  const placed = { id: order.id, authorisation };

  await save(placed);
  return placed;
}

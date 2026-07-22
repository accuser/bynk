import { authorise } from "../payment/authorise.js";
import { orders } from "../storage/orders.js";

export async function placeOrder(id: string, cents: number) {
  const payment = await authorise(cents);

  if (!payment.ok) {
    return { ok: false, reason: "payment-declined" };
  }

  await orders.insert({ id, cents, status: "placed" });
  return { ok: true, id };
}

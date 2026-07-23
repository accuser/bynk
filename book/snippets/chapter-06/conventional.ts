type OrderStatus = "Draft" | "Placed" | "Paid" | "Cancelled";

type OrderState = {
  readonly status: OrderStatus;
  readonly paymentRef: string | null;
};

type OrderError = "ExpectedPlaced" | "AlreadyPaid" | "AlreadyCancelled";
type UpdateResult =
  | { ok: true; order: OrderState }
  | { ok: false; error: OrderError };

export function pay(order: OrderState, paymentRef: string): UpdateResult {
  switch (order.status) {
    case "Placed":
      return {
        ok: true,
        order: { status: "Paid", paymentRef },
      };
    case "Paid":
      return { ok: false, error: "AlreadyPaid" };
    case "Cancelled":
      return { ok: false, error: "AlreadyCancelled" };
    case "Draft":
      return { ok: false, error: "ExpectedPlaced" };
  }
}

export function restoreStatus(
  order: OrderState,
  status: OrderStatus,
): OrderState {
  return { ...order, status };
}

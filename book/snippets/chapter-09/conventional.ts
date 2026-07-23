type Order = {
  readonly id: string;
  readonly amount: number;
};

type Bank = {
  authorise(amount: number): Promise<boolean>;
};

type Orders = {
  save(order: Order): Promise<void>;
};

export async function placeOrder(
  bank: Bank,
  orders: Orders,
  order: Order,
): Promise<"placed" | "rejected"> {
  if (!(await bank.authorise(order.amount))) {
    return "rejected";
  }

  await orders.save(order);
  return "placed";
}

export async function testLargeOrderIsRejected(): Promise<void> {
  const saved: Order[] = [];

  const bank: Bank = {
    authorise: async (amount) => amount <= 10_000,
  };
  const orders: Orders = {
    save: async (order) => {
      saved.push(order);
    },
  };

  const result = await placeOrder(
    bank,
    orders,
    { id: "order-1", amount: 20_000 },
  );

  if (result !== "rejected" || saved.length !== 0) {
    throw new Error("expected rejection without persistence");
  }
}

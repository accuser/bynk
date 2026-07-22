type OrderLine = {
  orderId: string;
  customerId: string;
  quantity: number;
};

export function acceptLine(line: OrderLine): OrderLine {
  if (!Number.isInteger(line.quantity) || line.quantity < 1 || line.quantity > 100) {
    throw new Error("quantity must be between 1 and 100");
  }

  return line;
}

declare function reserve(
  orderId: string,
  customerId: string,
  quantity: number,
): void;

const line = acceptLine({ orderId: "ord-42", customerId: "cust-7", quantity: 2 });

reserve(line.customerId, line.orderId, line.quantity); // still compiles

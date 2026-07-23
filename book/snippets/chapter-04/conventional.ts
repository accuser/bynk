type PaymentError = "Declined" | "ProviderUnavailable";
type PaymentResult =
  | { ok: true; reference: string }
  | { ok: false; error: PaymentError };

declare const bank: {
  charge(cents: number): Promise<PaymentResult>;
};

declare const audit: {
  authorised(reference: string): Promise<void>;
};

export async function authorise(cents: number): Promise<PaymentResult> {
  const outcome = await bank.charge(cents);

  if (outcome.ok) {
    await audit.authorised(outcome.reference);
  }

  return outcome;
}

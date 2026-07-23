type CustomerId = string;

type Principal = {
  readonly id: CustomerId;
  readonly claims: ReadonlySet<string>;
};

type HttpRequest = {
  readonly params: Readonly<Record<string, string>>;
};

type AuthenticatedRequest = HttpRequest & {
  readonly principal: Principal;
};

type HttpResponse<T> = {
  readonly status: number;
  readonly body: T;
};

type Handler<T> = (request: HttpRequest) => Promise<HttpResponse<T>>;

declare function requireCustomer<T>(
  handler: (request: AuthenticatedRequest) => Promise<HttpResponse<T>>,
): Handler<T>;

declare const baskets: {
  load(owner: CustomerId): Promise<{ readonly itemCount: number }>;
};

export const getBasket = requireCustomer(async (request) => {
  if (!request.principal.claims.has("basket:read")) {
    return { status: 403, body: { itemCount: 0 } };
  }

  const basket = await baskets.load(request.params.owner);
  return { status: 200, body: basket };
});

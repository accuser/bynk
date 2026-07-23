type UserId = string;

type Principal = {
  readonly id: UserId;
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

declare function requireUser<T>(
  handler: (request: AuthenticatedRequest) => Promise<HttpResponse<T>>,
): Handler<T>;

declare const baskets: {
  load(owner: UserId): Promise<{ readonly itemCount: number }>;
};

export const getBasket = requireUser(async (request) => {
  if (!request.principal.claims.has("basket:read")) {
    return { status: 403, body: { itemCount: 0 } };
  }

  const basket = await baskets.load(request.params.owner);
  return { status: 200, body: basket };
});

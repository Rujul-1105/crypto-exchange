/**
 * REST client for the CEX API server. Returns typed JSON where possible;
 * falls back to text on parse errors so the UI can surface them.
 */

const BASE = process.env.NEXT_PUBLIC_API_BASE ?? "";

async function fetchJson<T>(url: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${BASE}${url}`, {
    ...init,
    headers: {
      "content-type": "application/json",
      ...(init?.headers ?? {}),
    },
  });
  if (!res.ok) {
    const body = await res.text().catch(() => "");
    throw new ApiError(res.status, body || res.statusText);
  }
  return (await res.json()) as T;
}

export class ApiError extends Error {
  constructor(public status: number, public body: string) {
    super(`API ${status}: ${body}`);
  }
}

export type Side = "buy" | "sell";
export type OrderType =
  | { type: "limit"; price: string }
  | { type: "market" }
  | { type: "stop"; trigger: string }
  | { type: "stop_limit"; trigger: string; limit: string };

export type PlaceOrderBody = {
  symbol: string;
  side: Side;
  tif?: "gtc" | "ioc";
  quantity: string;
} & OrderType;

export type DepthLevel = [price: string, quantity: string];
export type DepthSnapshot = {
  symbol: string;
  bids: DepthLevel[];
  asks: DepthLevel[];
  last_trade_price: string | null;
};

export type Trade = {
  id: number;
  symbol: string;
  price: string;
  quantity: string;
  buy_order_id: number;
  sell_order_id: number;
  taker_side: Side;
  /** Unix milliseconds — matches the `Timestamp` type on the backend. */
  timestamp: number;
};

export type Candle = {
  symbol: string;
  interval: string;
  open_ts: number;
  close_ts: number;
  open: string;
  high: string;
  low: string;
  close: string;
  volume: string;
};

export const api = {
  symbols: () => fetchJson<string[]>(`/api/symbols`),
  depth: (symbol: string, depth = 20) =>
    fetchJson<DepthSnapshot>(`/api/orderbook/${symbol}?depth=${depth}`),
  trades: (symbol: string, limit = 50) =>
    fetchJson<Trade[]>(`/api/trades/${symbol}?limit=${limit}`),
  candles: (symbol: string, interval: string, limit = 500) =>
    fetchJson<Candle[]>(`/api/candles/${symbol}/${interval}?limit=${limit}`),
  authNonce: (pubkey: string) =>
    fetchJson<{ nonce: string; message: string }>(`/api/auth/nonce`, {
      method: "POST",
      body: JSON.stringify({ pubkey }),
    }),
  authVerify: (pubkey: string, nonce: string, signature: string) =>
    fetchJson<{ jwt: string; pubkey: string; expires_at_unix_ms: number }>(
      `/api/auth/verify`,
      {
        method: "POST",
        body: JSON.stringify({ pubkey, nonce, signature }),
      },
    ),
  placeOrder: (body: PlaceOrderBody, jwt: string) =>
    fetchJson<{ stream_id: string }>(`/api/orders`, {
      method: "POST",
      headers: { authorization: `Bearer ${jwt}` },
      body: JSON.stringify(body),
    }),
  cancelOrder: (id: number, jwt: string) =>
    fetchJson<{ stream_id: string }>(`/api/orders/${id}`, {
      method: "DELETE",
      headers: { authorization: `Bearer ${jwt}` },
    }),
  balances: (jwt: string) =>
    fetchJson<{ sol: string; usdc: string }>(`/api/balances`, {
      headers: { authorization: `Bearer ${jwt}` },
    }),
};

"use client";

import { useState } from "react";
import { useMutation } from "@tanstack/react-query";
import clsx from "clsx";

import { api } from "@/lib/api";
import { useSessionStore } from "@/stores/sessionStore";
import { useUiStore } from "@/stores/uiStore";

type Kind = "limit" | "market" | "stop";

export function OrderForm() {
  const jwt = useSessionStore((s) => s.jwt);
  const symbol = useUiStore((s) => s.symbol);
  const side = useUiStore((s) => s.orderSide);
  const setSide = useUiStore((s) => s.setOrderSide);

  const [kind, setKind] = useState<Kind>("limit");
  const [price, setPrice] = useState("");
  const [trigger, setTrigger] = useState("");
  const [quantity, setQuantity] = useState("");
  const [tif, setTif] = useState<"gtc" | "ioc">("gtc");
  const [feedback, setFeedback] = useState<{ kind: "error" | "info"; text: string } | null>(null);

  const mutation = useMutation({
    mutationFn: async () => {
      if (!jwt) throw new Error("Sign in to place an order.");
      const body = {
        symbol,
        side,
        tif,
        quantity,
        ...(kind === "limit" ? { type: "limit", price } : {}),
        ...(kind === "market" ? { type: "market" } : {}),
        ...(kind === "stop" ? { type: "stop_limit", trigger, limit: price } : {}),
      };
      return api.placeOrder(body as any, jwt);
    },
    onSuccess: () => {
      setFeedback({ kind: "info", text: "Order queued." });
      setQuantity("");
    },
    onError: (e: Error) => setFeedback({ kind: "error", text: e.message }),
  });

  return (
    <section aria-label="Order form" className="rounded-xl border border-line bg-bg-surface">
      <header className="border-b border-line px-4 py-3">
        <div className="inline-flex w-full rounded-full bg-bg-raised p-1 text-xs">
          <SideTab active={side === "buy"} side="buy" onClick={() => setSide("buy")} />
          <SideTab active={side === "sell"} side="sell" onClick={() => setSide("sell")} />
        </div>
      </header>

      <div className="space-y-3 p-4">
        <div className="inline-flex w-full rounded-full bg-bg-raised p-1 text-xs">
          {(["limit", "market", "stop"] as Kind[]).map((k) => (
            <button
              key={k}
              onClick={() => setKind(k)}
              className={clsx(
                "flex-1 rounded-full px-3 py-1.5 capitalize transition",
                kind === k ? "bg-bg-surface text-text-primary" : "text-text-muted hover:text-text-primary",
              )}
            >
              {k}
            </button>
          ))}
        </div>

        {kind === "limit" && (
          <NumberField label="Limit price" value={price} onChange={setPrice} suffix="USDC" />
        )}
        {kind === "stop" && (
          <>
            <NumberField label="Trigger" value={trigger} onChange={setTrigger} suffix="USDC" />
            <NumberField label="Limit" value={price} onChange={setPrice} suffix="USDC" />
          </>
        )}
        <NumberField label="Quantity" value={quantity} onChange={setQuantity} suffix="SOL" />

        <div className="inline-flex w-full rounded-full bg-bg-raised p-1 text-xs">
          {(["gtc", "ioc"] as const).map((t) => (
            <button
              key={t}
              onClick={() => setTif(t)}
              className={clsx(
                "flex-1 rounded-full px-3 py-1.5 uppercase tracking-wide transition",
                tif === t ? "bg-bg-surface text-text-primary" : "text-text-muted hover:text-text-primary",
              )}
            >
              {t}
            </button>
          ))}
        </div>

        {feedback && (
          <p
            className={clsx(
              "text-xs",
              feedback.kind === "error" ? "text-rose-400" : "text-emerald-400",
            )}
          >
            {feedback.text}
          </p>
        )}

        <button
          onClick={() => mutation.mutate()}
          disabled={mutation.isPending || !quantity}
          className={clsx(
            "flex w-full items-center justify-center rounded-full px-5 py-3 text-sm font-medium transition active:scale-[0.99] disabled:cursor-not-allowed disabled:bg-line disabled:text-text-muted",
            side === "buy"
              ? "bg-emerald-500 text-bg-base hover:-translate-y-px hover:bg-emerald-400"
              : "bg-rose-500 text-white hover:-translate-y-px hover:bg-rose-400",
          )}
        >
          {mutation.isPending ? "Submitting" : `${side === "buy" ? "Buy" : "Sell"} ${symbol.split("-")[0] ?? ""}`}
        </button>
      </div>
    </section>
  );
}

function SideTab({ active, side, onClick }: { active: boolean; side: "buy" | "sell"; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      aria-pressed={active}
      className={clsx(
        "flex-1 rounded-full px-3 py-1.5 text-xs font-medium uppercase tracking-wide transition",
        active
          ? side === "buy"
            ? "bg-emerald-500 text-bg-base"
            : "bg-rose-500 text-white"
          : "text-text-muted hover:text-text-primary",
      )}
    >
      {side}
    </button>
  );
}

function NumberField({ label, value, onChange, suffix }: { label: string; value: string; onChange: (s: string) => void; suffix: string }) {
  return (
    <label className="block">
      <span className="text-[11px] uppercase tracking-[0.14em] text-text-dim">{label}</span>
      <div className="mt-1 flex items-baseline gap-2 rounded-lg border border-line bg-bg-base px-3 py-2 focus-within:border-line-strong">
        <input
          inputMode="decimal"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder="0.00"
          className="w-full bg-transparent font-mono text-sm text-text-primary outline-none placeholder:text-text-dim"
        />
        <span className="font-mono text-xs text-text-muted">{suffix}</span>
      </div>
    </label>
  );
}

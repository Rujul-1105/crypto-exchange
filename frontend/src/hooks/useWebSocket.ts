"use client";

import { useCallback, useEffect, useRef, useState } from "react";

type Status = "connecting" | "open" | "closed";

type ClientMsg = { op: "subscribe" | "unsubscribe" | "ping"; channel?: string; ts?: number };

/** Reconnecting WS client with subscription tracking + event-replay cursor. */
export function useWebSocket(url: string, onMessage: (msg: unknown) => void) {
  const [status, setStatus] = useState<Status>("connecting");
  const wsRef = useRef<WebSocket | null>(null);
  const onMessageRef = useRef(onMessage);
  const subsRef = useRef<Set<string>>(new Set());
  const pendingRef = useRef<Set<string>>(new Set());
  const baseUrlRef = useRef(url);
  // Last stream id the server told us about. The server includes
  // `stream_id` on every forwarded event so we can use it as a replay
  // cursor on reconnect — see `?last_event_id=...` in connect().
  const lastEventIdRef = useRef<string | null>(null);

  useEffect(() => {
    onMessageRef.current = onMessage;
  }, [onMessage]);

  useEffect(() => {
    baseUrlRef.current = url;
  }, [url]);

  const flushPending = useCallback(() => {
    const ws = wsRef.current;
    if (!ws || ws.readyState !== WebSocket.OPEN) return;
    for (const ch of pendingRef.current) {
      ws.send(JSON.stringify({ op: "subscribe", channel: ch }));
      subsRef.current.add(ch);
    }
    pendingRef.current.clear();
  }, []);

  useEffect(() => {
    if (!baseUrlRef.current) return;
    let cancelled = false;
    let attempt = 0;
    let timer: ReturnType<typeof setTimeout> | null = null;

    const connect = () => {
      if (cancelled) return;
      setStatus("connecting");
      // Append `?last_event_id=<id>` on reconnect so the server replays
      // events the client missed while disconnected. On the first connect
      // `lastEventIdRef.current` is null and the server falls back to "0"
      // (full replay from the start of the stream, bounded by XTRIM MAXLEN).
      const base = baseUrlRef.current;
      const tail = lastEventIdRef.current
        ? `?last_event_id=${encodeURIComponent(lastEventIdRef.current)}`
        : "";
      const ws = new WebSocket(base + tail);
      wsRef.current = ws;
      ws.onopen = () => {
        if (cancelled) return;
        setStatus("open");
        attempt = 0;
        // Replay all active subscriptions + queue any pending ones.
        const all = new Set<string>([...subsRef.current, ...pendingRef.current]);
        for (const ch of all) {
          ws.send(JSON.stringify({ op: "subscribe", channel: ch }));
        }
        pendingRef.current.clear();
        subsRef.current = all;
      };
      ws.onmessage = (ev) => {
        if (cancelled) return;
        try {
          const parsed = JSON.parse(ev.data);
          // Record the server-assigned stream id so the next reconnect
          // can request events from this point forward.
          if (parsed && typeof parsed === "object" && typeof parsed.stream_id === "string") {
            lastEventIdRef.current = parsed.stream_id;
          }
          onMessageRef.current(parsed);
        } catch (e) {
          console.warn("ws: bad json", e);
        }
      };
      ws.onclose = () => {
        if (cancelled) return;
        setStatus("closed");
        const delay = Math.min(30_000, 500 * 2 ** attempt++);
        timer = setTimeout(connect, delay);
      };
      ws.onerror = () => ws.close();
    };

    connect();
    return () => {
      cancelled = true;
      if (timer) clearTimeout(timer);
      wsRef.current?.close();
    };
  }, [url]);

  const subscribe = useCallback((channel: string) => {
    pendingRef.current.add(channel);
    flushPending();
  }, [flushPending]);

  const unsubscribe = useCallback((channel: string) => {
    subsRef.current.delete(channel);
    pendingRef.current.delete(channel);
    const ws = wsRef.current;
    if (ws && ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify({ op: "unsubscribe", channel }));
    }
  }, []);

  const send = useCallback((msg: ClientMsg) => {
    const ws = wsRef.current;
    if (ws && ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify(msg));
    }
  }, []);

  return { status, subscribe, unsubscribe, send };
}
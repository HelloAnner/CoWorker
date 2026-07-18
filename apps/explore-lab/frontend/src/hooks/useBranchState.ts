import { useCallback, useEffect, useRef, useState } from 'react';
import { getBranchState, getEventsStreamUrl } from '../api/client';
import type { BranchState } from '../api/types';

const REFRESH_THROTTLE_MS = 1000;
const FALLBACK_INTERVAL_MS = 5000;

/** 先取一次权威 /state，再用 branch_runner 的 SSE 作为状态失效通知。
 * SSE 断线时退回低频兜底刷新，避免 idle 分支持续高频请求。*/
export function useBranchState(controlPort: number | null) {
  const [state, setState] = useState<BranchState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const requestSeqRef = useRef(0);
  const lastRefreshAtRef = useRef(0);
  const throttleTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const fallbackTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const clearThrottleTimer = useCallback(() => {
    if (throttleTimerRef.current !== null) {
      clearTimeout(throttleTimerRef.current);
      throttleTimerRef.current = null;
    }
  }, []);

  const clearFallbackTimer = useCallback(() => {
    if (fallbackTimerRef.current !== null) {
      clearInterval(fallbackTimerRef.current);
      fallbackTimerRef.current = null;
    }
  }, []);

  const refresh = useCallback(async () => {
    if (controlPort === null) {
      requestSeqRef.current += 1;
      setState(null);
      setError(null);
      return null;
    }

    const seq = ++requestSeqRef.current;
    try {
      const next = await getBranchState(controlPort);
      if (seq !== requestSeqRef.current) return null;
      lastRefreshAtRef.current = Date.now();
      setState(next);
      setError(null);
      return next;
    } catch (e) {
      if (seq !== requestSeqRef.current) return null;
      setError(e instanceof Error ? e.message : '分支状态拉取失败');
      return null;
    }
  }, [controlPort]);

  const scheduleRefresh = useCallback(() => {
    if (controlPort === null || throttleTimerRef.current !== null) return;
    const elapsed = Date.now() - lastRefreshAtRef.current;
    const delay = Math.max(0, REFRESH_THROTTLE_MS - elapsed);
    throttleTimerRef.current = setTimeout(() => {
      throttleTimerRef.current = null;
      void refresh();
    }, delay);
  }, [controlPort, refresh]);

  const startFallbackRefresh = useCallback(() => {
    if (fallbackTimerRef.current !== null) return;
    fallbackTimerRef.current = setInterval(() => {
      void refresh();
    }, FALLBACK_INTERVAL_MS);
  }, [refresh]);

  useEffect(() => {
    if (controlPort === null) {
      setState(null);
      setError(null);
      return;
    }

    void refresh();
    const events = new EventSource(getEventsStreamUrl(controlPort));
    let acceptingStreamEvents = false;
    let replayTimer: ReturnType<typeof setTimeout> | null = null;
    const ignoreInitialReplay = () => {
      acceptingStreamEvents = false;
      if (replayTimer !== null) clearTimeout(replayTimer);
      replayTimer = setTimeout(() => {
        acceptingStreamEvents = true;
        replayTimer = null;
      }, REFRESH_THROTTLE_MS);
    };

    events.onopen = () => {
      clearFallbackTimer();
      ignoreInitialReplay();
    };
    events.onmessage = () => {
      if (!acceptingStreamEvents) return;
      scheduleRefresh();
    };
    events.onerror = () => {
      startFallbackRefresh();
    };

    return () => {
      requestSeqRef.current += 1;
      events.close();
      if (replayTimer !== null) clearTimeout(replayTimer);
      clearThrottleTimer();
      clearFallbackTimer();
    };
  }, [
    clearFallbackTimer,
    clearThrottleTimer,
    controlPort,
    refresh,
    scheduleRefresh,
    startFallbackRefresh,
  ]);

  return { state, error, refresh };
}

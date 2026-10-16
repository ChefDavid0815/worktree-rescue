import { useEffect, useState } from "react";

/** Wall clock rounded down to `step` ms, re-rendering once per step. Keeps
 * time-based query keys stable between renders. */
export function useNow(step = 60_000): number {
  const round = () => Math.floor(Date.now() / step) * step + step;
  const [now, setNow] = useState(round);
  useEffect(() => {
    const id = setInterval(() => setNow(round()), Math.min(step, 30_000));
    return () => clearInterval(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [step]);
  return now;
}

import { useCallback, useEffect, useRef, useState } from "react";

export function useToast(timeout = 2200) {
  const [message, setMessage] = useState<string | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);

  const show = useCallback(
    (msg: string) => {
      setMessage(msg);
      clearTimeout(timer.current);
      timer.current = setTimeout(() => setMessage(null), timeout);
    },
    [timeout],
  );

  useEffect(() => () => clearTimeout(timer.current), []);

  return { message, show };
}

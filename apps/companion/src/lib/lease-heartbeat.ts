const INTERVAL_MS = 100;

export function startLeaseHeartbeat(
  renew: () => Promise<unknown>,
  current: () => boolean,
  failed: (error: unknown) => void,
  now = () => performance.now(),
): () => void {
  let stopped = false;
  let timer: ReturnType<typeof setTimeout>;
  const schedule = (deadline: number) => {
    timer = setTimeout(send, Math.max(0, Math.ceil(deadline - now())));
  };
  async function send() {
    if (stopped || !current()) return;
    const deadline = now() + INTERVAL_MS;
    try {
      await renew();
    } catch (error) {
      if (!stopped && current()) failed(error);
      return;
    }
    if (!stopped && current()) schedule(deadline);
  }
  schedule(now() + INTERVAL_MS);
  return () => {
    stopped = true;
    clearTimeout(timer);
  };
}

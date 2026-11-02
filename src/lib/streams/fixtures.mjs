export function delivery(overrides = {}, deliveryId = '1') {
  return { subscriptionId: 'token', delivery: deliveryId, frame: {
    kind: 'snapshot', topic: 'preferences', source: 'service', session: 'session',
    generation: '1', revision: '1', capturedUs: '123', clockId: 'monotonic',
    continuity: 'baseline', payload: { theme: 'dark' }, ...overrides,
  } };
}
export const turn = () => new Promise((resolve) => setImmediate(resolve));
export function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

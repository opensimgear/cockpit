export function openDcsTelemetryReaders(cdp, browser) {
  return cdp.evaluate(`(async () => {
    const {SharedObservations} = await import('/src/lib/streams/shared.ts');
    const cancel = new AbortController();
    const originalFetch = window.fetch;
    const trace = window.__dcsSlowReader = {
      fast: 0, slow: 0, error: null,
      close() {
        cancel.abort();
        trace.fastReader.close();
        window.fetch = originalFetch;
      },
    };
    let transport;
    if (${browser}) {
      const {createBrowserObservationTransport} = await import('/src/lib/streams/browser.ts');
      // Hold only the first receipt for the slow reader on the real shared connection.
      window.fetch = (url, init) => {
        const receipt = typeof url === 'string' && url.endsWith('/api/observe/ack') && JSON.parse(init.body);
        if (receipt && receipt.subscriptionId === trace.slowId && receipt.delivery === '1') {
          return new Promise((resolve, reject) => {
            trace.release = () => originalFetch.call(window, url, init).then(resolve, reject);
            init.signal?.addEventListener('abort', () => reject(init.signal.reason), {once: true});
          });
        }
        return originalFetch.call(window, url, init);
      };
      transport = createBrowserObservationTransport();
    } else {
      const {createNativeObservationTransport} = await import('/src/lib/streams/native.ts');
      transport = createNativeObservationTransport();
    }
    trace.fastReader = new SharedObservations(transport);
    transport.open('games.telemetry', {
      frame(envelope, ack) {
        trace.slow++;
        trace.slowId = envelope.subscriptionId;
        if (${browser} || trace.slow > 1) void ack();
        else trace.release = ack;
      },
      error(error) { trace.error = error; },
    }, cancel.signal);
    trace.fastReader.subscribe('games.telemetry', state => {
      if (state.status === 'ready') trace.fast++;
    });
  })()`);
}

// Constant-space observer for real compiled browser-panel feedback in the
// long-running DCS Feature Lab soak. It listens to existing SharedWorker
// messages without acknowledging, delaying, or replacing panel traffic.

const OBSERVER_KEY = '__dcsSoakPanelObserver';

export async function installSoakPanel(page) {
  await page.addInitScript(({observerKey}) => {
    const current = window[observerKey];
    if (current?.installed) return;

    const state = {
      installed: true,
      workerCount: 0,
      feedbackFrames: 0,
      freshValues: 0,
      nonFreshValues: 0,
      nonFreshAfterFirstFresh: 0,
      firstFreshSeen: false,
      statuses: Object.create(null),
      identityChanges: 0,
      bindingGenerationAdvances: 0,
      bindingGenerationRegressions: 0,
      sequenceRegressions: 0,
      assignmentEvents: 0,
      feedbackClosures: 0,
      workerErrors: 0,
      paintCount: 0,
      invalidCounters: 0,
      diagnostics: [],
      identity: null,
      identityKey: null,
      bindingGeneration: null,
      sequence: null,
      latest: null,
      latestFresh: null,
      latestPaint: null,
      latestPaintTransition: null,
      maxSourceAgeMs: null,
    };

    const note = (kind, details = {}) => {
      state.diagnostics.push({at: performance.now(), kind, ...details});
      if (state.diagnostics.length > 8) state.diagnostics.shift();
    };
    const counter = value => {
      if (typeof value !== 'string' || !/^(0|[1-9][0-9]*)$/.test(value)) return null;
      try { return BigInt(value); } catch { return null; }
    };
    const observe = message => {
      if (!message || typeof message !== 'object') return;
      if (message.type === 'assignments') {
        state.assignmentEvents++;
        note('assignments-event');
        return;
      }
      if (message.type === 'feedback-closed' || message.type === 'closed') {
        state.feedbackClosures++;
        note(message.type);
        return;
      }
      if (message.type !== 'feedback' || !message.reply?.feedback) return;

      const feedback = message.reply.feedback;
      state.feedbackFrames++;
      const identity = Object.fromEntries(
        ['clientId', 'sessionId', 'generation', 'controllerInstanceId', 'publicationHash']
          .map(key => [key, feedback[key] ?? null]),
      );
      const identityKey = JSON.stringify(identity);
      const bindingGeneration = counter(feedback.bindingGeneration);
      const sequence = counter(feedback.sequence);
      if (bindingGeneration === null || sequence === null) {
        state.invalidCounters++;
        note('invalid-counter', {
          bindingGeneration: feedback.bindingGeneration ?? null,
          sequence: feedback.sequence ?? null,
        });
      }
      if (state.identityKey !== null && state.identityKey !== identityKey) {
        state.identityChanges++;
        note('identity-change', {previous: state.identity, next: identity});
      }
      if (identityKey === state.identityKey && bindingGeneration !== null
          && state.bindingGeneration !== null) {
        if (bindingGeneration < state.bindingGeneration) {
          state.bindingGenerationRegressions++;
          note('binding-generation-regression', {
            previous: state.bindingGeneration.toString(),
            next: bindingGeneration.toString(),
          });
        } else if (bindingGeneration > state.bindingGeneration) {
          state.bindingGenerationAdvances++;
          note('binding-generation-advance', {
            previous: state.bindingGeneration.toString(),
            next: bindingGeneration.toString(),
          });
        }
      }
      if (sequence !== null && state.sequence !== null
          && identityKey === state.identityKey
          && bindingGeneration !== null
          && bindingGeneration === state.bindingGeneration
          && sequence < state.sequence) {
        state.sequenceRegressions++;
        note('sequence-regression', {
          bindingGeneration: bindingGeneration.toString(),
          previous: state.sequence.toString(),
          next: sequence.toString(),
        });
      }
      state.identity = identity;
      state.identityKey = identityKey;
      if (bindingGeneration !== null) state.bindingGeneration = bindingGeneration;
      if (sequence !== null) state.sequence = sequence;

      const sample = feedback.values?.find(value =>
        value?.channelId === 'flight-channel-0' && value?.outputId === 'flight-output-0');
      if (!sample) return;
      const status = typeof sample.status === 'string' ? sample.status : 'invalid';
      state.statuses[status] = (state.statuses[status] ?? 0) + 1;
      if (status === 'fresh') {
        state.freshValues++;
        state.firstFreshSeen = true;
      } else {
        state.nonFreshValues++;
        if (state.firstFreshSeen) state.nonFreshAfterFirstFresh++;
      }
      if (Number.isFinite(sample.sourceAgeMs)) {
        state.maxSourceAgeMs = state.maxSourceAgeMs === null
          ? sample.sourceAgeMs : Math.max(state.maxSourceAgeMs, sample.sourceAgeMs);
      }
      state.latest = {
        channelId: sample.channelId,
        outputId: sample.outputId,
        status,
        value: sample.value ?? null,
        provenance: sample.provenance ?? null,
        sourceKind: sample.sourceKind ?? null,
        sourceAgeMs: Number.isFinite(sample.sourceAgeMs) ? sample.sourceAgeMs : null,
        unit: sample.unit ?? null,
        sequence: feedback.sequence ?? null,
        bindingGeneration: feedback.bindingGeneration ?? null,
        mode: feedback.mode ?? null,
      };
      if (status === 'fresh' && typeof sample.value === 'boolean') {
        state.latestFresh = {value: sample.value, sequence: feedback.sequence ?? null,
          bindingGeneration: feedback.bindingGeneration ?? null, at: performance.now()};
      }
      if (status !== 'fresh') note('non-fresh-feedback', {status, sourceAgeMs: state.latest.sourceAgeMs});
    };

    Object.defineProperty(window, observerKey, {value: state, configurable: false});
    const OriginalSharedWorker = window.SharedWorker;
    if (typeof OriginalSharedWorker !== 'function') {
      note('shared-worker-unavailable');
      return;
    }
    window.SharedWorker = new Proxy(OriginalSharedWorker, {
      construct(target, args, newTarget) {
        const worker = Reflect.construct(target, args, newTarget);
        state.workerCount++;
        worker.port.addEventListener('message', event => observe(event.data));
        worker.port.addEventListener('messageerror', () => {
          state.workerErrors++;
          note('message-error');
        });
        return worker;
      },
    });

    const context = window.CanvasRenderingContext2D?.prototype;
    if (context?.putImageData) {
      const originalPutImageData = context.putImageData;
      context.putImageData = function(image, ...args) {
        const result = originalPutImageData.call(this, image, ...args);
        if (this.canvas.matches('[data-widget-id="probe"] canvas')) {
          const [red, green, blue, alpha] = image.data;
          const lit = red > 240 && green < 50 && blue < 50 && alpha === 255;
          state.paintCount++;
          const paint = {
            at: performance.now(),
            pixel: [red, green, blue, alpha],
            lit,
            value: lit,
          };
          if (state.latestPaint?.lit !== lit) state.latestPaintTransition = paint;
          state.latestPaint = paint;
        }
        return result;
      };
    } else {
      note('canvas-put-image-data-unavailable');
    }
  }, {observerKey: OBSERVER_KEY});
}

export async function readSoakPanel(page) {
  return page.evaluate(observerKey => {
    const state = window[observerKey];
    const statusNode = document.querySelector('.toolbar span.active');
    const panelStatus = statusNode?.textContent?.trim() ?? null;
    const errorNode = document.querySelector('[role="alert"], .toolbar .error');
    const memory = performance.memory;
    return {
      installed: state?.installed === true,
      documentReadyState: document.readyState,
      documentVisible: document.visibilityState === 'visible',
      panelStatus,
      panelReady: panelStatus === 'Connected · panel active'
        || panelStatus === 'Connected · feedback active · controls paused',
      panelErrorText: errorNode?.textContent?.trim() || null,
      canvasCount: document.querySelectorAll('canvas').length,
      paintCount: state?.paintCount ?? 0,
      latestPaint: state?.latestPaint ?? null,
      latestPaintTransition: state?.latestPaintTransition ?? null,
      workerCount: state?.workerCount ?? 0,
      feedbackFrames: state?.feedbackFrames ?? 0,
      freshValues: state?.freshValues ?? 0,
      nonFreshValues: state?.nonFreshValues ?? 0,
      nonFreshAfterFirstFresh: state?.nonFreshAfterFirstFresh ?? 0,
      firstFreshSeen: state?.firstFreshSeen ?? false,
      statuses: {...(state?.statuses ?? {})},
      identityChanges: state?.identityChanges ?? 0,
      bindingGenerationAdvances: state?.bindingGenerationAdvances ?? 0,
      bindingGenerationRegressions: state?.bindingGenerationRegressions ?? 0,
      sequenceRegressions: state?.sequenceRegressions ?? 0,
      assignmentEvents: state?.assignmentEvents ?? 0,
      feedbackClosures: state?.feedbackClosures ?? 0,
      workerErrors: state?.workerErrors ?? 0,
      invalidCounters: state?.invalidCounters ?? 0,
      identity: state?.identity ?? null,
      bindingGeneration: state?.bindingGeneration?.toString() ?? null,
      sequence: state?.sequence?.toString() ?? null,
      latest: state?.latest ?? null,
      latestFresh: state?.latestFresh ?? null,
      maxSourceAgeMs: state?.maxSourceAgeMs ?? null,
      diagnostics: state?.diagnostics?.slice() ?? [],
      jsHeap: memory ? {
        usedBytes: memory.usedJSHeapSize,
        totalBytes: memory.totalJSHeapSize,
        limitBytes: memory.jsHeapSizeLimit,
      } : null,
    };
  }, OBSERVER_KEY);
}

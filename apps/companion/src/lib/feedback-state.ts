import { validateFeedbackValues, type PortableUi } from '@opensimgear/companion-contracts';
import type { FeedbackSamples } from '@opensimgear/companion-ui';
import type { SessionIdentity } from './session-guard.ts';
const key = (identity: SessionIdentity) => JSON.stringify([identity.epoch, identity.sessionId, identity.generation, identity.instanceId, identity.publicationHash]);
/** Visual snapshots have their own revision fence and never call an input/controller API. */
export function createFeedbackView() {
  let identity = '', revision = -1, samples: FeedbackSamples = {};
  const snapshot = () => structuredClone(samples);
  return {
    snapshot,
    accept(expected: SessionIdentity, actual: SessionIdentity, nextRevision: number, values: unknown, ui: PortableUi, now: number): { accepted: boolean; error?: string } {
      if (key(expected) !== key(actual)) return { accepted: false, error: 'Feedback belongs to another controller session.' };
      if (!Number.isSafeInteger(nextRevision) || nextRevision < 0 || !Number.isFinite(now) || now < 0) return { accepted: false, error: 'Invalid feedback revision or clock.' };
      const nextIdentity = key(expected);
      if (identity === nextIdentity && nextRevision <= revision) return { accepted: false };
      const checked = validateFeedbackValues(values, ui);
      if (!checked.ok) return { accepted: false, error: checked.error };
      identity = nextIdentity; revision = nextRevision;
      samples = Object.fromEntries(checked.value.map(sample => [sample.channelId, { ...sample, receivedAtMs: now }]));
      return { accepted: true };
    },
    /** A reset cannot replay an already-seen snapshot; native resync supplies a new revision. */
    reset() { samples = {}; },
    disconnect() { samples = Object.fromEntries(Object.entries(samples).map(([channelId, sample]) => { const { value: _value, ...metadata } = sample; return [channelId, { ...metadata, status: 'disconnected' as const }]; })); },
    clear() { identity = ''; revision = -1; samples = {}; },
  };
}

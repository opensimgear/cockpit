export type PanelMode = 'local' | 'remote' | 'offline';
export interface SessionIdentity { epoch: number; sessionId: string; generation: string; instanceId: string; publicationHash: string }
interface RemoteShape { epoch: number; phase: string; publicationHash?: string; session?: Record<string, unknown> }
export function remoteIdentity(status: RemoteShape): SessionIdentity | null {
  const session = status.session;
  if (!Number.isSafeInteger(status.epoch) || status.epoch < 0 || !session || typeof session.sessionId !== 'string' || !session.sessionId || typeof session.generation !== 'string' || !/^[1-9][0-9]*$/.test(session.generation) || typeof session.controllerInstanceId !== 'string' || !session.controllerInstanceId || typeof status.publicationHash !== 'string' || !/^sha256:[a-f0-9]{64}$/.test(status.publicationHash)) return null;
  return { epoch: status.epoch, sessionId: session.sessionId, generation: session.generation, instanceId: session.controllerInstanceId, publicationHash: status.publicationHash };
}
const same = (a: SessionIdentity | null, b: SessionIdentity | null) => a !== null && b !== null && Object.keys(a).every(key => a[key as keyof SessionIdentity] === b[key as keyof SessionIdentity]);
/** Async responses carry a view epoch; lifecycle/selection changes cancel every outstanding operation. */
export function createSessionGuard() {
  let revision = 0, mode: PanelMode = 'local', selected = '', verified: SessionIdentity | null = null, armed = false, acceptingConnection = false;
  function invalidate(nextMode: PanelMode = mode, instanceId = selected) { revision++; mode = nextMode; selected = instanceId; verified = null; armed = false; acceptingConnection = false; return revision; }
  const current = (token: number) => token === revision;
  function matches(status: RemoteShape) { return acceptingConnection && mode === 'remote' && same(verified, remoteIdentity(status)) && selected === verified?.instanceId; }
  function canStart(status: RemoteShape, visible: boolean) { return visible && !armed && status.phase === 'ready' && matches(status); }
  return {
    capture: () => revision, current, invalidate, matches, canStart,
    connect(instanceId: string) { const token = invalidate('remote', instanceId); acceptingConnection = true; return token; },
    verify(token: number, status: RemoteShape, hash: string) { const identity = remoteIdentity(status); if (!current(token) || !acceptingConnection || mode !== 'remote' || !identity || identity.instanceId !== selected || identity.publicationHash !== hash || !['ready','armed'].includes(status.phase)) return false; verified = identity; return true; },
    beginStart(status: RemoteShape, visible: boolean) { if (!canStart(status, visible)) return null; revision++; return revision; },
    arm(token: number, status: RemoteShape, visible: boolean) { if (!current(token) || !canStart(status, visible)) return false; armed = true; return true; },
    snapshot: () => ({ revision, mode, selected, acceptingConnection, verified: verified ? { ...verified } : null, armed }),
  };
}

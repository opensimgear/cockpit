export type CockpitView = 'overview' | 'devices' | 'device' | 'profiles' | 'companions' | 'games' | 'activity' | 'debug' | 'preferences';
export type CompanionPage = { kind: 'main' } | { kind: 'companion' | 'panel' | 'widget'; id: string };
export interface CockpitRoute {
  view: CockpitView;
  componentId?: string;
  deviceId?: string;
  profileId?: string;
  companion?: CompanionPage;
  gameId?: string;
  settingsKey?: string;
}
type Entry = { index: number; route: CockpitRoute };
type HistoryHost = Pick<Window, 'history' | 'location' | 'addEventListener' | 'removeEventListener'>;
const views = new Set(['overview', 'devices', 'device', 'profiles', 'companions', 'games', 'activity', 'debug', 'preferences']);
const id = (value: unknown): value is string => typeof value === 'string' && value.length > 0 && value.length <= 1024;

export function cockpitRoute(value: unknown): CockpitRoute | null {
  if (!value || typeof value !== 'object') return null;
  const input = value as Record<string, unknown>;
  if (!views.has(String(input.view))) return null;
  const route: CockpitRoute = { view: input.view as CockpitView };
  for (const field of ['componentId', 'deviceId', 'profileId', 'gameId', 'settingsKey'] as const) {
    if (input[field] !== undefined) { if (!id(input[field])) return null; route[field] = input[field]; }
  }
  if (input.companion !== undefined) {
    const page = input.companion as CompanionPage;
    if (!page || typeof page !== 'object') return null;
    if (page.kind === 'main') route.companion = { kind: 'main' };
    else if ((page.kind === 'companion' || page.kind === 'panel' || page.kind === 'widget') && id(page.id)) route.companion = { kind: page.kind, id: page.id };
    else return null;
  }
  return route;
}
const equal = (a: CockpitRoute, b: CockpitRoute) => JSON.stringify(a) === JSON.stringify(b);
function entry(value: unknown): Entry | null {
  const candidate = (value as { osgCockpit?: Entry } | null)?.osgCockpit;
  const route = cockpitRoute(candidate?.route);
  return candidate && Number.isSafeInteger(candidate.index) && candidate.index >= 0 && route ? { index: candidate.index, route } : null;
}

/** Browser history owns navigation only; restoring an entry never replays device actions. */
export function createCockpitHistory(initialRoute: CockpitRoute, options: {
  restore: (route: CockpitRoute) => Promise<CockpitRoute | false>;
  urlFor: (route: CockpitRoute) => string;
  onError?: (error: unknown) => void;
  host?: HistoryHost;
}) {
  const host = options.host ?? window;
  let current = entry(host.history.state) ?? { index: 0, route: cockpitRoute(initialRoute)! };
  let browserIndex = current.index;
  let restoring = false, disposed = false;
  let correctingTo: number | undefined;
  function replace() { host.history.replaceState({ ...host.history.state, osgCockpit: current }, '', options.urlFor(current.route)); }
  replace();

  async function pop(event: PopStateEvent) {
    const target = entry(event.state);
    if (!target || disposed) return;
    browserIndex = target.index;
    if (correctingTo !== undefined) {
      if (target.index === correctingTo) { correctingTo = undefined; replace(); }
      return;
    }
    if (restoring || target.index === current.index) return;
    restoring = true;
    try {
      const restored = await options.restore(target.route);
      if (!disposed && restored !== false) current = { index: target.index, route: cockpitRoute(restored)! };
    } catch(error) { options.onError?.(error); }
    finally {
      restoring = false;
      if (!disposed) {
        if (browserIndex !== current.index) { correctingTo = current.index; host.history.go(current.index - browserIndex); }
        else replace();
      }
    }
  }
  function record(route: CockpitRoute, replaceEntry = false) {
    const next = cockpitRoute(route);
    if (!next || disposed || restoring || correctingTo !== undefined || equal(current.route, next)) return;
    current = { index: current.index + (replaceEntry ? 0 : 1), route: next };
    browserIndex = current.index;
    if (replaceEntry) replace();
    else host.history.pushState({ osgCockpit: current }, '', options.urlFor(next));
  }
  function back() { if (!restoring && correctingTo === undefined && current.index > 0) host.history.back(); }
  function forward() { if (!restoring && correctingTo === undefined) host.history.forward(); }
  function mouse(event: MouseEvent) {
    if (event.button !== 3 && event.button !== 4) return;
    event.preventDefault();
    if (event.type === 'mouseup') { if (event.button === 3) back(); else forward(); }
  }
  function keyboard(event: KeyboardEvent) {
    if (!event.altKey || event.ctrlKey || event.metaKey || event.shiftKey || !['ArrowLeft', 'ArrowRight'].includes(event.key)) return;
    event.preventDefault();
    if (event.key === 'ArrowLeft') back(); else forward();
  }
  const onPop = ((event: Event) => { void pop(event as PopStateEvent); }) as EventListener;
  const onMouse = mouse as EventListener, onKeyboard = keyboard as EventListener;
  host.addEventListener('popstate', onPop);
  for (const name of ['mousedown', 'mouseup', 'auxclick']) host.addEventListener(name, onMouse, { capture: true });
  host.addEventListener('keydown', onKeyboard, { capture: true });
  return {
    get route() { return current.route; },
    get navigating() { return restoring || correctingTo !== undefined; },
    record, back, forward,
    adopt(route:CockpitRoute) {
      const next=cockpitRoute(route);
      if(!next||disposed||restoring)return;
      current={index:current.index+1,route:next};browserIndex=current.index;replace();
    },
    ownsLocation() { return !!entry(host.history.state) && options.urlFor(current.route) === `${host.location.pathname}${host.location.search}${host.location.hash}`; },
    dispose() {
      disposed = true;
      host.removeEventListener('popstate', onPop);
      for (const name of ['mousedown', 'mouseup', 'auxclick']) host.removeEventListener(name, onMouse, { capture: true });
      host.removeEventListener('keydown', onKeyboard, { capture: true });
    },
  };
}

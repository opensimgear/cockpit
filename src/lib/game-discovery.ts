import type {Game, GameDiscoveryAction, GameDiscoveryEntry} from './games';
export function discoveryActions(entries: readonly GameDiscoveryEntry[], drafts: Record<string, Partial<GameDiscoveryAction>>): GameDiscoveryAction[] {
  return entries.map(entry => ({ id: entry.id, selected: entry.action !== 'remove', ...drafts[entry.id] }));
}
export function discoverySelectAll(entries: readonly Pick<GameDiscoveryEntry, 'id'>[], drafts: Record<string, Partial<GameDiscoveryAction>>, selected: boolean): Record<string, Partial<GameDiscoveryAction>> {
  const next = {...drafts};
  for (const entry of entries) next[entry.id] = {...next[entry.id], selected};
  return next;
}
export function discoveryPathError(path: string): string {
  if (!path.trim()) return 'Enter a local folder or executable path.';
  if (/[\u0000-\u001f\u007f]/.test(path)) return 'The location must not contain control characters.';
  return /^(?:[a-zA-Z]:[\\/]|\\\\[^\\/]+[\\/][^\\/]+|\/)/.test(path.trim()) ? '' : 'Enter an absolute folder or executable path.';
}

/** Saved titles and other pending titles are equally valid explicit match targets. */
export function discoveryMatchOptions(catalogue: readonly Pick<Game, 'id' | 'name'>[], entries: readonly GameDiscoveryEntry[], current: GameDiscoveryEntry): {id:string;name:string}[] {
  const options = new Map<string, {id:string;name:string}>();
  for (const game of catalogue) if (game.id !== current.gameId) options.set(game.id, {id:game.id,name:game.name});
  for (const entry of entries) {
    if (entry.action === 'remove' || entry.id === current.id || !entry.gameId || entry.gameId === current.gameId || options.has(entry.gameId)) continue;
    options.set(entry.gameId, {id:entry.gameId,name:entry.name});
  }
  return [...options.values()];
}

/** Discovery artwork is supplied by native library readers; never load arbitrary remote URLs. */
export function discoveryImage(entry: Pick<GameDiscoveryEntry, 'image'>, game?: Pick<Game, 'background' | 'defaultBackground'>): string {
  for (const image of [entry.image, game?.background?.image, game?.defaultBackground?.image]) {
    if (typeof image === 'string' && /^data:image\/(?:jpeg|png|webp|gif);base64,[A-Za-z0-9+/]+={0,2}$/.test(image)) return image;
  }
  return '';
}

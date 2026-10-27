import {gameLibraryClientName,gameHasIntegration,type Game,type GameDiscoveryAction,type GameDiscoveryEntry,type GameDiscoveryObservation} from './games';
export function discoveryActions(entries: readonly GameDiscoveryEntry[], drafts: Record<string, Partial<GameDiscoveryAction>>): GameDiscoveryAction[] {
  return entries.map(entry => ({ id: entry.id, selected: entry.action !== 'remove', ...drafts[entry.id] }));
}
export function discoverySelectAll(entries: readonly Pick<GameDiscoveryEntry, 'id'>[], drafts: Record<string, Partial<GameDiscoveryAction>>, selected: boolean): Record<string, Partial<GameDiscoveryAction>> {
  const next = {...drafts};
  for (const entry of entries) next[entry.id] = {...next[entry.id], selected};
  return next;
}

/** Additional detections explain the scan count without creating import actions. */
export function discoveryOtherInstallations(catalogue: readonly Game[], entries: readonly GameDiscoveryEntry[], observations: readonly GameDiscoveryObservation[], library: string) {
  const normalized=(path:string)=>path.replace(/^\\\\\?\\/,'').replaceAll('\\','/').toLowerCase().replace(/\/+$/,'');
  const client=(value:string)=>value==='local'?'disk':value.toLowerCase();
  const platformPaths=new Set([
    ...observations.filter(observation=>client(observation.client)!=='disk'&&observation.kind!=='tool').map(observation=>normalized(observation.installPath)),
    ...entries.filter(entry=>client(entry.library)!=='disk'&&entry.action!=='remove').map(entry=>normalized(entry.installPath)),
  ]);
  const seen=new Set<string>();
  return observations.filter(observation=>{
    const path=normalized(observation.installPath);
    if(client(observation.client)!==client(library)||observation.kind==='tool'||seen.has(path)||(client(library)==='disk'&&platformPaths.has(path)))return false;
    seen.add(path);
    return !entries.some(entry=>client(entry.library)===client(library)&&normalized(entry.installPath)===path);
  }).map(observation=>{
    const path=normalized(observation.installPath);
    const pending=entries.find(entry=>entry.action!=='remove'&&normalized(entry.installPath)===path);
    const game=catalogue.find(game=>game.installations?.some(installation=>normalized(installation.installPath)===path));
    const installation=game?.installations?.find(installation=>normalized(installation.installPath)===path);
    const executableCandidates=pending?.executableCandidates??installation?.executableCandidates??observation.executableCandidates;
    const otherLibrary=pending?.library==='disk'?'Added paths':gameLibraryClientName(pending?.library??'');
    return {id:observation.id,name:pending?.name??game?.name??observation.name,installPath:observation.installPath,
      executableCandidates,executable:pending?.executable??installation?.executable??executableCandidates[0],
      image:discoveryImage(pending??{},game),integrationAvailable:!!pending?.integration||gameHasIntegration(game),status:pending?`Listed in ${otherLibrary}`:game?'Up to date':'Detected'};
  });
}
export function discoveryPathError(path: string): string {
  if (!path.trim()) return 'Enter a folder containing game folders.';
  if (/[\u0000-\u001f\u007f]/.test(path)) return 'The location must not contain control characters.';
  return /^(?:[a-zA-Z]:[\\/]|\\\\[^\\/]+[\\/][^\\/]+|\/)/.test(path.trim()) ? '' : 'Enter an absolute folder path.';
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

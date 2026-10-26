import type { Game, GameCrop } from './games';

export type GameArtwork = {
  image: string;
  category: 'Flight' | 'Racing' | 'Rally' | 'Game';
  positionX?: number; positionY?: number;
  crop?: GameCrop;
  wordmark: string;
  edition: string;
  description: string;
  telemetryLabel: string;
};

/** Bundled official Steam landscape capsules; provenance is in public/game-art/sources.json. */
export const gameArtwork: Record<string, GameArtwork> = {
  dcs: {
    image: '/game-art/dcs.jpg', category: 'Flight', wordmark: 'DCS', edition: 'WORLD',
    description: 'Aircraft controls · DCS-BIOS feedback', telemetryLabel: 'DCS-BIOS',
  },
  'assetto-corsa': {
    image: '/game-art/assetto-corsa.jpg', category: 'Racing', wordmark: 'ASSETTO CORSA', edition: 'ORIGINAL',
    description: 'Racing telemetry · Rev lights & displays', telemetryLabel: 'Native shared-memory telemetry',
  },
  'assetto-corsa-competizione': {
    image: '/game-art/assetto-corsa-competizione.jpg', category: 'Racing', wordmark: 'ASSETTO CORSA', edition: 'COMPETIZIONE',
    description: 'GT racing telemetry · Rev lights & displays', telemetryLabel: 'Native shared-memory telemetry',
  },
  'assetto-corsa-evo': {
    image: '/game-art/assetto-corsa-evo.jpg', category: 'Racing', wordmark: 'ASSETTO CORSA', edition: 'EVO',
    description: 'Racing telemetry · Rev lights & displays', telemetryLabel: 'Native shared-memory telemetry',
  },
  'assetto-corsa-rally': {
    image: '/game-art/assetto-corsa-rally.jpg', category: 'Rally', wordmark: 'ASSETTO CORSA', edition: 'RALLY',
    description: 'Rally telemetry · Rev lights & displays', telemetryLabel: 'Native shared-memory telemetry',
  },
  'x-plane-11': {
    image: '/game-art/x-plane-11.jpg', category: 'Flight', wordmark: 'X-PLANE', edition: '11',
    description: 'Flight telemetry · Indicators & displays', telemetryLabel: 'X-Plane RREF telemetry',
  },
  'x-plane-12': {
    image: '/game-art/x-plane-12.jpg', category: 'Flight', wordmark: 'X-PLANE', edition: '12',
    description: 'Flight telemetry · Indicators & displays', telemetryLabel: 'X-Plane RREF telemetry',
  },
};

/** User art overrides both discovered art and the bundled Steam artwork. */
export function resolveGameArtwork(game: Game): GameArtwork {
  const bundled = gameArtwork[game.integrationKey ?? game.id];
  const base: GameArtwork = bundled ?? { image: '', category: 'Game', wordmark: game.name, edition: '', description: 'Keyboard and mouse bindings · Profile activation', telemetryLabel: 'Bindings and profile activation' };
  const background = game.background ?? game.defaultBackground;
  return { ...base, image: background?.image ?? base.image, positionX: background?.positionX ?? 50, positionY: background?.positionY ?? 50, crop:background?.crop };
}

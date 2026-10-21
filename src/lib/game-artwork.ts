import type { GameId } from './games';

type GameArtwork = {
  image: string;
  category: 'Flight' | 'Racing' | 'Rally';
  wordmark: string;
  edition: string;
  description: string;
  telemetryLabel: string;
};

/** Bundled publisher screenshots; provenance is in public/game-art/sources.json. */
export const gameArtwork: Record<GameId, GameArtwork> = {
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

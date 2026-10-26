import type {Game,GameCrop} from './games';

export type GameArtwork={image:string;positionX:number;positionY:number;crop?:GameCrop};

/** Every title uses its saved image, discovered platform image, or the same placeholder. */
export function resolveGameArtwork(game:Game):GameArtwork {
  const background=game.background??game.defaultBackground;
  return {image:background?.image??'',positionX:background?.positionX??50,positionY:background?.positionY??50,crop:background?.crop};
}

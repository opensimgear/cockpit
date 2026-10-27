/** Cached provider facts; these never replace editable library settings. */
export type MetadataEntity = {id:string;label:string};
export type MetadataExternalId = {platform:string;id:string};
export type GameMetadata = {
  source:'wikidata';entityId:string;url:string;title:string;description?:string|null;aliases:string[];
  genres:MetadataEntity[];genreAncestors:MetadataEntity[];platforms:MetadataEntity[];
  developers:MetadataEntity[];publishers:MetadataEntity[];releaseDates:string[];externalIds:MetadataExternalId[];
  fetchedAt:number;matchedBy:'external-id'|'exact-title';
};
export type GameMetadataLookup = {
  status:'matched'|'not-found'|'ambiguous'|'unavailable';metadata?:GameMetadata|null;checkedAt:number;stale:boolean;
};
export type GameMetadataResponse = {
  gameId:string;source:'wikidata';sourceName:'Wikidata';license:'CC0';status:'matched'|'not-cached'|'unmatched';
  results:{platform:string;appId:string;lookup:GameMetadataLookup}[];
};

import {uiPreferences} from './ui-preferences.ts';
import {INVENTORY_STORAGE_KEY,type InventoryStorage} from '../system-inventory.ts';
/** An adapter over the hydrated native record; this never opens browser storage. */
export const inventoryStorage:InventoryStorage={
  async refresh(){if(!await uiPreferences.hydrate())throw Error('Remembered inventory could not be refreshed from SQLite.');},
  getItem(key){
    if(key!==INVENTORY_STORAGE_KEY)throw Error('Unknown inventory owner.');
    const entry=uiPreferences.get('remembered');if(!entry.ready)throw Error(entry.error||'Reload remembered inventory before changing it.');
    return entry.value===null?null:JSON.stringify(entry.value);
  },
  async setItem(key,value){
    if(key!==INVENTORY_STORAGE_KEY)throw Error('Unknown inventory owner.');
    await uiPreferences.save('remembered',JSON.parse(value));
  },
};

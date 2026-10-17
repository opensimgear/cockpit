import type { DeepReadonly } from './profiles';
import type { ProfileInventoryClient, ResourceDetails } from './profile-inventory';
/** A provider catalogue stays paged and local. Browsing never declares graph ports. */
export class ProfilePortBrowser {
  private epoch = 0;
  private offsets: number[] = [];
  private current: DeepReadonly<ResourceDetails> | null = null;
  constructor(private inventory: Pick<ProfileInventoryClient, 'inspect'>) {}
  get page(): DeepReadonly<ResourceDetails> | null { return this.current; }
  get hasPrevious(): boolean { return this.offsets.length > 1; }
  get hasNext(): boolean { return !!this.current && this.current.offset + this.current.ports.length < this.current.total; }
  clear(): void { this.epoch++; this.offsets = []; this.current = null; }
  async open(resourceId: string): Promise<DeepReadonly<ResourceDetails> | null> {
    this.clear(); return this.load(resourceId, 0, false);
  }
  async next(): Promise<DeepReadonly<ResourceDetails> | null> {
    if (!this.current || !this.hasNext) return this.current;
    return this.load(this.current.resource.id, this.current.offset + this.current.ports.length, true);
  }
  async previous(): Promise<DeepReadonly<ResourceDetails> | null> {
    if (!this.current || !this.hasPrevious) return this.current;
    const page = await this.load(this.current.resource.id, this.offsets[this.offsets.length - 2], true);
    if (page) this.offsets.splice(-2,2,page.offset);
    return page;
  }
  private async load(resourceId: string, offset: number, continued: boolean): Promise<DeepReadonly<ResourceDetails> | null> {
    const epoch = ++this.epoch;
    const previous = this.current;
    const page = await this.inventory.inspect(resourceId, offset, 32);
    if (epoch !== this.epoch) return null;
    if (continued && previous && (page.session !== previous.session || page.revision !== previous.revision || page.total !== previous.total || page.resource.capabilityRevision !== previous.resource.capabilityRevision)) throw new Error('This provider catalogue changed while browsing. Refresh its ports before continuing.');
    this.current = page;
    if (!continued) this.offsets = [offset];
    else if (!this.offsets.includes(offset)) this.offsets.push(offset);
    return page;
  }
}

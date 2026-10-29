import type {CanvasNode} from '../profile-canvas';

/** Keep a physical block's user label separate from its assigned hardware. */
export function physicalDeviceIdentity(
  node: Pick<CanvasNode, 'resource' | 'assignment' | 'observation'>,
  inventoryLoaded = true,
): string | undefined {
  if (node.resource?.kind !== 'device') return undefined;
  if (!node.assignment?.sourceId) return 'Unassigned';
  const name = node.observation?.id === node.assignment.sourceId ? node.observation.name.trim() : '';
  return name || (inventoryLoaded ? 'Offline device' : 'Checking device…');
}

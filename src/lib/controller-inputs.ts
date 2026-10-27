import type { DeepReadonly } from './profiles';
import type { ResourceSummary } from './profile-inventory';
import type { ControllerResult } from './controllers';

export const CONTROLLER_INPUT_PAGE_SIZE = 32;
export const CONTROLLER_INPUT_LIMIT = 8192;

/** Resolve a physical HID resource to its exact host-controller registry key. */
export function controllerSourceId(resource: DeepReadonly<ResourceSummary> | undefined): string {
  if (resource?.resourceType !== 'host-controller') return '';
  const connections = resource.connections.filter(connection =>
    connection.scope === 'host-controllers' && connection.route.transport === 'hid');
  return connections.length === 1 ? connections[0].key : '';
}

export type ControllerInspectRequest = { command: 'controller_inspect'; sourceId: string; offset: number };
export type ControllerInspectSend = (request: ControllerInspectRequest) => Promise<ControllerResult>;

/** Load one consistent snapshot of a controller's paged live controls. */
export async function loadControllerInputs(
  sourceId: string,
  send: ControllerInspectSend,
  current: () => boolean = () => true,
): Promise<ControllerResult | null> {
  if (!sourceId) throw new Error('Choose an exact physical controller.');
  const inspect = async (offset: number): Promise<ControllerResult | null> => {
    if (!current()) return null;
    const page = await send({ command: 'controller_inspect', sourceId, offset });
    if (!current()) return null;
    return page;
  };

  const first = await inspect(0);
  if (!first) return null;
  validatePage(first, sourceId, 0);
  if (first.total > CONTROLLER_INPUT_LIMIT) throw new Error('Controller reports too many input controls.');
  if (!first.source!.online) return { ...first, controls: [] };

  const controls = [...first.controls!];
  const ids = new Set<string>();
  addUniqueIds(controls, ids);
  let expectedOffset = controls.length;
  while (expectedOffset < first.total) {
    if (!current()) return null;
    if (expectedOffset <= 0 || expectedOffset > first.total) throw new Error('Controller input pages did not make progress.');
    const page = await inspect(expectedOffset);
    if (!page) return null;
    validatePage(page, sourceId, expectedOffset);
    if (page.source!.generation !== first.source!.generation || page.source!.online !== first.source!.online || page.total !== first.total) {
      throw new Error('Controller inputs changed while loading. Inspect the controller again.');
    }
    const nextControls = page.controls!;
    if (!nextControls.length || nextControls.length > CONTROLLER_INPUT_PAGE_SIZE) throw new Error('Controller input pages did not make progress.');
    addUniqueIds(nextControls, ids);
    controls.push(...nextControls);
    expectedOffset = controls.length;
  }
  if (controls.length !== first.total) throw new Error('Controller input pages did not match the reported input count.');
  return { ...first, controls };
}

function validatePage(page: ControllerResult, sourceId: string, offset: number): void {
  if (!page.source || page.source.id !== sourceId) throw new Error('Controller inspection returned a different physical controller.');
  if (!/^\d+$/.test(page.source.generation)) throw new Error('Controller inspection returned an invalid generation.');
  if (page.offset !== offset || !Number.isSafeInteger(page.total) || page.total < 0
    || !Array.isArray(page.controls) || page.controls.length > CONTROLLER_INPUT_PAGE_SIZE
    || offset + page.controls.length > page.total) {
    throw new Error('Controller inspection returned an inconsistent input page.');
  }
}

function addUniqueIds(controls: NonNullable<ControllerResult['controls']>, ids: Set<string>): void {
  for (const control of controls) {
    const id = control.capability?.id;
    if (typeof id !== 'string' || !id || ids.has(id)) throw new Error('Controller inspection returned duplicate or invalid input capabilities.');
    ids.add(id);
  }
}

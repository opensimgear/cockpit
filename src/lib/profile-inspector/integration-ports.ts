import type { DeepReadonly, ProfilePort, ProfilePortContract } from '../profiles';

type IntegrationPort = Pick<DeepReadonly<ProfilePort>, 'id' | 'name' | 'direction' | 'contract'>;

const MAX_PAGE_SIZE = 128;

/** A short, safe contract label for integration port rows. */
export function integrationPortTypeLabel(contract: DeepReadonly<ProfilePortContract>): string {
  if (contract.delivery === 'state' || contract.delivery === 'event') {
    const { schema } = contract;
    const type = schema.type === 'integer' || schema.type === 'number'
      ? `${schema.type}${schema.unit ? ` · ${schema.unit}` : ''}`
      : schema.type;
    return contract.delivery === 'event' ? `event · ${type}` : type;
  }

  // Only render the closed contract enums here; provider supplied parameter,
  // endpoint, and display strings are not suitable compact labels.
  return `${contract.delivery} · ${contract.semantics}`;
}

/** Filter a complete discovered catalogue by direction, then page exact choice objects. */
export function integrationPortPage<T extends IntegrationPort>(
  ports: readonly T[],
  query = '',
  page = 0,
  size = 32,
  direction: 'input' | 'output' = 'output',
): { rows: T[]; total: number; offset: number; page: number; pages: number } {
  const normalizedQuery = typeof query === 'string' ? query.trim().toLocaleLowerCase() : '';
  const safeSize = Number.isFinite(size) ? Math.min(MAX_PAGE_SIZE, Math.max(1, Math.floor(size))) : 32;
  const requestedPage = Number.isFinite(page) ? Math.max(0, Math.floor(page)) : 0;
  const matches = ports.filter(port => {
    if (port.direction !== direction) return false;
    if (!normalizedQuery) return true;
    return `${port.name} ${port.id} ${integrationPortTypeLabel(port.contract)}`.toLocaleLowerCase().includes(normalizedQuery);
  });

  const total = matches.length;
  const pages = Math.ceil(total / safeSize);
  const currentPage = pages ? Math.min(requestedPage, pages - 1) : 0;
  const offset = currentPage * safeSize;
  return { rows: matches.slice(offset, offset + safeSize), total, offset, page: currentPage, pages };
}

import type { DeepReadonly, ProfilePort, ProfilePortContract } from '../profiles';

type IntegrationOutput = Pick<DeepReadonly<ProfilePort>, 'id' | 'name' | 'direction' | 'contract'>;

const MAX_PAGE_SIZE = 128;

/** A short, safe contract label for integration output rows. */
export function outputTypeLabel(contract: DeepReadonly<ProfilePortContract>): string {
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

/** Filter and page a complete discovered catalogue while retaining exact choice objects. */
export function integrationOutputPage<T extends IntegrationOutput>(
  ports: readonly T[],
  query = '',
  page = 0,
  size = 32,
): { rows: T[]; total: number; offset: number; page: number; pages: number } {
  const normalizedQuery = typeof query === 'string' ? query.trim().toLocaleLowerCase() : '';
  const safeSize = Number.isFinite(size) ? Math.min(MAX_PAGE_SIZE, Math.max(1, Math.floor(size))) : 32;
  const requestedPage = Number.isFinite(page) ? Math.max(0, Math.floor(page)) : 0;
  const matches = ports.filter(port => {
    if (port.direction !== 'output') return false;
    if (!normalizedQuery) return true;
    return `${port.name} ${port.id} ${outputTypeLabel(port.contract)}`.toLocaleLowerCase().includes(normalizedQuery);
  });

  const total = matches.length;
  const pages = Math.ceil(total / safeSize);
  const currentPage = pages ? Math.min(requestedPage, pages - 1) : 0;
  const offset = currentPage * safeSize;
  return { rows: matches.slice(offset, offset + safeSize), total, offset, page: currentPage, pages };
}

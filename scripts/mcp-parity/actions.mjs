// Deliberately conservative lexical discovery: unfamiliar action syntax must be
// added here with a regression fixture, rather than hidden by an allowlist.
export function discover(files) {
  const actions = new Map();
  const add = (path, kind, anchor) => {
    anchor = anchor.replace(/\s+/g, ' ').trim();
    const action = { path, kind, anchor };
    actions.set(JSON.stringify(action), action);
  };
  for (const [path, source] of files) {
    if (/^src\/lib\/.*\.ts$/.test(path) || /^(?:src|apps\/[^/]+\/src|packages\/[^/]+\/src)\/.*\.svelte$/.test(path)) {
      for (const match of source.matchAll(/\bcommand\s*:\s*(['"])([a-z][a-z0-9_-]*)\1/g)) add(path, 'transport', match[2]);
    }
    if (/^(?:src|apps\/[^/]+\/src|packages\/[^/]+\/src)\/.*\.svelte$/.test(path)) {
      for (const match of source.matchAll(/\b(on:[a-z]+(?:\|[a-zA-Z]+)*|on[a-z][a-z0-9]*)\s*=\s*\{/g)) {
        let depth = 1, quote = '', escape = false, end = match.index + match[0].length;
        const start = end;
        for (; end < source.length && depth; end++) {
          const char = source[end];
          if (escape) { escape = false; continue; }
          if (char === '\\') { escape = true; continue; }
          if (quote) { if (char === quote) quote = ''; continue; }
          if ('\'"`'.includes(char)) { quote = char; continue; }
          if (char === '{') depth++;
          if (char === '}') depth--;
        }
        add(path, 'ui', match[1] + '={' + source.slice(start, end));
      }
    }
    if (/^src\/lib\/.*\.ts$/.test(path)) {
      for (const match of source.matchAll(/export\s+(?:async\s+function\s+(\w+)|(?:const|let)\s+(\w+)\s*=\s*async\b)/g)) add(path, 'action', match[1] || match[2]);
      for (const match of source.matchAll(/\b(?:invoke|request|command|mutate)(?:<[^>]+>)?\s*\(\s*(['"])([a-z][a-z0-9_-]*)\1/g)) add(path, 'transport', match[2]);
    }
    if (/^src-tauri\/src\/.*\.rs$/.test(path)) {
      for (const match of source.matchAll(/#\[(?:tauri::)?command(?:\([^\]]*\))?\]\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+(\w+)/g)) add(path, 'native', match[1]);
      if (/\/gateways(?:\/|\.rs)/.test(path) && !/test/.test(path)) {
        for (const match of source.matchAll(/(?:"[a-z][a-z0-9_-]*"\s*\|\s*)*"([a-z][a-z0-9_-]*)"\s*=>/g)) {
          for (const name of match[0].matchAll(/"([^"]+)"/g)) add(path, 'dispatch', name[1]);
        }
      }
    }
  }
  return [...actions.values()].sort((a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b)));
}

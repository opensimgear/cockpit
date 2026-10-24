'use strict';

const fs = require('node:fs');
const path = require('node:path');

const MANIFEST = 'tools/nx/workspace.json';
const SOURCE_EXTENSIONS = ['.ts', '.tsx', '.js', '.mjs', '.cjs', '.svelte', '.json', '.html', '.css'];
const WORKSPACE_PATH_PREFIXES = ['src/', 'apps/', 'packages/', 'tests/', 'scripts/', 'crates/', 'examples/', 'public/', 'integration-profiles/'];

function slash(value) {
  return value.split(path.sep).join('/');
}

function shouldUseShell(command, platform = process.platform) {
  return platform === 'win32' && /(?:^|[\\/])pnpm(?:\.cmd)?$/i.test(command);
}

function globRegex(glob) {
  let source = '';
  for (let i = 0; i < glob.length; i += 1) {
    const char = glob[i];
    if (char === '*' && glob[i + 1] === '*') {
      i += 1;
      if (glob[i + 1] === '/') {
        i += 1;
        source += '(?:.*/)?';
      } else {
        source += '.*';
      }
    } else if (char === '*') {
      source += '[^/]*';
    } else if (char === '?') {
      source += '[^/]';
    } else {
      source += char.replace(/[|\\{}()[\]^$+?.]/g, '\\$&');
    }
  }
  return new RegExp(`^${source}$`);
}

function matchesAny(file, patterns = []) {
  return patterns.some(pattern => globRegex(pattern).test(file));
}

function readManifest(root) {
  return JSON.parse(fs.readFileSync(path.join(root, MANIFEST), 'utf8'));
}

function walk(dir, root, files = []) {
  if (!fs.existsSync(dir)) return files;
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.name === 'node_modules' || entry.name === '.git' || entry.name === 'dist') continue;
    const absolute = path.join(dir, entry.name);
    if (entry.isDirectory()) walk(absolute, root, files);
    else if (entry.isFile()) files.push(slash(path.relative(root, absolute)));
  }
  return files;
}

function featureForTest(file, groups) {
  const normalized = slash(file);
  const candidates = groups.filter(group => !group.catchAll && matchesAny(normalized, group.testPatterns) && !matchesAny(normalized, group.excludePatterns));
  if (candidates.length > 1) throw new Error(`Test has multiple Nx owners: ${normalized} (${candidates.map(item => item.name).join(', ')})`);
  return candidates[0]?.name ?? groups.find(group => group.catchAll)?.name ?? null;
}

function testFilesForGroup(root, group, groups) {
  const srcTests = walk(path.join(root, 'src/lib'), root)
    .filter(file => file.endsWith('.test.mjs') && featureForTest(file, groups) === group.name);
  const extraTests = (group.additionalTests ?? []).filter(file => fs.existsSync(path.join(root, file)));
  return [...new Set([...srcTests, ...extraTests])].sort();
}

function resolveRelative(fromFile, request, root, includeMissing = false) {
  const isRelative = request.startsWith('.');
  if (!isRelative && !WORKSPACE_PATH_PREFIXES.some(prefix => request.startsWith(prefix))) return [];
  const base = path.resolve(root, isRelative ? path.dirname(fromFile) : '.', request);
  const relativeBase = path.relative(root, base);
  if (relativeBase.startsWith('..') || path.isAbsolute(relativeBase)) return [];
  if (fs.existsSync(base) && fs.statSync(base).isDirectory()) return [`${slash(relativeBase)}/**`];
  const candidates = [base, ...SOURCE_EXTENSIONS.map(ext => `${base}${ext}`), ...SOURCE_EXTENSIONS.map(ext => path.join(base, `index${ext}`))];
  const resolved = candidates.filter(candidate => {
    const relative = path.relative(root, candidate);
    return !relative.startsWith('..') && !path.isAbsolute(relative) && fs.existsSync(candidate) && fs.statSync(candidate).isFile();
  }).map(candidate => slash(path.relative(root, candidate)));
  if (resolved.length || !includeMissing) return resolved;
  return candidates.filter(candidate => {
    const relative = path.relative(root, candidate);
    return !relative.startsWith('..') && !path.isAbsolute(relative);
  }).map(candidate => slash(path.relative(root, candidate)));
}

function isTypeOnlyReference(contents, match) {
  const prefixStart = Math.max(contents.lastIndexOf(';', match.index), contents.lastIndexOf('\n', match.index)) + 1;
  const prefix = contents.slice(prefixStart, match.index);
  if (/\b(?:import|export)\s+type\b[\s\S]*\bfrom\s*$/.test(prefix)) return true;
  const allTypeImports = prefix.match(/\b(?:import|export)\s*\{([^}]*)\}\s*from\s*$/);
  if (allTypeImports && allTypeImports[1].split(',').every(item => /^\s*type\s+/.test(item))) return true;
  const suffix = contents.slice(match.index + match[0].length);
  return /\bimport\s*\(\s*$/.test(prefix) && /^\s*\)\s*\./.test(suffix);
}

function sourceClosure(root, testFiles) {
  const visited = new Set();
  const relativeReferencePattern = /(['"])(\.{1,2}\/[^'"\r\n]+)\1/g;
  const workspaceReferencePattern = /(['"])((?:src|apps|packages|tests|scripts|crates|examples|public|integration-profiles)\/[^'"\r\n]+)\1/g;
  const readerPattern = /(?:readFile(?:Sync)?|readdir(?:Sync)?|glob|open|access|stat(?:Sync)?|lstat(?:Sync)?|existsSync)\s*\(/;
  for (const testFile of testFiles) {
    const testSource = fs.existsSync(path.join(root, testFile)) ? fs.readFileSync(path.join(root, testFile), 'utf8') : '';
    const exactStubMatches = [...testSource.matchAll(/if\s*\(\s*(?:name|request|specifier)\s*===\s*(['"])(\.{1,2}\/[^'"\r\n]+)\1\s*\)\s*return\s*\{/g)];
    const exactStubs = new Set(exactStubMatches.map(match => match[2]));
    // A test with a recursive component loader can also contain a separate stubbed
    // harness. A file-wide suffix stub cannot prove which loader reads each child.
    const recursivelyLoadsComponents = /\b(?:component|load)\s*\(\s*match\[1\]\s*\)/.test(testSource);
    const suffixStubs = recursivelyLoadsComponents ? [] : [...testSource.matchAll(/if\s*\(\s*(?:name|request|specifier)\.endsWith\s*\(\s*(['"])([^'"\r\n]+)\1\s*\)\s*\)\s*return\s*\{/g)].map(match => match[2]);
    const directLoads = new Set();
    for (const block of testSource.matchAll(/loadModules\s*\(\s*\[([\s\S]*?)\]\s*\)/g)) {
      for (const request of block[1].matchAll(/(['"])(\.{1,2}\/[^'"\r\n]+)\1/g)) directLoads.add(request[2]);
    }
    for (const match of testSource.matchAll(/\b(?:component|load)\s*\(\s*(['"])(\.{1,2}\/[^'"\r\n]+\.svelte)\1\s*\)/g)) directLoads.add(match[2]);
    const realFiles = new Set([...directLoads].flatMap(request => resolveRelative(testFile, request, root)));
    const queue = [testFile];
    const seen = new Set();
    while (queue.length) {
      const file = queue.pop();
      if (seen.has(file)) continue;
      seen.add(file);
      visited.add(file);
      const absolute = path.join(root, file);
      if (!fs.existsSync(absolute) || !fs.statSync(absolute).isFile()) continue;
      const contents = fs.readFileSync(absolute, 'utf8');
      const references = [...contents.matchAll(relativeReferencePattern)];
      for (const match of contents.matchAll(workspaceReferencePattern)) {
        const statementStart = Math.max(contents.lastIndexOf(';', match.index), contents.lastIndexOf('\n', match.index)) + 1;
        if (readerPattern.test(contents.slice(statementStart, match.index))) references.push(match);
      }
      for (const match of references) {
        if (isTypeOnlyReference(contents, match)) continue;
        if (file === testFile && exactStubMatches.some(stub => match.index >= stub.index && match.index < stub.index + stub[0].length)) continue;
        const statementStart = Math.max(contents.lastIndexOf(';', match.index), contents.lastIndexOf('\n', match.index)) + 1;
        const isFileReader = readerPattern.test(contents.slice(statementStart, match.index));
        const resolved = resolveRelative(file, match[2], root, true);
        const isStub = exactStubs.has(match[2]) || suffixStubs.some(suffix => match[2].endsWith(suffix));
        if (file.endsWith('.svelte') && !isFileReader && isStub && !directLoads.has(match[2]) && !resolved.some(item => realFiles.has(item))) continue;
        for (const item of resolved) if (!seen.has(item)) queue.push(item);
      }
    }
  }
  return [...visited].sort();
}

function fileset(pattern) {
  return pattern.startsWith('!') ? `!{workspaceRoot}/${pattern.slice(1)}` : `{workspaceRoot}/${pattern}`;
}

function targetInputs(root, manifest, group, groups) {
  const tests = testFilesForGroup(root, group, groups);
  if (!tests.length) throw new Error(`Nx test group ${group.name} owns no tests`);
  const patterns = [
    ...manifest.globalInputs,
    ...(group.testPatterns ?? []),
    ...(group.additionalTests ?? []),
    ...(group.excludePatterns ?? []).map(pattern => `!${pattern}`),
    ...(group.sourcePatterns ?? []),
  ];
  if (group.catchAll) {
    patterns.push('src/lib/**/*.test.mjs');
    for (const other of groups.filter(item => item !== group && !item.catchAll)) {
      patterns.push(...(other.testPatterns ?? []).map(pattern => `!${pattern}`));
      patterns.push(...(other.excludePatterns ?? []).map(pattern => `!${pattern}`));
    }
  }
  const closure = sourceClosure(root, tests).map(file => `{workspaceRoot}/${file}`);
  return [...new Set([...patterns.map(pattern => pattern === 'sharedGlobals' ? pattern : fileset(pattern)), ...closure])];
}

function createProjectNodes(root) {
  const manifest = readManifest(root);
  const projects = {};
  for (const group of manifest.groups) {
    const rootPath = `tools/nx/projects/${group.name}`;
    projects[rootPath] = {
      name: group.name,
      projectType: 'library',
      metadata: { description: `Portable cockpit checks for ${group.name}` },
      targets: {
        test: {
          command: `node tools/nx/run-tests.mjs ${group.name}`,
          cache: true,
          inputs: targetInputs(root, manifest, group, manifest.groups),
        },
      },
    };
  }
  projects['tools/nx/projects/nx-workspace'] = {
    name: 'nx-workspace',
    projectType: 'library',
    metadata: { description: 'Nx workspace plugin and input-closure checks' },
    targets: {
      test: {
        command: 'node --experimental-test-coverage --test-coverage-include=tools/nx/**/*.cjs --test-coverage-lines=71 --test-coverage-branches=71 --test-coverage-functions=71 --test tools/nx/plugin.test.mjs',
        cache: true,
        inputs: ['sharedGlobals'],
      },
    },
  };
  return projects;
}

async function createNodesInternal(configFiles, _options, context) {
  return configFiles
    .filter(file => slash(file) === MANIFEST)
    .map(file => [file, { projects: createProjectNodes(context.workspaceRoot) }]);
}

const createNodes = [MANIFEST, createNodesInternal];

module.exports = {
  createNodes,
  createNodesV2: createNodes,
  _private: { createNodesInternal, createProjectNodes, featureForTest, globRegex, isTypeOnlyReference, readManifest, resolveRelative, shouldUseShell, sourceClosure, targetInputs, testFilesForGroup },
};

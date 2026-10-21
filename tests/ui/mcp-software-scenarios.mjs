// Additional real-client scenarios for the running native Cockpit.
// This module neither mocks IPC nor requires a physical device or live output.
import assert from 'node:assert/strict';

const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
const clone = value => JSON.parse(JSON.stringify(value));
async function until(operation, description, timeout = 20000) {
  const deadline = Date.now() + timeout; let last;
  while (Date.now() < deadline) {
    try { const result = await operation(); if (result) return result; } catch (error) { last = error; }
    await pause(100);
  }
  throw new Error(`${description}${last ? `: ${last.message}` : ''}`);
}

// These are the actual scalar/operation contracts from mapping/src/signal.rs,
// mapping/src/program.rs and the UI's profile-processing.ts state presets.
const booleanPort = (id, direction) => ({ id, name: id, direction,
  contract: { delivery: 'state', schema: { type: 'boolean' }, freshnessMs: 1000, fallback: null } });
const edge = (id, from, to) => ({ id, from: { nodeId: from, portId: 'out' }, to: { nodeId: to, portId: 'in' } });
const nodes = () => [
  { id: 'mcp-constant', name: 'MCP Boolean source', operation: { kind: 'constant', value: true }, inputs: [], outputs: [booleanPort('out', 'output')] },
  { id: 'mcp-not-first', name: 'MCP invert once', operation: { kind: 'not' }, inputs: [booleanPort('in', 'input')], outputs: [booleanPort('out', 'output')] },
  { id: 'mcp-not-second', name: 'MCP invert twice', operation: { kind: 'not' }, inputs: [booleanPort('in', 'input')], outputs: [booleanPort('out', 'output')] },
];

async function showGraph(page, name, expectedNodes, expectedEdges) {
  // Reload the real frontend so a previously mounted profile list cannot keep a
  // stale pre-MCP library projection. The desktop/native manager stays running.
  await page.evaluate(() => { window.location.reload(); return true; });
  await until(() => page.evaluate(() => !!document.querySelector('nav[aria-label="Main navigation"]')), 'Cockpit did not reload its navigation');
  await page.evaluate(() => {
    const button = [...document.querySelectorAll('nav[aria-label="Main navigation"] button')].find(button => button.textContent.trim() === 'Profiles');
    if (!button) throw new Error('Profiles navigation is missing');
    button.click();
  });
  await until(async () => {
    await page.evaluate(() => { document.querySelector('.back-profiles')?.click(); });
    return page.evaluate(name => [...document.querySelectorAll('[aria-label="Saved profiles"] button')].some(button => button.getAttribute('aria-label') === `Edit ${name}`), name);
  }, 'The real profile list did not show the MCP-saved graph');
  await page.evaluate(name => {
    [...document.querySelectorAll('[aria-label="Saved profiles"] button')].find(button => button.getAttribute('aria-label') === `Edit ${name}`).click();
  }, name);
  await until(() => page.evaluate(() => !!document.querySelector('[aria-label="Toggle detailed graph"]')), 'The real profile canvas did not load');
  await page.evaluate(() => {
    const toggle = document.querySelector('[aria-label="Toggle detailed graph"]');
    if (toggle?.getAttribute('aria-pressed') === 'false') toggle.click();
  });
  await until(() => page.evaluate(ids => {
    const graph = document.querySelector('[aria-label="Profile resource graph"]');
    const seen = [...(graph?.querySelectorAll('[data-node-id]') ?? [])].map(node => node.getAttribute('data-node-id'));
    return ids.every(id => seen.includes(id));
  }, expectedNodes.map(node => node.id)), 'The real profile editor did not render the saved processing nodes');
  await page.evaluate(() => document.querySelector('[aria-label="Fit all"]')?.click());
  const rendered = await until(() => page.evaluate(() => {
    const graph = document.querySelector('[aria-label="Profile resource graph"]');
    if (!graph) return null;
    const result = {
      heading: document.querySelector('.editor-title h1')?.textContent.trim(),
      nodes: [...graph.querySelectorAll('[data-node-id]')].map(node => ({ id: node.getAttribute('data-node-id'), name: node.querySelector('.block-heading strong')?.textContent.trim(), displayed: node.getBoundingClientRect().width > 0 })),
      routes: [...graph.querySelectorAll('path.signal-edge[data-binding-id]')].map(route => ({ id: route.getAttribute('data-binding-id'), pathPresent: !!route.getAttribute('d') })),
    };
    return result.routes.length >= 2 && result.routes.every(route => route.pathPresent) ? result : null;
  }), 'The real profile editor did not draw the saved signal routes');
  assert.equal(rendered.heading, name);
  for (const node of expectedNodes) assert.ok(rendered.nodes.some(item => item.id === node.id && item.name === node.name && item.displayed), `Missing rendered processing node ${node.id}`);
  for (const binding of expectedEdges) assert.ok(rendered.routes.some(item => item.id === binding.id && item.pathPresent), `Missing rendered binding ${binding.id}`);
  return rendered;
}

/** Root owns the desktop lifecycle and final profile/credential restoration. */
export async function softwareScenarios({ page, writer, connect, endpoint, token, profiles, evidence, ok, call, mutation }) {
  assert.equal(typeof token, 'string', 'The primary official SDK session needs its current credential');
  const report = { hardwareValidation: 'Deferred by user; no device or output activation is required by these software scenarios.' };
  let second, secondCredential, providerOriginal, providerChanged = false, providerCandidate, primaryError;
  const providerScope = { scope: 'game-provider', gameId: 'x-plane-11' };
  try {
    secondCredential = await page.invoke('mcp_credential_create', { name: 'Live acceptance independent writer', scope: 'write' });
    second = await connect(endpoint, secondCredential.token);
    // Cache the real tool output validators in both official SDK sessions.
    await second.listTools();
    const stopped = await ok(writer, 'profile_status');
    assert.equal(stopped.profileSession.owner, null, 'Run software-only graph/provider checks after explicit native Stop');

    const operationPages = await Promise.all(['constant', 'not'].map(search => ok(writer, 'processing_operations_list', { search, category: 'logic', offset: 0, limit: 64 })));
    for (const [index, kind] of ['constant', 'not'].entries()) {
      const operation = operationPages[index].operations.find(operation => operation.kind === kind);
      assert.ok(operation, `Native processing discovery did not advertise ${kind}`);
      assert.equal(operation.parametersSchema.properties.kind.const, kind);
      assert.equal(typeof operation.schemaHash, 'string');
    }
    const gamePage = await ok(writer, 'games_list', { offset: 0, limit: 1 });
    assert.equal(gamePage.catalogue.length, 1);
    assert.ok(!Object.hasOwn(gamePage.catalogue[0].vehicleSupport ?? {}, 'vehicles'), 'Game summaries must not embed an unbounded vehicle catalogue');
    const vehiclePage = await ok(writer, 'game_vehicles_list', { gameId: providerScope.gameId, offset: 0, limit: 1 });
    assert.equal(vehiclePage.game.id, providerScope.gameId); assert.ok(vehiclePage.vehicles.length <= 1); assert.equal(typeof vehiclePage.catalogueStamp, 'string');
    report.discovery = { processingKinds: ['constant', 'not'], nativeParameterSchemas: true, gamesSummaryPaged: true, gameVehiclesPaged: true, vehicleCount: vehiclePage.total };

    const seed = await ok(writer, 'profile_prepare_changes', { name: `MCP wired software ${Date.now()}`, expectedRevision: null });
    const graph = clone(seed.document), graphNodes = nodes(), graphEdges = [edge('mcp-source-to-not', 'mcp-constant', 'mcp-not-first'), edge('mcp-not-to-not', 'mcp-not-first', 'mcp-not-second')];
    graph.processingNodes = graphNodes; graph.bindings = graphEdges;
    graph.editor.positions = graphNodes.map((node, index) => ({ nodeId: node.id, x: 40 + index * 320, y: 80 }));
    const prepared = await ok(writer, 'profile_prepare_changes', { document: graph, assignments: [], expectedRevision: null });
    assert.equal(prepared.validation.valid, true, JSON.stringify(prepared.validation));
    assert.equal(prepared.compilation.readiness, 'ready', JSON.stringify(prepared.compilation));
    assert.equal(prepared.effects.saved, false); assert.equal(prepared.effects.applied, false); assert.ok(prepared.diff.totalChanges > 0);
    profiles.add(graph.id); await ok(writer, 'profile_save_prepared', mutation({ draftId: prepared.draftId }));
    const saved = await ok(writer, 'profile_get', { id: graph.id });
    assert.deepEqual(saved.document.processingNodes, graphNodes); assert.deepEqual(saved.document.bindings, graphEdges); assert.deepEqual(saved.assignments, []);
    const nativeSaved = (await page.native({ command: 'profile_assignments_get', id: graph.id })).profiles;
    assert.deepEqual(nativeSaved.document, saved.document); assert.deepEqual(nativeSaved.assignments, saved.assignments);
    const nativeCompile = (await page.native({ command: 'profile_compile', document: saved.document, assignments: saved.assignments })).profileCompilation;
    assert.equal(nativeCompile.readiness, prepared.compilation.readiness); assert.equal(nativeCompile.programHash, prepared.compilation.programHash);
    const trace = await ok(writer, 'compilation_inspect', { compilationId: nativeCompile.compilationId, section: 'nodes', offset: 0, limit: 64 });
    for (const node of graphNodes) assert.ok(JSON.stringify(trace.items).includes(node.id), `Native compiler trace is missing ${node.id}`);
    report.graph = { profileId: graph.id, processingNodes: graphNodes.length, bindings: graphEdges.length, readiness: nativeCompile.readiness, nativeSavedAgrees: true, nativeCompilerAgrees: true, hardwareOutputActivationRequested: false };

    // Independently authenticated clients prepare at the same revision, then
    // submit real concurrent saves. Native persistence must admit exactly one.
    const baseRevision = saved.document.revision;
    const names = [`${graph.name} writer A`, `${graph.name} writer B`];
    const drafts = await Promise.all([writer, second].map((client, index) => ok(client, 'profile_prepare_changes', { profileId: graph.id, expectedRevision: baseRevision, edits: [{ kind: 'set_name', name: names[index] }] })));
    const raced = await Promise.all([writer, second].map((client, index) => call(client, 'profile_save_prepared', mutation({ draftId: drafts[index].draftId }))));
    assert.equal(raced.filter(result => result.ok).length, 1, JSON.stringify(raced.map(result => ({ ok: result.ok, error: result.error }))));
    const winner = raced.findIndex(result => result.ok), loser = raced[1 - winner];
    assert.equal(loser.error.code, 'revision_conflict');
    const afterRace = await ok(writer, 'profile_get', { id: graph.id });
    assert.equal(afterRace.document.revision, baseRevision + 1); assert.equal(afterRace.document.name, names[winner]);
    assert.deepEqual(afterRace.document.processingNodes, graphNodes); assert.deepEqual(afterRace.document.bindings, graphEdges);
    assert.deepEqual((await ok(second, 'profile_get', { id: graph.id })).document, afterRace.document);
    assert.deepEqual((await page.native({ command: 'profile_get', id: graph.id })).profiles.document, afterRace.document);
    report.concurrentWriters = { independentCredentials: true, submitted: 2, committed: 1, conflicts: 1, conflictCode: loser.error.code, revisionAdvancedBy: 1, nativeAndBothClientsAgree: true };
    report.graph.editor = await showGraph(page, afterRace.document.name, graphNodes, graphEdges);
    assert.equal((await ok(writer, 'profile_status')).profileSession.owner, null);
    // Apply the software-only graph through the real coordinator and inspect
    // its computed ports; it declares no hardware, controller or desktop output.
    try {
      await ok(writer, 'profile_apply', mutation({ id: graph.id, expectedRevision: afterRace.document.revision, expectedActiveOwner: null }));
      const owner = (await ok(writer, 'profile_status')).profileSession.owner;
      assert.equal(owner.profileId, graph.id); assert.equal(owner.profileRevision, afterRace.document.revision);
      assert.deepEqual((await page.native({ command: 'profile_status' })).profiles.profileSession.owner, owner);
      const inspected = await until(async () => {
        const page = await ok(writer, 'applied_ports_inspect', { owner, nodeId: 'mcp-not-second', limit: 8 });
        return page.ports.some(port => port.endpoint.portId === 'out' && port.sample?.value?.value === true) && page;
      }, 'Applied constant / NOT / NOT graph did not compute its expected output');
      assert.ok(inspected.ports.some(port => port.endpoint.portId === 'in' && port.sample?.value?.value === false));
      report.graph.runtime = { appliedExactRevision: owner.profileRevision, nativeOwnerAgrees: true, secondNotInput: false, secondNotOutput: true, hardwareOutputsDeclared: 0 };
    } finally {
      await ok(writer, 'profile_stop', mutation({}));
    }
    assert.equal((await ok(writer, 'profile_status')).profileSession.owner, null);

    // Change an ordinary validated connection preference, without starting a
    // game/provider or changing automatic-selection policy. Restore it below.
    const [local, otherLocal] = await Promise.all([ok(writer, 'local_settings_get', providerScope), ok(second, 'local_settings_get', providerScope)]);
    assert.equal(local.revision, otherLocal.revision);
    assert.equal(local.settings.connection.source, 'rref'); assert.equal(local.settings.connection.version, 11);
    providerOriginal = { connection: clone(local.settings.connection) };
    providerCandidate = clone(providerOriginal);
    providerCandidate.connection.freshnessMs = providerOriginal.connection.freshnessMs === 3000 ? 3100 : 3000;
    const competing = clone(providerOriginal); competing.connection.freshnessMs = providerCandidate.connection.freshnessMs === 3100 ? 3200 : 3100;
    providerChanged = true;
    await ok(writer, 'local_settings_save', mutation({ ...providerScope, expectedSettingsRevision: local.revision, expectedActiveOwner: null, settings: providerCandidate }));
    const rejected = await call(second, 'local_settings_save', mutation({ ...providerScope, expectedSettingsRevision: otherLocal.revision, expectedActiveOwner: null, settings: competing }));
    assert.equal(rejected.ok, false); assert.equal(rejected.error.code, 'revision_conflict');
    const readback = await ok(writer, 'local_settings_get', providerScope);
    assert.deepEqual(readback.settings.connection, providerCandidate.connection);
    const nativeLocal = (await page.native({ command: 'profile_local_settings_get', ...providerScope })).localSettings;
    assert.deepEqual(nativeLocal.settings.connection, providerCandidate.connection); assert.equal(nativeLocal.revision, readback.revision);
    assert.equal((await ok(writer, 'profile_status')).profileSession.owner, null, 'A provider preference edit must not activate a profile');
    report.providerSettings = { gameId: providerScope.gameId, field: 'freshnessMs', before: providerOriginal.connection.freshnessMs, edited: providerCandidate.connection.freshnessMs, secondWriterConflict: rejected.error.code, nativeReadbackAgrees: true, profileActivated: false };
  } catch (error) { primaryError = error; throw error; }
  finally {
    const cleanupErrors = [];
    if (providerOriginal && providerChanged) {
      try {
        const latest = await ok(writer, 'local_settings_get', providerScope);
        if (JSON.stringify(latest.settings.connection) !== JSON.stringify(providerOriginal.connection)) {
          assert.deepEqual(latest.settings.connection, providerCandidate.connection, 'Provider settings changed outside this acceptance run; preserve that change instead of overwriting it');
          const owner = (await ok(writer, 'profile_status')).profileSession.owner;
          await ok(writer, 'local_settings_save', mutation({ ...providerScope, expectedSettingsRevision: latest.revision, expectedActiveOwner: owner, settings: providerOriginal }));
        }
        const restored = await ok(writer, 'local_settings_get', providerScope);
        assert.deepEqual(restored.settings.connection, providerOriginal.connection);
        const nativeRestored = (await page.native({ command: 'profile_local_settings_get', ...providerScope })).localSettings;
        assert.deepEqual(nativeRestored.settings.connection, providerOriginal.connection);
        if (report.providerSettings) report.providerSettings.restored = true;
      } catch (error) { cleanupErrors.push(error); }
    }
    if (second) { try { await second.close(); } catch (error) { cleanupErrors.push(error); } }
    if (secondCredential) { try { await page.invoke('mcp_credential_revoke', { id: secondCredential.credential.id }); } catch (error) { cleanupErrors.push(error); } }
    report.independentCredentialRemoved = !!secondCredential && !cleanupErrors.length;
    report.completed = !primaryError && !cleanupErrors.length;
    evidence.softwareScenarios = report;
    if (cleanupErrors.length) throw new AggregateError(primaryError ? [primaryError, ...cleanupErrors] : cleanupErrors, 'Software acceptance cleanup did not complete');
  }
}

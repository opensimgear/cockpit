import ts from 'typescript';
import { parse } from 'svelte/compiler';

// Each entry identifies a reviewed call site by its callback and delay.
const intervals = new Map();

const timeouts = new Map([
  // Connection retries and protocol deadlines.
  ['apps/companion/src/lib/lease-heartbeat.ts', ['send,Math.max(0,Math.ceil(deadline-now()))']],
  ['apps/browser-companion/src/App.svelte', [
    '()=>{retryTimer=undefined;void scheduleConnect();},Math.ceil(delay)',
    'runMaintenance,Math.ceil(delay)',
    'sendPreparationHeartbeat,Math.max(0,Math.ceil(sentAt+500-performance.now()))',
  ]],
  ['apps/browser-companion/src/connection-worker.ts', ['()=>{retry=undefined;void connect();},1000','maintain,Math.max(0,Math.ceil(next-performance.now()))']],
  ['apps/browser-companion/src/transport.ts', ['()=>this.fallbackDirect(worker),WORKER_STARTUP_TIMEOUT_MS']],
  ['apps/browser-companion/src/socket-transport.ts', [
    "()=>{socket.close();reject(new Error('Cockpit connection timed out'));},3000",
    "()=>{this.fail(new Error('Cockpit reply timed out'));this.socket?.close();},4000",
  ]],
  ['src/lib/gateway-workspace.ts', ['callback,milliseconds']],
  // Authored actions, feedback expiry and compiler work.
  ['packages/companion-ui/src/CompiledWidget.svelte', [
    '()=>{deadline=timeout=undefined;tickClock();},Math.max(0,Math.ceil(next-performance.now()))',
  ]],
  ['packages/companion-ui/src/PanelCanvas.svelte', [
    '()=>controller.tick(),Math.max(0,Math.ceil(deadline-performance.now()))',
    '()=>{now=performance.now();},Math.max(0,Math.ceil(deadline-performance.now()))',
  ]],
  ['packages/widget-compiler/src/compiler.ts', ['resolve,0']],
  // Visible pairing countdown and the user-initiated firmware settle deadline.
  ['src/lib/CompanionConnections.svelte', ['tick,remaining%1000||1000','resolve,1500']],
  // Debounce, dismissal, resource disposal and explicit operation deadlines.
  ['src/lib/activity/ActivityPanel.svelte', ['()=>settledSearch=value,250']],
  ['src/lib/companion-settings.ts', ['()=>{timer=undefined;void start();},Math.max(0,due-Date.now())']],
  ['src/lib/CompiledCompanionDesigner.svelte', ['()=>URL.revokeObjectURL(url),1000']],
  ['src/lib/GenericWidgetDesigner.svelte', ['()=>URL.revokeObjectURL(url),1000']],
  ['src/lib/notifications.ts', ['()=>dismissNotification(id),remainingMs']],
  ['src/lib/profile-elk-runtime.ts', ["()=>stopEngine(new Error('ELK layout took too long. Please try Auto layout again.')),30_000"]],
  ['src/lib/profile-inspector/ControlConnectionBadge.svelte', ['()=>{if(!hovered&&!popupHovered&&!focused)hide();},160']],
  ['src/lib/profile-vehicle-conflicts.ts', ['callback,delay']],
  ['src/lib/ProfileCanvas.svelte', ['()=>{connectionClickTimer=undefined;chooseConnection(id);},250']],
  ['src/lib/ProfileLibraryTools.svelte', ['()=>URL.revokeObjectURL(url),0']],
]);

const frames = new Map([
  // A received controller state schedules one frame. The callback performs no reads.
  ['src/lib/controller-observations.ts', [
    '()=>{frame=undefined;const next=pending;pending=undefined;if(next){rendered=next;this.notify(listener,next);}}',
  ]],
  ['packages/companion-ui/src/CompiledWidget.svelte', ['()=>{raf=undefined;tickClock();}']],
  ['src/lib/ProfileCanvas.svelte', [
    'callback',
    '()=>{moveFrame=0;const latest=pendingMove;pendingMove=null;if(latest)move(latest);}',
  ]],
]);

function fingerprint(source) {
  const scanner = ts.createScanner(ts.ScriptTarget.Latest, true, undefined, source);
  const tokens = [];
  while (scanner.scan() !== ts.SyntaxKind.EndOfFileToken) tokens.push(scanner.getTokenText());
  return JSON.stringify(tokens);
}

function scripts(file, source) {
  if (!file.endsWith('.svelte')) return [source];
  const { instance, module } = parse(source, { modern: true });
  return [instance, module].filter(Boolean).map(script =>
    source.slice(script.content.start, script.content.end));
}

export function checkUiTimers(sources) {
  const violations = [];
  for (const [file, source] of sources) {
    const expected = new Map([
      ['setInterval', (intervals.get(file) ?? []).map(fingerprint)],
      ['setTimeout', (timeouts.get(file) ?? []).map(fingerprint)],
      ['requestAnimationFrame', (frames.get(file) ?? []).map(fingerprint)],
    ]);
    for (const script of scripts(file, source)) {
      const ast = ts.createSourceFile(file, script, ts.ScriptTarget.Latest, true,
        file.endsWith('.svelte') ? ts.ScriptKind.TS : undefined);
      function visit(node) {
        if (ts.isCallExpression(node)) {
          const callee = node.expression;
          const name = ts.isIdentifier(callee) ? callee.text
            : ts.isPropertyAccessExpression(callee) ? callee.name.text
            : ts.isElementAccessExpression(callee) && ts.isStringLiteralLike(callee.argumentExpression)
              ? callee.argumentExpression.text : undefined;
          const entries = expected.get(name);
          if (entries) {
            const argumentsText = node.arguments.map(argument => argument.getText()).join(',');
            const index = entries.indexOf(fingerprint(argumentsText));
            if (index === -1) {
              violations.push(`${file}: unclassified ${name} ${node.getText().replace(/\s+/g, ' ')}`);
            } else entries.splice(index, 1);
          }
        }
        ts.forEachChild(node, visit);
      }
      visit(ast);
    }
    for (const [name, entries] of expected) {
      if (entries.length) violations.push(`${file}: remove ${entries.length} stale ${name} entries`);
    }
  }
  if (violations.length) throw new Error(violations.join('\n'));
}

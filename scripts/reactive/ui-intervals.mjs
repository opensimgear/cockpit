import ts from 'typescript';
import { parse } from 'svelte/compiler';

const allowed = new Map([
  ['src/lib/CompanionConnections.svelte', [
    "()=>{secondsLeft=Math.max(0,Math.ceil((expires-Date.now())/1000));if(!secondsLeft&&pairing){pairing=undefined;if(kind==='browser')browserOfferAttempted=false;}},1000",
  ]],
  ['src/lib/Esp32Companions.svelte', ['refresh,1000']],
  ['src/lib/ProfileDesktopControls.svelte', ['refresh,1500']],
  ['apps/companion/src/App.svelte', [
    "()=>{if(remoteSelected&&gate.snapshot().armed&&remote.phase==='armed'&&snapshot.armed){consttoken=gate.capture();voidrequest({type:'keepalive',epoch:remote.epoch}).catch(value=>{if(gate.current(token))fail(value);});}},100",
  ]],
  ['apps/browser-companion/src/App.svelte', ['sendPreparationHeartbeat,500', '()=>{if(owner)voidmaintainConnection();},250']],
  ['apps/browser-companion/src/connection-worker.ts', ['()=>voidconnection.tick().catch(stop),250']],
]);

function scripts(file, source) {
  if (!file.endsWith('.svelte')) return [source];
  const { instance, module } = parse(source, { modern: true });
  return [instance, module].filter(Boolean).map(script =>
    source.slice(script.content.start, script.content.end));
}

export function checkUiIntervals(sources) {
  const violations = [];
  for (const [file, source] of sources) {
    for (const dormant of ['Esp32Companions', 'ProfileDesktopControls']) {
      if (file !== `src/lib/${dormant}.svelte` && source.includes(dormant)) {
        violations.push(`${file}: dormant ${dormant} has a production reference`);
      }
    }
    const expected = [...(allowed.get(file) ?? [])];
    for (const script of scripts(file, source)) {
      const ast = ts.createSourceFile(file, script, ts.ScriptTarget.Latest, true,
        file.endsWith('.svelte') ? ts.ScriptKind.TS : undefined);
      function visit(node) {
        if (ts.isCallExpression(node)) {
          const callee = node.expression;
          const interval = ts.isIdentifier(callee) && callee.text === 'setInterval'
            || ts.isPropertyAccessExpression(callee) && callee.name.text === 'setInterval'
            || ts.isElementAccessExpression(callee) && ts.isStringLiteral(callee.argumentExpression)
              && callee.argumentExpression.text === 'setInterval';
          if (interval) {
            const argumentsText = node.arguments.map(argument => argument.getText().replace(/\s+/g, '')).join(',');
            const index = expected.indexOf(argumentsText);
            if (index === -1) {
              violations.push(`${file}: unclassified interval ${node.getText().replace(/\s+/g, ' ')}`);
            } else expected.splice(index, 1);
          }
        }
        ts.forEachChild(node, visit);
      }
      visit(ast);
    }
    if (expected.length) violations.push(`${file}: remove ${expected.length} stale interval entries`);
  }
  if (violations.length) throw new Error(violations.join('\n'));
}

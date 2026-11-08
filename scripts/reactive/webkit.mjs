import {until} from './cdp.mjs';

// WebKitGTK's HTTP inspector serves the target list and a socket per WebView.
export async function webKitPage(endpoint) {
  const origin = new URL(endpoint);
  if (origin.protocol !== 'http:' || origin.hostname !== '127.0.0.1') {
    throw Error('Use a private loopback WebKit inspector.');
  }
  const path = await until(async () => {
    const response = await fetch(origin, {signal:AbortSignal.timeout(1000)}).catch(() => null);
    if (!response?.ok) return false;
    const rows = (await response.text()).matchAll(/<tr>(.*?)<\/tr>/gs);
    for (const [, row] of rows) {
      if (/<div class="targeturl">(?:http:\/\/127\.0\.0\.1:1420|tauri:\/\/localhost)\/?<\/div>/.test(row)) {
        return row.match(/\/socket\/\d+\/\d+\/WebPage/)?.[0];
      }
    }
  });
  const socket = new WebSocket(new URL(path, `ws://${origin.host}`));
  const pending = new Map();
  let pageTarget, next = 0;
  const settle = message => {
    const task = pending.get(message.id);
    if (!task) return;
    clearTimeout(task.timer);
    pending.delete(message.id);
    if (message.error) task.reject(Error(message.error.message));
    else task.resolve(message.result);
  };
  socket.addEventListener('message', event => {
    const message = JSON.parse(event.data);
    if (message.method === 'Target.targetCreated' && message.params.targetInfo.type === 'page') {
      pageTarget = message.params.targetInfo.targetId;
    } else if (message.method === 'Target.dispatchMessageFromTarget' && message.params.targetId === pageTarget) {
      settle(JSON.parse(message.params.message));
    } else if (message.error) settle(message);
  });
  socket.addEventListener('close', () => {
    for (const id of pending.keys()) settle({id, error:{message:'WebKit inspector closed.'}});
  });
  try {
    await until(() => {
      if (socket.readyState === WebSocket.CLOSED) throw Error('WebKit inspector connection failed.');
      return pageTarget;
    });
  } catch (error) {
    socket.close();
    throw error;
  }
  const call = (method, params) => new Promise((resolve, reject) => {
    const id = ++next;
    const timer = setTimeout(() => {
      pending.delete(id);
      reject(Error(`${method} timed out.`));
    }, 15_000);
    pending.set(id, {resolve, reject, timer});
    socket.send(JSON.stringify({id, method:'Target.sendMessageToTarget', params:{
      targetId:pageTarget, message:JSON.stringify({id, method, params}),
    }}));
  });
  return {
    async evaluate(expression) {
      const wrapped = `(() => {
        const value = (${expression});
        const encode = result => JSON.stringify([result]);
        return value instanceof Promise ? value.then(encode) : encode(value);
      })()`;
      let reply = await call('Runtime.evaluate', {expression:wrapped});
      if (!reply.wasThrown && reply.result.objectId) {
        reply = await call('Runtime.awaitPromise', {promiseObjectId:reply.result.objectId, returnByValue:true});
      }
      if (reply.wasThrown) throw Error(reply.result.description);
      return JSON.parse(reply.result.value)[0];
    },
    close() { socket.close(); },
  };
}

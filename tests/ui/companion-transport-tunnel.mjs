// The browser still connects to the real loopback listener with its original
// origin and TLS identity. Destroying both sockets simulates transport failure
// without a WebSocket close frame or an application unload.
import {createServer} from 'node:http';
import {connect} from 'node:net';
import {once} from 'node:events';

export async function openCompanionTunnel(target) {
  if (target.hostname !== '127.0.0.1' || !target.port) throw new Error('Expected a private companion listener.');
  let online = true, established = 0;
  const clients = new Set();
  const server = createServer((_request, response) => {
    response.writeHead(403).end();
  });
  server.on('connection', socket => {
    clients.add(socket);
    socket.once('close', () => clients.delete(socket));
  });
  server.on('connect', (request, client, head) => {
    if (!online || request.url !== target.host) {
      client.end('HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n');
      return;
    }
    const upstream = connect({host:target.hostname,port:Number(target.port)});
    const close = () => {client.destroy();upstream.destroy();};
    client.once('close',close);
    client.once('error',close);
    upstream.once('close',close);
    upstream.once('error',close);
    upstream.once('connect', () => {
      if (!online || client.destroyed) return close();
      established++;
      client.write('HTTP/1.1 200 Connection Established\r\n\r\n');
      if (head.length) upstream.write(head);
      client.pipe(upstream);
      upstream.pipe(client);
    });
  });
  server.listen(0,'127.0.0.1');
  await once(server,'listening');
  const drop = () => {
    online = false;
    for (const socket of clients) socket.destroy();
  };
  return {
    browserArgs:[`--proxy-server=http://127.0.0.1:${server.address().port}`,'--proxy-bypass-list=<-loopback>'],
    get established() {return established;},
    drop,
    restore() {online = true;},
    async close() {
      drop();
      await new Promise((resolve,reject) => server.close(error => error ? reject(error) : resolve()));
    },
  };
}

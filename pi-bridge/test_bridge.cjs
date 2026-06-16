const WebSocket = require('ws');

const port = process.argv[2];
const sessionPath = process.argv[3];
const ws = new WebSocket(`ws://127.0.0.1:${port}/`);
ws.on('open', () => {
  ws.send(JSON.stringify({ type: 'create_session', sessionId: 's1', sessionPath, cwd: '/Users/juju/Develop/mini-pi' }));
  setTimeout(() => {
    ws.send(JSON.stringify({ type: 'get_messages', sessionId: 's1', id: 'm1' }));
  }, 2000);
});
ws.on('message', (data) => {
  const msg = JSON.parse(data.toString());
  console.log('recv:', JSON.stringify(msg).slice(0, 800));
});
ws.on('error', (e) => console.error('ws error:', e.message));

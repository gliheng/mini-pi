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
  if (msg.command === 'get_messages' && msg.success) {
    const msgs = msg.data.messages;
    console.log('total messages:', msgs.length);
    for (const m of msgs) {
      const text = m.content?.find(c => c.type === 'text')?.text || m.content?.[0]?.text || '(no text)';
      console.log(`${m.id} ${m.role}: ${text.slice(0, 60)}`);
    }
  } else {
    console.log('recv:', JSON.stringify(msg).slice(0, 200));
  }
});
ws.on('error', (e) => console.error('ws error:', e.message));

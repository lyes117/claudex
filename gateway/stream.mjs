export async function* sseEvents(body) {
  const decoder = new TextDecoder('utf-8', { fatal: true });
  let buffer = '';
  for await (const chunk of body) {
    buffer += decoder.decode(chunk, { stream: true });
    if (buffer.length > 16 * 1024 * 1024) throw new Error('SSE event exceeded the safety limit');
    let boundary;
    while ((boundary = /\r?\n\r?\n/.exec(buffer))) {
      const frame = buffer.slice(0, boundary.index); buffer = buffer.slice(boundary.index + boundary[0].length);
      const data = frame.split(/\r?\n/).filter(line => line.startsWith('data:')).map(line => line.slice(5).trimStart()).join('\n');
      if (data && data !== '[DONE]') yield JSON.parse(data);
    }
  }
  buffer += decoder.decode();
  if (!buffer.split(/\r?\n/).every(line => !line.trim() || line.startsWith(':'))) throw new Error('Truncated SSE frame');
}

export async function writeEvent(response, event) {
  if (response.destroyed) throw new Error('Client disconnected');
  if (!response.write(event)) {
    await new Promise((resolve, reject) => {
      const cleanup = () => { response.off('drain', drained); response.off('close', closed); response.off('error', closed); };
      const drained = () => { cleanup(); resolve(); };
      const closed = () => { cleanup(); reject(new Error('Client disconnected')); };
      response.once('drain', drained); response.once('close', closed); response.once('error', closed);
    });
  }
}

export function validateToolItem(item, tools) {
  if (item.type !== 'function_call') return;
  if (typeof item.call_id !== 'string' || !item.call_id) throw new Error('Missing tool call ID');
  if (!tools?.some(tool => tool.name === item.name)) throw new Error('Provider returned an unknown tool');
  let args; try { args = JSON.parse(item.arguments); } catch { throw new Error('Provider returned malformed tool arguments'); }
  if (!args || typeof args !== 'object' || Array.isArray(args)) throw new Error('Tool arguments must be an object');
}

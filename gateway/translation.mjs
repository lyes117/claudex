import converter from '@caixiaoshun/claudex/dist/src/converter.js';
export const { estimateRequestTokens } = converter;
const signaturePrefix = 'claudex-openai:';

function reasoningBlock(item) {
  return { type: 'thinking', thinking: (item.summary || []).filter(part => typeof part.text === 'string').map(part => part.text).join('\n'), signature: signaturePrefix + Buffer.from(JSON.stringify(item)).toString('base64url') };
}

function searchResults(output) {
  const found = new Map();
  for (const item of output) for (const part of item.content || []) for (const annotation of part.annotations || []) {
    if (annotation.type !== 'url_citation' || typeof annotation.url !== 'string') continue;
    let url; try { url = new URL(annotation.url); } catch { continue; }
    if (!['https:', 'http:'].includes(url.protocol)) continue;
    found.set(url.href, { type: 'web_search_result', url: url.href, title: annotation.title || url.hostname, encrypted_content: '' });
  }
  return [...found.values()];
}

function searchUse(item) {
  return { type: 'server_tool_use', id: item.id, name: 'web_search', input: { query: item.action?.query || item.action?.queries?.join(' ') || '' } };
}

export class StreamConverter extends converter.StreamConverter {
  toolStreams = new Map();
  reasoningId;
  processEvent(type, event) {
    const item = event.item;
    if (['response.completed', 'response.incomplete'].includes(type) && (this.toolStreams.size || this.reasoningId !== undefined)) throw new Error('Stream ended without a completed item');
    if (type === 'response.reasoning_summary_text.delta' && typeof event.delta === 'string') {
      const frames = [...this.ensureStarted()];
      if (!this.blockOpen || this.reasoningId !== event.item_id) {
        frames.push(...this.closeBlock());
        this.reasoningId = event.item_id; this.blockOpen = true;
        frames.push(this.sse('content_block_start', { type: 'content_block_start', index: this.blockIndex, content_block: { type: 'thinking', thinking: '' } }));
      }
      frames.push(this.sse('content_block_delta', { type: 'content_block_delta', index: this.blockIndex, delta: { type: 'thinking_delta', thinking: event.delta } }));
      return frames;
    }
    if (type === 'response.output_item.added' && item?.type === 'function_call' && item.id && item.call_id && item.name) {
      const frames = [...this.ensureStarted(), ...this.closeBlock()];
      const index = this.blockIndex++;
      this.toolStreams.set(item.id, { index, arguments: '' });
      frames.push(this.sse('content_block_start', { type: 'content_block_start', index, content_block: { type: 'tool_use', id: item.call_id, name: item.name, input: {} } }));
      return frames;
    }
    const tool = this.toolStreams.get(event.item_id || item?.id);
    if (type === 'response.function_call_arguments.delta' && tool && typeof event.delta === 'string') {
      tool.arguments += event.delta;
      return [this.sse('content_block_delta', { type: 'content_block_delta', index: tool.index, delta: { type: 'input_json_delta', partial_json: event.delta } })];
    }
    if (type === 'response.output_item.done' && item?.type === 'function_call' && tool) {
      const frames = [];
      if (tool.arguments && tool.arguments !== item.arguments) throw new Error('Tool argument stream differs from completion');
      if (!tool.arguments) frames.push(this.sse('content_block_delta', { type: 'content_block_delta', index: tool.index, delta: { type: 'input_json_delta', partial_json: item.arguments } }));
      frames.push(this.sse('content_block_stop', { type: 'content_block_stop', index: tool.index }));
      this.toolStreams.delete(item.id);
      return frames;
    }
    if (type === 'response.output_item.done' && item?.type === 'web_search_call') {
      const frames = [...this.ensureStarted(), ...this.closeBlock(), this.sse('content_block_start', { type: 'content_block_start', index: this.blockIndex, content_block: searchUse(item) }), this.sse('content_block_stop', { type: 'content_block_stop', index: this.blockIndex++ })];
      return frames;
    }
    if (type === 'response.completed') {
      const output = event.response.output, results = searchResults(output);
      const frames = [...this.ensureStarted(), ...this.closeBlock()];
      for (const call of output.filter(value => value.type === 'web_search_call')) {
        frames.push(this.sse('content_block_start', { type: 'content_block_start', index: this.blockIndex, content_block: { type: 'web_search_tool_result', tool_use_id: call.id, content: results } }), this.sse('content_block_stop', { type: 'content_block_stop', index: this.blockIndex++ }));
      }
      return [...frames, ...super.processEvent(type, event)];
    }
    if (type === 'response.output_item.done' && item?.type === 'reasoning' && this.blockOpen && this.reasoningId === item.id) {
      if (!item.encrypted_content) throw new Error('Streamed reasoning ended without its signature');
      this.reasoningId = undefined;
      return [this.sse('content_block_delta', { type: 'content_block_delta', index: this.blockIndex, delta: { type: 'signature_delta', signature: reasoningBlock(item).signature } }), ...this.closeBlock()];
    }
    if (type !== 'response.output_item.done' || item?.type !== 'reasoning' || !item.encrypted_content) return super.processEvent(type, event);
    const block = reasoningBlock(item);
    const frames = [...this.ensureStarted(), ...this.closeBlock(), this.sse('content_block_start', { type: 'content_block_start', index: this.blockIndex, content_block: { type: 'thinking', thinking: '', signature: '' } })];
    if (block.thinking) frames.push(this.sse('content_block_delta', { type: 'content_block_delta', index: this.blockIndex, delta: { type: 'thinking_delta', thinking: block.thinking } }));
    frames.push(this.sse('content_block_delta', { type: 'content_block_delta', index: this.blockIndex, delta: { type: 'signature_delta', signature: block.signature } }), this.sse('content_block_stop', { type: 'content_block_stop', index: this.blockIndex++ }));
    return frames;
  }
}

export function codexToAnthropic(response, model) {
  const result = converter.codexToAnthropic(response, model);
  result.content.unshift(...response.output.filter(item => item.type === 'reasoning' && item.encrypted_content).map(reasoningBlock));
  const results = searchResults(response.output);
  for (const call of response.output.filter(item => item.type === 'web_search_call')) result.content.unshift(searchUse(call), { type: 'web_search_tool_result', tool_use_id: call.id, content: results });
  return result;
}

export function translateRequest(request, route) {
  // Keep the shared converter; correct its optional-schema and image-result losses.
  const validateBlock = block => {
    if (!block || !['text', 'image', 'tool_use', 'tool_result', 'thinking', 'redacted_thinking', 'server_tool_use', 'web_search_tool_result'].includes(block.type)) {
      throw Object.assign(new Error('Unsupported content block; convert documents to text or images before inference.'), { status: 400 });
    }
    if (block.type === 'tool_result' && Array.isArray(block.content)) block.content.forEach(validateBlock);
  };
  const messages = request.messages.map(message => {
    if (!Array.isArray(message.content)) return message;
    const content = [];
    for (const block of message.content) {
      validateBlock(block);
      if (block.type === 'server_tool_use' && block.name === 'web_search') { content.push({ type: 'text', text: `Web search: ${JSON.stringify(block.input)}` }); continue; }
      if (block.type === 'web_search_tool_result') { content.push({ type: 'text', text: `Web search results: ${JSON.stringify(block.content)}` }); continue; }
      content.push(block);
      if (block.type === 'tool_result' && Array.isArray(block.content)) {
        content.push(...block.content.filter(part => part.type === 'image'));
      }
    }
    return { ...message, content };
  });
  const result = converter.anthropicToCodex({ ...request, messages, model: `claudex:${route.model}:${route.effort || 'medium'}`, stream: true });
  result.include = ['reasoning.encrypted_content'];
  result.input = messages.flatMap(message => {
    const reasoning = [];
    if (message.role === 'assistant' && Array.isArray(message.content)) {
      for (const block of message.content) {
        if (block.type !== 'thinking' || !block.signature?.startsWith(signaturePrefix)) continue;
        const item = JSON.parse(Buffer.from(block.signature.slice(signaturePrefix.length), 'base64url').toString('utf8'));
        if (item.type !== 'reasoning' || typeof item.encrypted_content !== 'string') throw Object.assign(new Error('Invalid OpenAI reasoning state'), { status: 400 });
        reasoning.push({ type: 'reasoning', ...(item.id ? { id: item.id } : {}), encrypted_content: item.encrypted_content, summary: [] });
      }
    }
    return [...reasoning, ...converter.anthropicToCodex({ model: `claudex:${route.model}`, messages: [message], stream: true }).input];
  });
  const effort = request.output_config?.effort;
  result.reasoning = { effort: ['none', 'minimal', 'low', 'medium', 'high', 'xhigh'].includes(effort) ? effort : effort === 'max' ? 'xhigh' : route.effort || 'medium', summary: 'auto' };
  if (request.tools?.length) {
    result.tools = request.tools.map(tool => {
      if (tool.name === 'web_search' && /^web_search_/.test(tool.type || '')) {
        if (tool.max_uses !== undefined) {
          if (!Number.isInteger(tool.max_uses) || tool.max_uses < 1) throw Object.assign(new Error('WebSearch max_uses must be a positive integer.'), { status: 400 });
          // The subscription endpoint rejects max_tool_calls. Enforce the limit
          // in the gateway stream and also tell the model before it searches.
          result.instructions += `\nWeb search limit: invoke web_search at most ${tool.max_uses} times in this response.`;
        }
        const filters = Object.fromEntries(['allowed_domains', 'blocked_domains'].filter(key => tool[key] !== undefined).map(key => [key, tool[key]]));
        return { type: 'web_search', ...(Object.keys(filters).length ? { filters } : {}), ...(tool.user_location ? { user_location: tool.user_location } : {}) };
      }
      if (!tool.name || !tool.input_schema) throw Object.assign(new Error('Unsupported server-side tool; use local Claude Code tools.'), { status: 400 });
      return { type: 'function', name: tool.name, description: tool.description || '', parameters: tool.input_schema, strict: false };
    });
    const choice = request.tool_choice;
    result.tool_choice = choice?.type === 'any' ? 'required' : choice?.type === 'tool' ? request.tools.some(tool => tool.name === choice.name && /^web_search_/.test(tool.type || '')) ? { type: 'web_search' } : { type: 'function', name: choice.name } : choice?.type === 'none' ? 'none' : 'auto';
    result.parallel_tool_calls = !choice?.disable_parallel_tool_use;
  }
  if (request.output_config?.format?.type === 'json_schema') {
    result.text = { format: { type: 'json_schema', name: 'claudex_output', schema: request.output_config.format.schema, strict: false } };
  }
  return result;
}

export function fixStreamFrame(frame, usage) {
  if (!frame.includes('"tool_use"') && !(usage && frame.startsWith('event: message_delta'))) return frame;
  const data = JSON.parse(frame.slice(frame.indexOf('data: ') + 6).trim());
  if (data.type === 'content_block_start' && data.content_block?.type === 'tool_use') data.content_block.input = {};
  if (data.type === 'message_delta' && usage) data.usage = { ...data.usage, ...translateUsage(usage) };
  return `event: ${data.type}\ndata: ${JSON.stringify(data)}\n\n`;
}

export function translateUsage(usage = {}) {
  const cached = usage.input_tokens_details?.cached_tokens || 0;
  return { input_tokens: Math.max(0, (usage.input_tokens || 0) - cached), cache_read_input_tokens: cached, output_tokens: usage.output_tokens || 0 };
}

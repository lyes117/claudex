//! Original limited DSL adapted to the existing local workflow call contract.
pub(super) const SOURCE: &str = r#"
const { args, agent, phase, parallel, pipeline, log } = (() => {
  const call = globalThis.__workflowAgent;
  const setPhase = globalThis.__workflowPhase;
  const emitLog = globalThis.__workflowLog;
  const args = globalThis.__workflowArguments;
  delete globalThis.__workflowAgent;
  delete globalThis.__workflowPhase;
  delete globalThis.__workflowLog;
  delete globalThis.__workflowArguments;
  let index = 0;
  let currentPhase = 'Workflow';
  const phase = title => { setPhase(title); currentPhase = title; };
  const agent = (prompt, options = {}) => {
    if (typeof prompt !== 'string' || !prompt.trim()) throw Error('invalid workflow prompt');
    if (!options || typeof options !== 'object' || Array.isArray(options)) throw Error('invalid workflow options');
    for (const key of Object.keys(options)) {
      if (!['label', 'phase', 'schema', 'model', 'effort'].includes(key)) throw Error('unsupported workflow option');
    }
    const position = ++index;
    return call({ name: options.label || `Agent ${position}`, prompt,
      phase: options.phase || currentPhase, role: null, schema: options.schema ?? null,
      model: options.model ?? null, effort: options.effort ?? null });
  };
  const parallel = tasks => {
    if (!Array.isArray(tasks) || tasks.length > 64) throw Error('workflow group limit');
    return Promise.all(tasks.map(task => typeof task === 'function' ? task() : task));
  };
  const pipeline = (items, task) => {
    if (!Array.isArray(items) || items.length > 64 || typeof task !== 'function') throw Error('invalid workflow pipeline');
    return parallel(items.map((item, index) => () => task(item, index)));
  };
  const log = message => emitLog(message);
  Math.random = () => { throw Error('Pass randomness through args'); };
  // Explicit dates are deterministic; wall-clock access is refused, including
  // through Date.prototype.constructor and subclasses.
  const NativeDate = globalThis.Date;
  const explicitValue = value => {
    if (typeof value === 'number' && Number.isFinite(value)) return value;
    if (typeof value === 'string' && /^\d{4}-\d{2}-\d{2}(?:T\d{2}:\d{2}(?::\d{2}(?:\.\d{1,3})?)?(?:Z|[+-]\d{2}:\d{2}))?$/.test(value)) return value;
    throw Error('Use an explicit ISO date with timezone or epoch milliseconds');
  };
  function ExplicitDate(...values) {
    if (!new.target || values.length === 0) throw Error('Pass the date through args');
    const value = values.length === 1 ? explicitValue(values[0]) : NativeDate.UTC(...values);
    return Reflect.construct(NativeDate, [value], new.target);
  }
  ExplicitDate.prototype = NativeDate.prototype;
  Object.defineProperty(NativeDate.prototype, 'constructor', { value: ExplicitDate, writable: false, configurable: false });
  ExplicitDate.parse = value => NativeDate.parse(explicitValue(value));
  ExplicitDate.UTC = NativeDate.UTC;
  ExplicitDate.now = () => { throw Error('Pass the date through args'); };
  globalThis.Date = ExplicitDate;
  for (const suffix of ['FullYear', 'Month', 'Date', 'Day', 'Hours', 'Minutes', 'Seconds', 'Milliseconds']) {
    NativeDate.prototype[`get${suffix}`] = NativeDate.prototype[`getUTC${suffix}`];
    if (suffix !== 'Day') NativeDate.prototype[`set${suffix}`] = NativeDate.prototype[`setUTC${suffix}`];
  }
  NativeDate.prototype.getTimezoneOffset = () => 0;
  NativeDate.prototype.getYear = function() { return this.getUTCFullYear() - 1900; };
  NativeDate.prototype.setYear = function(value) { return this.setUTCFullYear(value >= 0 && value < 100 ? value + 1900 : value); };
  for (const name of ['toString', 'toLocaleString', 'toLocaleDateString', 'toLocaleTimeString']) {
    NativeDate.prototype[name] = NativeDate.prototype.toISOString;
  }
  NativeDate.prototype.toDateString = function() { return this.toISOString().slice(0, 10); };
  NativeDate.prototype.toTimeString = function() { return this.toISOString().slice(11); };
  globalThis.Intl = undefined;
  globalThis.Temporal = undefined;
  globalThis.performance = undefined;
  globalThis.Atomics = undefined;
  globalThis.SharedArrayBuffer = undefined;
  globalThis.WebAssembly = undefined;
  globalThis.workflow = Object.freeze({agent, phase, parallel, pipeline, log});
  return { args, agent, phase, parallel, pipeline, log };
})();
"#;

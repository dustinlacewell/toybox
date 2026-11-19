// Globals installed by the engine. Evaluated once per engine as a function
// expression; `native` is the small set of Rust helpers it needs.
// Returns { tick } for the Rust event loop.
(function (native) {
  "use strict";
  const g = globalThis;
  const define = (name, value) =>
    Object.defineProperty(g, name, { value, writable: true, configurable: true, enumerable: false });

  // ---------------------------------------------------------------- console
  const errText = (e) => {
    let head = (e.name || "Error") + (e.message ? ": " + e.message : "");
    return e.stack ? head + "\n" + String(e.stack).replace(/\s+$/, "") : head;
  };
  const inspect = (v, depth, seen) => {
    switch (typeof v) {
      case "string": return depth ? JSON.stringify(v) : v;
      case "bigint": return v + "n";
      case "symbol": return v.toString();
      case "function": return "[Function" + (v.name ? ": " + v.name : " (anonymous)") + "]";
      case "number": return Object.is(v, -0) ? "-0" : String(v);
      case "object": break;
      default: return String(v);
    }
    if (v === null) return "null";
    if (v instanceof Error) return depth ? "[" + (v.name || "Error") + ": " + v.message + "]" : errText(v);
    if (seen.indexOf(v) >= 0) return "[Circular]";
    if (v instanceof Date) return isNaN(v.getTime()) ? "Invalid Date" : v.toISOString();
    if (v instanceof RegExp) return String(v);
    if (ArrayBuffer.isView(v) || v instanceof ArrayBuffer) {
      const n = v.byteLength;
      return (v.constructor && v.constructor.name || "Buffer") + "(" + (ArrayBuffer.isView(v) && !(v instanceof DataView) ? v.length : n) + ")";
    }
    if (depth > 2) return Array.isArray(v) ? "[Array]" : "[Object]";
    seen.push(v);
    try {
      if (Array.isArray(v)) {
        const items = [];
        for (let i = 0; i < v.length && i < 100; i++) items.push(inspect(v[i], depth + 1, seen));
        if (v.length > 100) items.push("... " + (v.length - 100) + " more items");
        return items.length ? "[ " + items.join(", ") + " ]" : "[]";
      }
      if (v instanceof Map) {
        const items = [];
        for (const [k, x] of v) items.push(inspect(k, depth + 1, seen) + " => " + inspect(x, depth + 1, seen));
        return "Map(" + v.size + ") { " + items.join(", ") + " }";
      }
      if (v instanceof Set) {
        const items = [];
        for (const x of v) items.push(inspect(x, depth + 1, seen));
        return "Set(" + v.size + ") { " + items.join(", ") + " }";
      }
      const keys = Object.keys(v);
      const proto = Object.getPrototypeOf(v);
      const tag = proto && proto !== Object.prototype && proto.constructor && proto.constructor.name
        ? proto.constructor.name + " " : "";
      if (!keys.length) return tag + "{}";
      const items = keys.slice(0, 100).map((k) =>
        (/^[A-Za-z_$][\w$]*$/.test(k) ? k : JSON.stringify(k)) + ": " + inspect(v[k], depth + 1, seen));
      return tag + "{ " + items.join(", ") + " }";
    } catch (_) {
      return Object.prototype.toString.call(v);
    } finally {
      seen.pop();
    }
  };
  const format = (args) => {
    if (!args.length) return "";
    let out = "";
    let i = 0;
    if (typeof args[0] === "string" && args[0].indexOf("%") >= 0 && args.length > 1) {
      i = 1;
      out = args[0].replace(/%[sdifjoOc%]/g, (m) => {
        if (m === "%%") return "%";
        if (i >= args.length) return m;
        const a = args[i++];
        switch (m) {
          case "%s": return typeof a === "string" ? a : inspect(a, 1, []);
          case "%d": return typeof a === "bigint" ? a + "n" : String(Number(a));
          case "%i": return typeof a === "bigint" ? a + "n" : String(parseInt(a, 10));
          case "%f": return String(parseFloat(a));
          case "%j": try { return JSON.stringify(a); } catch (_) { return "[Circular]"; }
          case "%c": return "";
          default: return inspect(a, 1, []);
        }
      });
    } else {
      out = inspect(args[0], 0, []);
      i = 1;
    }
    for (; i < args.length; i++) out += " " + inspect(args[i], 0, []);
    return out;
  };
  const sink = (level) => (...args) => { native.log(level, format(args)); };
  const consoleObj = {
    log: sink(0), info: sink(1), warn: sink(2), error: sink(3), debug: sink(4),
    trace: sink(3),
    assert(cond, ...args) { if (!cond) native.log(3, "Assertion failed" + (args.length ? ": " + format(args) : "")); },
  };
  for (const k of Object.keys(consoleObj)) Object.defineProperty(consoleObj, k, { enumerable: true });
  define("console", consoleObj);

  // ----------------------------------------------------------------- timers
  let nextTimerId = 1;
  let timerSeq = 0;
  const timers = new Map();
  const addTimer = (fn, delay, args, repeat) => {
    if (typeof fn !== "function") throw new TypeError('The "callback" argument must be of type function');
    delay = Number(delay);
    if (!(delay >= 0)) delay = 0;
    if (delay > 2147483647) delay = 1;
    const id = nextTimerId++;
    timers.set(id, { id, fn, args, repeat: repeat ? Math.max(delay, 1) : -1, due: native.now() + delay, seq: timerSeq++ });
    return id;
  };
  const clearTimer = (id) => { timers.delete(Number(id)); };
  define("setTimeout", function setTimeout(fn, delay, ...args) { return addTimer(fn, delay, args, false); });
  define("setInterval", function setInterval(fn, delay, ...args) { return addTimer(fn, delay, args, true); });
  define("clearTimeout", function clearTimeout(id) { clearTimer(id); });
  define("clearInterval", function clearInterval(id) { clearTimer(id); });
  define("queueMicrotask", function queueMicrotask(fn) {
    if (typeof fn !== "function") throw new TypeError('The "callback" argument must be of type function');
    Promise.resolve().then(() => {
      try { fn(); } catch (e) { addTimer(() => { throw e; }, 0, [], false); }
    });
  });
  // Runs at most one due timer. Returns the milliseconds until the next timer
  // is due (0 when one is due now), or -1 when there are none.
  const tick = () => {
    const now = native.now();
    let best = null;
    for (const t of timers.values()) {
      if (t.due <= now && (best === null || t.due < best.due || (t.due === best.due && t.seq < best.seq))) best = t;
    }
    if (best !== null) {
      if (best.repeat >= 0) { best.due = now + best.repeat; best.seq = timerSeq++; } else timers.delete(best.id);
      best.fn(...best.args);
    }
    let next = Infinity;
    const after = native.now();
    for (const t of timers.values()) if (t.due < next) next = t.due;
    return next === Infinity ? -1 : Math.max(0, next - after);
  };

  // ------------------------------------------------------------ performance
  const origin = Date.now() - native.now();
  define("performance", {
    now() { return native.now(); },
    get timeOrigin() { return origin; },
    toJSON() { return { timeOrigin: origin }; },
  });

  // ------------------------------------------------------------- base64
  const B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  const invalidChar = (msg) => { const e = new Error(msg); e.name = "InvalidCharacterError"; e.code = 5; return e; };
  define("btoa", function btoa(data) {
    if (arguments.length === 0) throw new TypeError("btoa requires 1 argument");
    const s = String(data);
    const parts = [];
    let chunk = "";
    for (let i = 0; i < s.length; i += 3) {
      const a = s.charCodeAt(i);
      const b = i + 1 < s.length ? s.charCodeAt(i + 1) : NaN;
      const c = i + 2 < s.length ? s.charCodeAt(i + 2) : NaN;
      if (a > 255 || b > 255 || c > 255) throw invalidChar("The string to be encoded contains characters outside of the Latin1 range.");
      const n = (a << 16) | ((b || 0) << 8) | (c || 0);
      chunk += B64[(n >> 18) & 63] + B64[(n >> 12) & 63] + (isNaN(b) ? "=" : B64[(n >> 6) & 63]) + (isNaN(c) ? "=" : B64[n & 63]);
      if (chunk.length > 65536) { parts.push(chunk); chunk = ""; }
    }
    parts.push(chunk);
    return parts.join("");
  });
  define("atob", function atob(data) {
    if (arguments.length === 0) throw new TypeError("atob requires 1 argument");
    let s = String(data).replace(/[\t\n\f\r ]+/g, "");
    if (s.length % 4 === 0) s = s.replace(/==?$/, "");
    if (s.length % 4 === 1 || /[^A-Za-z0-9+/]/.test(s)) throw invalidChar("The string to be decoded is not correctly encoded.");
    const parts = [];
    let chunk = "";
    let bits = 0, acc = 0;
    for (let i = 0; i < s.length; i++) {
      acc = (acc << 6) | B64.indexOf(s[i]);
      bits += 6;
      if (bits >= 8) {
        bits -= 8;
        chunk += String.fromCharCode((acc >> bits) & 255);
        acc &= (1 << bits) - 1;
      }
      if (chunk.length > 65536) { parts.push(chunk); chunk = ""; }
    }
    parts.push(chunk);
    return parts.join("");
  });

  // ------------------------------------------------- TextEncoder / Decoder
  const wellFormed = (s) =>
    typeof s.toWellFormed === "function"
      ? s.toWellFormed()
      : s.replace(/[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/g, "�");
  const toBytes = (input) => {
    if (input === undefined) return new Uint8Array(0);
    if (input instanceof ArrayBuffer) return new Uint8Array(input);
    if (ArrayBuffer.isView(input)) return new Uint8Array(input.buffer, input.byteOffset, input.byteLength);
    throw new TypeError('The "input" argument must be an instance of ArrayBuffer or ArrayBufferView.');
  };
  class TextEncoder {
    get encoding() { return "utf-8"; }
    encode(input = "") { return native.utf8Encode(wellFormed(String(input))); }
    encodeInto(source, destination) {
      const bytes = native.utf8Encode(wellFormed(String(source)));
      let n = Math.min(bytes.length, destination.length);
      while (n < bytes.length && n > 0 && (bytes[n] & 0xc0) === 0x80) n--;
      destination.set(bytes.subarray(0, n));
      const read = n === bytes.length ? String(source).length : native.utf8Decode(bytes.subarray(0, n), false).length;
      return { read, written: n };
    }
  }
  Object.defineProperty(TextEncoder.prototype, Symbol.toStringTag, { value: "TextEncoder", configurable: true });

  const labels = {
    "utf-8": "utf-8", "utf8": "utf-8", "unicode-1-1-utf-8": "utf-8",
    "utf-16le": "utf-16le", "utf-16": "utf-16le", "ucs-2": "utf-16le", "unicode": "utf-16le",
    "csunicode": "utf-16le", "iso-10646-ucs-2": "utf-16le", "unicodefeff": "utf-16le",
  };
  const decState = new WeakMap();
  class TextDecoder {
    constructor(label = "utf-8", options = {}) {
      const enc = labels[String(label).trim().toLowerCase()];
      if (!enc) throw new RangeError('The "' + label + '" encoding is not supported');
      decState.set(this, {
        enc, fatal: !!(options && options.fatal), ignoreBOM: !!(options && options.ignoreBOM),
        pending: null, started: false,
      });
    }
    get encoding() { return decState.get(this).enc; }
    get fatal() { return decState.get(this).fatal; }
    get ignoreBOM() { return decState.get(this).ignoreBOM; }
    decode(input, options) {
      const st = decState.get(this);
      if (!st) throw new TypeError("Illegal invocation");
      const stream = !!(options && options.stream);
      let bytes = toBytes(input);
      if (st.pending) {
        const joined = new Uint8Array(st.pending.length + bytes.length);
        joined.set(st.pending, 0);
        joined.set(bytes, st.pending.length);
        bytes = joined;
        st.pending = null;
      }
      let text;
      if (st.enc === "utf-8") {
        let end = bytes.length;
        if (stream) {
          for (let k = 1; k <= 3 && k <= bytes.length; k++) {
            const b = bytes[bytes.length - k];
            if ((b & 0xc0) === 0x80) continue;
            const need = b >= 0xf0 ? 4 : b >= 0xe0 ? 3 : b >= 0xc0 ? 2 : 1;
            if (need > k) end = bytes.length - k;
            break;
          }
          if (end < bytes.length) { st.pending = bytes.slice(end); bytes = bytes.subarray(0, end); }
        }
        if (!st.started && bytes.length >= 3 && bytes[0] === 0xef && bytes[1] === 0xbb && bytes[2] === 0xbf && !st.ignoreBOM) {
          bytes = bytes.subarray(3);
        }
        text = native.utf8Decode(bytes, st.fatal);
        if (text == null) { st.pending = null; st.started = false; throw new TypeError("The encoded data was not valid utf-8"); }
      } else {
        if (stream && (bytes.length & 1)) { st.pending = bytes.slice(bytes.length - 1); bytes = bytes.subarray(0, bytes.length - 1); }
        if (!st.started && bytes.length >= 2 && bytes[0] === 0xff && bytes[1] === 0xfe && !st.ignoreBOM) bytes = bytes.subarray(2);
        const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
        const parts = [];
        const units = bytes.length >> 1;
        for (let i = 0; i < units; i += 8192) {
          const n = Math.min(8192, units - i);
          const codes = new Array(n);
          for (let j = 0; j < n; j++) codes[j] = view.getUint16((i + j) * 2, true);
          parts.push(String.fromCharCode.apply(null, codes));
        }
        text = parts.join("");
        if (bytes.length & 1) text += "�";
        const fixed = wellFormed(text);
        if (fixed !== text) {
          if (st.fatal) { st.started = false; throw new TypeError("The encoded data was not valid utf-16le"); }
          text = fixed;
        }
        if (st.fatal && (bytes.length & 1)) { st.started = false; throw new TypeError("The encoded data was not valid utf-16le"); }
      }
      st.started = stream ? (st.started || bytes.length > 0) : false;
      return text;
    }
  }
  Object.defineProperty(TextDecoder.prototype, Symbol.toStringTag, { value: "TextDecoder", configurable: true });
  define("TextEncoder", TextEncoder);
  define("TextDecoder", TextDecoder);

  // ------------------------------------------------- AbortController/Signal
  const kInit = Symbol("init");
  const sigState = new WeakMap();
  class AbortEvent {
    constructor(type, target) {
      this.type = type; this.target = target; this.currentTarget = target;
      this.defaultPrevented = false; this.bubbles = false; this.cancelable = false;
      this.timeStamp = native.now();
    }
    preventDefault() {}
    stopPropagation() {}
  }
  const abortError = (name, message, code) => { const e = new Error(message); e.name = name; e.code = code; return e; };
  const doAbort = (signal, reason) => {
    const st = sigState.get(signal);
    if (st.aborted) return;
    st.aborted = true;
    st.reason = reason === undefined ? abortError("AbortError", "This operation was aborted", 20) : reason;
    const ev = new AbortEvent("abort", signal);
    let firstError;
    let failed = false;
    const run = (fn, self) => {
      try { typeof fn === "function" ? fn.call(self, ev) : fn.handleEvent(ev); }
      catch (e) { if (!failed) { failed = true; firstError = e; } }
    };
    if (typeof signal.onabort === "function") run(signal.onabort, signal);
    for (const l of st.listeners.slice()) {
      if (l.removed) continue;
      if (l.once) removeListener(st, l.fn);
      run(l.fn, signal);
    }
    if (failed) addTimer(() => { throw firstError; }, 0, [], false);
  };
  const removeListener = (st, fn) => {
    const i = st.listeners.findIndex((l) => l.fn === fn);
    if (i >= 0) { st.listeners[i].removed = true; st.listeners.splice(i, 1); }
  };
  class AbortSignal {
    constructor(key) {
      if (key !== kInit) throw new TypeError("Illegal constructor");
      sigState.set(this, { aborted: false, reason: undefined, listeners: [] });
      this.onabort = null;
    }
    get aborted() { return sigState.get(this).aborted; }
    get reason() { return sigState.get(this).reason; }
    throwIfAborted() { const st = sigState.get(this); if (st.aborted) throw st.reason; }
    addEventListener(type, fn, options) {
      if (type !== "abort" || !fn || (typeof fn !== "function" && typeof fn.handleEvent !== "function")) return;
      const st = sigState.get(this);
      if (st.listeners.some((l) => l.fn === fn)) return;
      st.listeners.push({ fn, once: !!(options && typeof options === "object" && options.once), removed: false });
    }
    removeEventListener(type, fn) { if (type === "abort") removeListener(sigState.get(this), fn); }
    dispatchEvent(event) {
      if (event && event.type === "abort") doAbort(this, undefined);
      return true;
    }
    static abort(reason) { const s = new AbortSignal(kInit); doAbort(s, reason); return s; }
    static timeout(ms) {
      const s = new AbortSignal(kInit);
      addTimer(() => doAbort(s, abortError("TimeoutError", "The operation was aborted due to timeout", 23)), ms, [], false);
      return s;
    }
    static any(signals) {
      const s = new AbortSignal(kInit);
      for (const x of signals) {
        if (x.aborted) { doAbort(s, x.reason); return s; }
      }
      for (const x of signals) x.addEventListener("abort", () => doAbort(s, x.reason), { once: true });
      return s;
    }
  }
  Object.defineProperty(AbortSignal.prototype, Symbol.toStringTag, { value: "AbortSignal", configurable: true });
  const ctlSignal = new WeakMap();
  class AbortController {
    constructor() { ctlSignal.set(this, new AbortSignal(kInit)); }
    get signal() { return ctlSignal.get(this); }
    abort(reason) { doAbort(ctlSignal.get(this), reason); }
  }
  Object.defineProperty(AbortController.prototype, Symbol.toStringTag, { value: "AbortController", configurable: true });
  define("AbortSignal", AbortSignal);
  define("AbortController", AbortController);

  // ---------------------------------------------------------- structuredClone
  const cloneError = (msg) => { const e = new Error(msg); e.name = "DataCloneError"; e.code = 25; return e; };
  define("structuredClone", function structuredClone(value) {
    if (arguments.length === 0) throw new TypeError("The \"value\" argument must be specified");
    const memo = new Map();
    const clone = (v) => {
      const t = typeof v;
      if (t === "function") throw cloneError((v.name ? "function " + v.name : "function") + " could not be cloned.");
      if (t === "symbol") throw cloneError(v.toString() + " could not be cloned.");
      if (t !== "object" || v === null) return v;
      if (memo.has(v)) return memo.get(v);
      let out;
      if (Array.isArray(v)) {
        out = new Array(v.length);
        memo.set(v, out);
        for (const k of Object.keys(v)) out[k] = clone(v[k]);
        return out;
      }
      if (v instanceof Date) { out = new Date(v.getTime()); memo.set(v, out); return out; }
      if (v instanceof RegExp) { out = new RegExp(v.source, v.flags); memo.set(v, out); return out; }
      if (v instanceof Map) {
        out = new Map(); memo.set(v, out);
        for (const [k, x] of v) out.set(clone(k), clone(x));
        return out;
      }
      if (v instanceof Set) {
        out = new Set(); memo.set(v, out);
        for (const x of v) out.add(clone(x));
        return out;
      }
      if (v instanceof ArrayBuffer) { out = v.slice(0); memo.set(v, out); return out; }
      if (ArrayBuffer.isView(v)) {
        const buf = clone(v.buffer);
        out = v instanceof DataView
          ? new DataView(buf, v.byteOffset, v.byteLength)
          : new v.constructor(buf, v.byteOffset, v.length);
        memo.set(v, out);
        return out;
      }
      if (v instanceof Error) {
        const names = { EvalError, RangeError, ReferenceError, SyntaxError, TypeError, URIError };
        const C = Object.prototype.hasOwnProperty.call(names, v.name) ? names[v.name] : Error;
        out = new C(v.message);
        memo.set(v, out);
        if (v.stack !== undefined) Object.defineProperty(out, "stack", { value: String(v.stack), writable: true, configurable: true });
        if ("cause" in v) out.cause = clone(v.cause);
        return out;
      }
      if (v instanceof Boolean || v instanceof Number || v instanceof String) { out = Object(v.valueOf()); memo.set(v, out); return out; }
      if (v instanceof Promise || v instanceof WeakMap || v instanceof WeakSet) {
        throw cloneError("#<" + (v.constructor && v.constructor.name || "Object") + "> could not be cloned.");
      }
      out = {};
      memo.set(v, out);
      for (const k of Object.keys(v)) out[k] = clone(v[k]);
      return out;
    };
    return clone(value);
  });

  return { tick };
})

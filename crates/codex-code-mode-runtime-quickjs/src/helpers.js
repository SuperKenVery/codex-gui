(() => {
  const nativeEmitText = globalThis.__codex_emit_text;
  const nativeEmitImage = globalThis.__codex_emit_image;
  const nativeEmitAudio = globalThis.__codex_emit_audio;
  const nativeInvokeTool = globalThis.__codex_invoke_tool;
  const nativeNotify = globalThis.__codex_notify;
  const nativeYield = globalThis.__codex_yield;
  const nativeStore = globalThis.__codex_store;
  const nativeLoad = globalThis.__codex_load;
  const nativeExit = globalThis.__codex_exit;
  const nativeSleep = globalThis.__codex_sleep;
  const metadata = globalThis.__codex_tool_metadata;

  const stringifyOutput = value => {
    if (value === undefined) return "undefined";
    if (value === null) return "null";
    if (typeof value === "string") return value;
    if (typeof value === "bigint") return value.toString();
    if (typeof value === "boolean" || typeof value === "number") return String(value);
    const json = JSON.stringify(value);
    return json === undefined ? String(value) : json;
  };

  const parseDetail = detail => {
    if (detail == null) return null;
    if (typeof detail !== "string" || !["auto", "low", "high", "original"].includes(detail.toLowerCase())) {
      throw new TypeError("image detail must be one of: auto, low, high, original");
    }
    return detail.toLowerCase();
  };

  const imageParts = (value, detailOverride) => {
    let imageUrl;
    let detail = null;
    if (typeof value === "string") {
      imageUrl = value;
    } else if (value && typeof value === "object" && !Array.isArray(value)) {
      if (value.image_url !== undefined) {
        imageUrl = value.image_url;
        detail = parseDetail(value.detail);
      } else if (value.type === "image") {
        const data = value.data;
        if (typeof data !== "string" || data.length === 0) throw new TypeError("image expected MCP image data");
        const mime = value.mimeType || value.mime_type || "application/octet-stream";
        imageUrl = data.toLowerCase().startsWith("data:") ? data : `data:${mime};base64,${data}`;
        const metaDetail = value._meta && value._meta["codex/imageDetail"];
        if (["auto", "low", "high", "original"].includes(metaDetail)) detail = metaDetail;
      }
    }
    if (typeof imageUrl !== "string" || imageUrl.length === 0) {
      throw new TypeError("image expects a non-empty image URL string, an object with image_url and optional detail, or a raw MCP image block");
    }
    if (/^https?:/i.test(imageUrl)) throw new TypeError("Tool call failed: remote image URLs are not supported in tool outputs. Pass a base64 data URI instead");
    if (!/^data:/i.test(imageUrl)) throw new TypeError("Tool call failed: invalid image output. Pass a base64 data URI instead");
    return [imageUrl, parseDetail(detailOverride) || detail || "high"];
  };

  globalThis.text = value => nativeEmitText(stringifyOutput(value));
  globalThis.image = (value, detail) => nativeEmitImage(...imageParts(value, detail));
  globalThis.audio = value => {
    let audioUrl;
    if (typeof value === "string") audioUrl = value;
    else if (value && typeof value === "object" && !Array.isArray(value)) {
      if (value.audio_url !== undefined) audioUrl = value.audio_url;
      else if (value.type === "audio" && typeof value.data === "string" && value.data.length > 0) {
        const mime = value.mimeType || value.mime_type || "application/octet-stream";
        audioUrl = value.data.toLowerCase().startsWith("data:") ? value.data : `data:${mime};base64,${value.data}`;
      }
    }
    if (typeof audioUrl !== "string" || !/^data:/i.test(audioUrl)) {
      throw new TypeError("audio expects a data URL string, an object with audio_url, or a raw MCP audio block");
    }
    nativeEmitAudio(audioUrl);
  };
  globalThis.generatedImage = value => {
    if (!value || typeof value !== "object") throw new TypeError("generatedImage expects an image generation result object");
    globalThis.image(value);
    if (value.output_hint !== undefined) {
      if (typeof value.output_hint !== "string") throw new TypeError("generatedImage output_hint must be a string when provided");
      globalThis.text(value.output_hint);
    }
  };

  globalThis.store = (key, value) => {
    key = String(key);
    const json = JSON.stringify(value);
    if (json === undefined) throw new TypeError(`Unable to store ${JSON.stringify(key)}. Only plain serializable objects can be stored.`);
    nativeStore(key, json);
  };
  globalThis.load = key => {
    const json = nativeLoad(String(key));
    return json == null ? undefined : JSON.parse(json);
  };
  globalThis.notify = value => {
    const text = stringifyOutput(value);
    if (text.trim().length === 0) throw new TypeError("notify expects non-empty text");
    nativeNotify(text);
  };
  globalThis.yield_control = () => nativeYield();
  globalThis.exit = () => {
    nativeExit();
    throw new Error("__codex_code_mode_exit__");
  };

  let nextTimerId = 1;
  const timers = new Map();
  globalThis.setTimeout = (callback, delay = 0) => {
    if (typeof callback !== "function") throw new TypeError("setTimeout expects a function");
    const id = nextTimerId++;
    timers.set(id, true);
    nativeSleep(Number(delay)).then(() => {
      if (timers.delete(id)) callback();
    });
    return id;
  };
  globalThis.clearTimeout = id => { timers.delete(Number(id)); };

  const tools = {};
  for (const tool of metadata) {
    tools[tool.name] = async function(input) {
      let encoded = arguments.length === 0 ? null : JSON.stringify(input);
      if (encoded === undefined) encoded = null;
      const response = JSON.parse(await nativeInvokeTool(tool.index, encoded));
      if (!response.ok) throw new Error(response.error);
      return response.value;
    };
  }
  globalThis.tools = tools;
  globalThis.ALL_TOOLS = metadata.map(({name, description}) => ({name, description}));

  delete globalThis.__codex_emit_text;
  delete globalThis.__codex_emit_image;
  delete globalThis.__codex_emit_audio;
  delete globalThis.__codex_invoke_tool;
  delete globalThis.__codex_notify;
  delete globalThis.__codex_yield;
  delete globalThis.__codex_store;
  delete globalThis.__codex_load;
  delete globalThis.__codex_exit;
  delete globalThis.__codex_sleep;
  delete globalThis.__codex_tool_metadata;
  delete globalThis.console;
  delete globalThis.Atomics;
  delete globalThis.SharedArrayBuffer;
  delete globalThis.WebAssembly;
})();

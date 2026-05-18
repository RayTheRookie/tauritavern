import { invoke } from "@tauri-apps/api/core";

// ── State ──────────────────────────────────

const state = {
  workbenchId: null,
  manifest: { name: "", author: "", version: "1.0.0", description: "", entry_file: "ui/index.html", cover_image: "" },
  preset: {
    system_prompt: "",
    prompt_entries: [],
    model: null, temperature: null, max_tokens: null,
    context_window_size: null, provider: null, provider_url: null,
    chat_format: null, authors_note: null, authors_note_depth: null,
    user_name: "User", char_name: "Character"
  },
  worldInfo: [],
  pipeline: {
    context_strategy: { max_context_tokens: 8000, rag_fetch_count: 3, rag_similarity_threshold: 0.75, history_fetch_limit: 100 },
    regex_mutators: []
  },
  uiFiles: {
    "index.html": '<!doctype html>\n<html lang="en">\n<head>\n  <meta charset="UTF-8" />\n  <title>My Character</title>\n  <link rel="stylesheet" href="style.css" />\n  <script src="../tauri-tavern-sdk.js"><\/script>\n  <script defer src="script.js"><\/script>\n</head>\n<body>\n  <h1>Hello!</h1>\n</body>\n</html>\n',
    "script.js": "const SDK = window.TavernSDK;\n\n// Your chat logic here\n",
    "style.css": "body {\n  font-family: sans-serif;\n  background: #1a1a2e;\n  color: #e0e0e0;\n  margin: 0;\n  padding: 16px;\n}\n"
  },
  agentConfig: { provider: "openai", model: "gpt-4o", provider_url: null, temperature: 0.2, max_tokens: 4096 },
  agentMessages: [],
  editorSettings: { fontSize: 13, tabSize: 2, wordWrap: false, autoPreview: true },
  activeTab: "metadata",
  dirty: new Set(),
  autoSaveTimer: null,
  providers: [],
};

// ── DOM refs ───────────────────────────────

const tabBar = document.getElementById("tab-bar");
const dirtyIndicator = document.getElementById("dirty-indicator");
const workbenchIdDisplay = document.getElementById("workbench-id-display");
const btnSaveAll = document.getElementById("btn-save-all");
const btnExport = document.getElementById("btn-export");
const btnClose = document.getElementById("btn-close-creator");
const toast = document.getElementById("cr-toast");
const agentBubble = document.getElementById("agent-bubble");
const agentPanel = document.getElementById("agent-panel");
const agentClose = document.getElementById("agent-close");
const agentSettings = document.getElementById("agent-settings");
const agentMessages = document.getElementById("agent-messages");
const agentInput = document.getElementById("agent-input");
const agentSend = document.getElementById("agent-send");
const sidebarContext = document.getElementById("sidebar-context");
const previewFrame = document.getElementById("creator-preview-iframe");
const previewStatus = document.getElementById("preview-status");
const previewRefresh = document.getElementById("preview-refresh");
const previewFocusCode = document.getElementById("preview-focus-code");
const leftSidebar = document.getElementById("left-sidebar");
const previewSidebar = document.getElementById("preview-sidebar");
const leftResizer = document.getElementById("left-resizer");
const previewResizer = document.getElementById("preview-resizer");

// ── Init ───────────────────────────────────

document.addEventListener("DOMContentLoaded", async () => {
  const params = new URLSearchParams(window.location.search);
  const wid = params.get("workbench");
  if (!wid) {
    showToast("No workbench ID provided", "error");
    return;
  }
  state.workbenchId = wid;
  workbenchIdDisplay.textContent = wid.slice(0, 8) + "...";

  try {
    state.providers = await invoke("get_providers");
    await loadWorkbench();
    setupTabBar();
    setupWorkbenchLayout();
    await switchTab("metadata");
    schedulePreviewRefresh(0);
  } catch (e) {
    showToast("Failed to load: " + e, "error");
  }

  btnSaveAll.addEventListener("click", () => saveAll());
  btnExport.addEventListener("click", () => handleExport());
  btnClose.addEventListener("click", () => window.close());
  setupCreatorAgent();
  window.addEventListener("creator-edit-regex-replacement", async event => {
    const index = event.detail?.index;
    if (!Number.isInteger(index)) return;
    await switchTab("ui-code");
    window.dispatchEvent(new CustomEvent("creator-select-regex-replacement", { detail: { index } }));
  });

  window.addEventListener("beforeunload", () => {
    if (state.autoSaveTimer) clearTimeout(state.autoSaveTimer);
    saveAll();
  });
});

// ── Tab System ─────────────────────────────

function setupTabBar() {
  tabBar.querySelectorAll(".cr-tab").forEach(btn => {
    btn.addEventListener("click", () => switchTab(btn.dataset.tab));
  });
}

async function switchTab(tabName) {
  // Save current tab
  if (state.activeTab && state.activeTab !== tabName) {
    await flushSave();
  }

  state.activeTab = tabName;

  // Update tab buttons
  tabBar.querySelectorAll(".cr-tab").forEach(b => {
    b.classList.toggle("active", b.dataset.tab === tabName);
  });

  // Show panel
  document.querySelectorAll(".cr-panel").forEach(p => p.classList.add("hidden"));
  const panel = document.getElementById("panel-" + tabName);
  if (panel) panel.classList.remove("hidden");

  // Lazy-load tab module
  switch (tabName) {
    case "metadata": await loadTab("metadata-tab.js", panel, "renderMetadata"); break;
    case "preset": await loadTab("preset-tab.js", panel, "renderPreset"); break;
    case "world-info": await loadTab("world-info-tab.js", panel, "renderWorldInfo"); break;
    case "pipeline": await loadTab("pipeline-tab.js", panel, "renderPipeline"); break;
    case "ui-code": await loadTab("ui-code-tab.js", panel, "renderUICode"); break;
    case "test-chat": await loadTab("test-chat-tab.js", panel, "renderTestChat"); break;
  }
  updateSidebarContext();
  schedulePreviewRefresh(0);
}

const loadedTabs = {};
async function loadTab(filename, panel, renderFn) {
  if (!loadedTabs[filename]) {
    loadedTabs[filename] = await import("./tabs/" + filename);
  }
  loadedTabs[filename][renderFn](panel, state);
}

// ── Workbench I/O ──────────────────────────

async function loadWorkbench() {
  const data = await invoke("get_workbench", { workbenchId: state.workbenchId });
  if (data.manifest) state.manifest = data.manifest;
  if (data.preset) state.preset = data.preset;
  if (data.world_info) state.worldInfo = data.world_info;
  if (data.pipeline) state.pipeline = data.pipeline;
  if (data.agent_config) state.agentConfig = data.agent_config;
  if (data.ui_files) {
    for (const [name, content] of Object.entries(data.ui_files)) {
      state.uiFiles[name] = content;
    }
  }
}

function applyWorkbenchBundle(data) {
  if (data.manifest) state.manifest = data.manifest;
  if (data.preset) state.preset = data.preset;
  if (data.world_info) state.worldInfo = data.world_info;
  if (data.pipeline) state.pipeline = data.pipeline;
  if (data.agent_config) state.agentConfig = data.agent_config;
  if (data.ui_files) {
    state.uiFiles = { ...state.uiFiles, ...data.ui_files };
  }
}

function markDirty(key) {
  state.dirty.add(key);
  dirtyIndicator.classList.remove("hidden");
}

function markClean(key) {
  state.dirty.delete(key);
  if (state.dirty.size === 0) {
    dirtyIndicator.classList.add("hidden");
  }
}

async function flushSave() {
  if (state.autoSaveTimer) {
    clearTimeout(state.autoSaveTimer);
    state.autoSaveTimer = null;
  }
  // Save all dirty items
  const promises = [];
  for (const key of state.dirty) {
    switch (key) {
      case "manifest":
        promises.push(invoke("save_workbench_manifest", { workbenchId: state.workbenchId, manifest: state.manifest }));
        break;
      case "preset":
        promises.push(invoke("save_workbench_preset", { workbenchId: state.workbenchId, preset: state.preset }));
        break;
      case "worldInfo":
        promises.push(invoke("save_workbench_world_info", { workbenchId: state.workbenchId, entries: state.worldInfo }));
        break;
      case "pipeline":
        promises.push(invoke("save_workbench_pipeline", { workbenchId: state.workbenchId, pipeline: state.pipeline }));
        break;
      case "uiFiles":
        for (const [name, content] of Object.entries(state.uiFiles)) {
          promises.push(invoke("save_workbench_ui_file", { workbenchId: state.workbenchId, filename: name, content }));
        }
        break;
    }
  }
  if (promises.length > 0) {
    try {
      await Promise.all(promises);
      state.dirty.clear();
      dirtyIndicator.classList.add("hidden");
    } catch (e) {
      showToast("Save failed: " + e, "error");
    }
  }
}

function scheduleAutoSave(key) {
  markDirty(key);
  if (state.autoSaveTimer) clearTimeout(state.autoSaveTimer);
  state.autoSaveTimer = setTimeout(() => flushSave(), 2000);
}

async function saveAll() {
  // Mark everything dirty
  state.dirty.add("manifest");
  state.dirty.add("preset");
  state.dirty.add("worldInfo");
  state.dirty.add("pipeline");
  state.dirty.add("uiFiles");
  await flushSave();
  showToast("All saved", "success");
}

async function refreshActiveTab() {
  const panel = document.getElementById("panel-" + state.activeTab);
  switch (state.activeTab) {
    case "metadata": await loadTab("metadata-tab.js", panel, "renderMetadata"); break;
    case "preset": await loadTab("preset-tab.js", panel, "renderPreset"); break;
    case "world-info": await loadTab("world-info-tab.js", panel, "renderWorldInfo"); break;
    case "pipeline": await loadTab("pipeline-tab.js", panel, "renderPipeline"); break;
    case "ui-code": await loadTab("ui-code-tab.js", panel, "renderUICode"); break;
    case "test-chat": await loadTab("test-chat-tab.js", panel, "renderTestChat"); break;
  }
}

function setupCreatorAgent() {
  if (!agentBubble || !agentPanel) return;
  agentBubble.addEventListener("click", () => {
    agentPanel.classList.toggle("hidden");
    agentInput?.focus();
  });
  agentClose?.addEventListener("click", () => agentPanel.classList.add("hidden"));
  agentSettings?.addEventListener("click", openAgentSettings);
  agentSend?.addEventListener("click", sendAgentMessage);
  agentInput?.addEventListener("keydown", e => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      sendAgentMessage();
    }
  });
}

async function sendAgentMessage() {
  const message = agentInput.value.trim();
  if (!message || agentSend.disabled) return;
  await flushSave();
  agentInput.value = "";
  appendAgentMessage("user", message);
  agentSend.disabled = true;
  agentInput.disabled = true;

  const pending = appendAgentMessage("assistant", "Working...");
  try {
    const response = await invoke("creator_agent_chat", {
      request: {
        workbench_id: state.workbenchId,
        message,
        history: state.agentMessages.slice(-8),
      },
    });
    pending.textContent = response.reply || "Done.";
    state.agentMessages.push({ role: "user", content: message });
    state.agentMessages.push({ role: "assistant", content: response.reply || "Done." });
    if (response.workbench) {
      applyWorkbenchBundle(response.workbench);
      state.dirty.clear();
      dirtyIndicator.classList.add("hidden");
      await refreshActiveTab();
      schedulePreviewRefresh(0);
    }
    if (response.applied?.length) {
      showToast("Agent updated: " + response.applied.join(", "), "success");
    }
  } catch (e) {
    pending.textContent = "Error: " + e;
    pending.classList.add("error");
  } finally {
    agentSend.disabled = false;
    agentInput.disabled = false;
    agentInput.focus();
    agentMessages.scrollTop = agentMessages.scrollHeight;
  }
}

function setupWorkbenchLayout() {
  previewRefresh?.addEventListener("click", () => schedulePreviewRefresh(0));
  previewFocusCode?.addEventListener("click", () => switchTab("ui-code"));
  setupHorizontalResize(leftResizer, leftSidebar, "leftSidebarWidth", {
    min: 180,
    max: 420,
    defaultWidth: 240,
    side: "left",
  });
  setupHorizontalResize(previewResizer, previewSidebar, "previewSidebarWidth", {
    min: 260,
    max: 720,
    defaultWidth: 420,
    side: "right",
  });
}

function setupHorizontalResize(handle, panel, storageKey, options) {
  if (!handle || !panel) return;
  const saved = Number(localStorage.getItem(storageKey));
  const initial = Number.isFinite(saved) && saved > 0 ? saved : options.defaultWidth;
  panel.style.width = `${initial}px`;

  handle.addEventListener("pointerdown", e => {
    e.preventDefault();
    handle.setPointerCapture(e.pointerId);
    const startX = e.clientX;
    const startWidth = panel.getBoundingClientRect().width;
    const move = event => {
      const delta = event.clientX - startX;
      const raw = options.side === "right" ? startWidth - delta : startWidth + delta;
      const width = Math.max(options.min, Math.min(options.max, raw));
      panel.style.width = `${width}px`;
      localStorage.setItem(storageKey, String(width));
    };
    const up = event => {
      handle.releasePointerCapture(event.pointerId);
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  });
}

function updateSidebarContext() {
  if (!sidebarContext) return;
  const files = Object.keys(state.uiFiles);
  const displayRegex = (state.pipeline.regex_mutators || [])
    .map((item, index) => ({ item, index }))
    .filter(({ item }) => ["display", "frontend", "message_display"].includes(item.target || ""));
  const tabLabels = {
    metadata: "Metadata",
    preset: "Prompt Preset",
    "world-info": "World Info",
    pipeline: "Pipeline",
    "ui-code": "UI Code",
    "test-chat": "Test Chat",
  };
  sidebarContext.innerHTML = `
    <div class="sidebar-section">
      <div class="sidebar-section-title">Active</div>
      <div class="sidebar-row strong">${escapeHtml(tabLabels[state.activeTab] || state.activeTab)}</div>
    </div>
    <div class="sidebar-section">
      <div class="sidebar-section-title">UI Files</div>
      ${files.map(file => `<button class="sidebar-row sidebar-file" data-file="${escapeHtml(file)}">${escapeHtml(file)}</button>`).join("")}
    </div>
    <div class="sidebar-section">
      <div class="sidebar-section-title">Regex</div>
      <button class="sidebar-row sidebar-regex-manage">Manage Regex</button>
      ${displayRegex.length
        ? displayRegex.map(({ item, index }) => `<button class="sidebar-row sidebar-regex" data-index="${index}">${escapeHtml(item.id || "Regex #" + (index + 1))}</button>`).join("")
        : `<button class="sidebar-row sidebar-regex-new">+ Display Regex</button>`}
    </div>
    <div class="sidebar-section">
      <div class="sidebar-section-title">Card</div>
      <div class="sidebar-row">${escapeHtml(state.manifest.name || "Untitled")}</div>
      <div class="sidebar-muted">${escapeHtml(state.manifest.author || "Anonymous")}</div>
    </div>
  `;
  sidebarContext.querySelectorAll(".sidebar-file").forEach(btn => {
    btn.addEventListener("click", async () => {
      await switchTab("ui-code");
      window.dispatchEvent(new CustomEvent("creator-select-ui-file", { detail: { file: btn.dataset.file } }));
    });
  });
  sidebarContext.querySelectorAll(".sidebar-regex").forEach(btn => {
    btn.addEventListener("click", () => {
      window.dispatchEvent(new CustomEvent("creator-edit-regex-replacement", { detail: { index: parseInt(btn.dataset.index) } }));
    });
  });
  sidebarContext.querySelector(".sidebar-regex-manage")?.addEventListener("click", () => switchTab("pipeline"));
  sidebarContext.querySelector(".sidebar-regex-new")?.addEventListener("click", async () => {
    state.pipeline.regex_mutators.push({
      id: `display_regex_${state.pipeline.regex_mutators.length + 1}`,
      enabled: true,
      target: "display",
      depth_range: [],
      pattern: "<Gui>([\\s\\S]*?)</Gui>",
      replacement: "$1",
      flags: "gs",
      sample: "<Gui><div>Hello</div></Gui>",
      description: "Frontend display replacement",
    });
    scheduleAutoSave("pipeline");
    const index = state.pipeline.regex_mutators.length - 1;
    updateSidebarContext();
    window.dispatchEvent(new CustomEvent("creator-edit-regex-replacement", { detail: { index } }));
  });
}

let previewTimer;
function schedulePreviewRefresh(delay = 120) {
  clearTimeout(previewTimer);
  previewTimer = setTimeout(refreshCreatorPreview, delay);
}

function refreshCreatorPreview() {
  if (!previewFrame || !state.workbenchId) return;
  try {
    previewFrame.srcdoc = buildPreviewDocument();
    if (previewStatus) previewStatus.textContent = "Updated " + new Date().toLocaleTimeString();
  } catch (e) {
    if (previewStatus) previewStatus.textContent = "Preview error";
    showToast("Preview failed: " + e, "error");
  }
}

function buildPreviewDocument() {
  const baseHref = `tavern://localhost/workbench/${state.workbenchId}/ui/`;
  const sdkSrc = "tavern://localhost/sdk/tauri-tavern-sdk.js";
  let html = state.uiFiles["index.html"] || "<!doctype html><html><head></head><body></body></html>";
  const css = state.uiFiles["style.css"] || "";
  const js = state.uiFiles["script.js"] || "";

  html = html
    .replace(/<link\b[^>]*href=["'](?:\.\/)?style\.css["'][^>]*>/gi, "")
    .replace(/<script\b[^>]*src=["'](?:\.\/)?script\.js["'][^>]*>\s*<\/script>/gi, "")
    .replace(/<script\b[^>]*src=["']\.\.\/tauri-tavern-sdk\.js["'][^>]*>\s*<\/script>/gi, "");

  const headInject = `<base href="${baseHref}"><script src="${sdkSrc}"><\/script><style data-live-style>${css}</style>`;
  const bodyInject = `<script data-live-script>${js.replace(/<\/script/gi, "<\\/script")}<\/script>`;

  if (/<head[^>]*>/i.test(html)) {
    html = html.replace(/<head[^>]*>/i, match => `${match}${headInject}`);
  } else {
    html = /<html[^>]*>/i.test(html)
      ? html.replace(/<html[^>]*>/i, match => `${match}<head>${headInject}</head>`)
      : `<head>${headInject}</head>${html}`;
  }

  if (/<\/body>/i.test(html)) {
    html = html.replace(/<\/body>/i, `${bodyInject}</body>`);
  } else {
    html += bodyInject;
  }
  return html;
}

function appendAgentMessage(role, text) {
  const el = document.createElement("div");
  el.className = "agent-msg " + role;
  el.textContent = text;
  agentMessages.appendChild(el);
  agentMessages.scrollTop = agentMessages.scrollHeight;
  return el;
}

function openAgentSettings() {
  const providers = state.providers || [];
  const cfg = state.agentConfig || {};
  const container = document.createElement("div");
  container.innerHTML = `
    <div class="cr-modal-overlay" id="agent-settings-modal">
      <div class="cr-modal agent-settings-modal">
        <div class="cr-modal-header">
          <h3>Creator Agent Settings</h3>
          <button class="btn btn-ghost btn-sm" id="agent-settings-close">&times;</button>
        </div>
        <div class="cr-modal-body">
          <div class="cr-form-row">
            <div class="cr-form-group">
              <label><span>Provider</span>
              <select id="agent-provider">
                ${providers.map(pr => `<option value="${escapeHtml(pr.id)}" ${cfg.provider === pr.id ? "selected" : ""}>${escapeHtml(pr.display_name)}</option>`).join("")}
              </select></label>
            </div>
            <div class="cr-form-group">
              <label><span>Model</span><input id="agent-model" type="text" value="${escapeHtml(cfg.model || "")}" /></label>
            </div>
          </div>
          <div class="cr-form-group">
            <label><span>API Key</span><input id="agent-api-key" type="password" placeholder="Stored in keyring for this provider" /></label>
          </div>
          <div class="cr-form-group">
            <label><span>Provider URL</span><input id="agent-provider-url" type="text" value="${escapeHtml(cfg.provider_url || "")}" placeholder="Optional custom endpoint" /></label>
          </div>
          <div class="cr-form-row">
            <div class="cr-form-group">
              <label><span>Temperature</span><input id="agent-temp" type="number" min="0" max="2" step="0.1" value="${cfg.temperature ?? 0.2}" /></label>
            </div>
            <div class="cr-form-group">
              <label><span>Max Tokens</span><input id="agent-max-tokens" type="number" min="512" step="256" value="${cfg.max_tokens ?? 4096}" /></label>
            </div>
          </div>
        </div>
        <div class="cr-modal-footer">
          <button class="btn btn-ghost btn-sm" id="agent-fetch-models">Fetch Models</button>
          <button class="btn btn-primary btn-sm" id="agent-save-settings">Save Settings</button>
        </div>
      </div>
    </div>
  `;
  document.body.appendChild(container.firstElementChild);

  const close = () => document.getElementById("agent-settings-modal")?.remove();
  document.getElementById("agent-settings-close").addEventListener("click", close);
  document.getElementById("agent-save-settings").addEventListener("click", async () => {
    try {
      await saveAgentSettingsFromModal();
      close();
      showToast("Agent settings saved", "success");
    } catch (e) {
      showToast("Agent settings failed: " + e, "error");
    }
  });
  document.getElementById("agent-fetch-models").addEventListener("click", fetchAgentModels);
}

async function saveAgentSettingsFromModal() {
  const provider = document.getElementById("agent-provider").value;
  const model = document.getElementById("agent-model").value.trim();
  const providerUrl = document.getElementById("agent-provider-url").value.trim();
  const apiKey = document.getElementById("agent-api-key").value.trim();
  if (!provider || !model) throw new Error("Provider and model are required");
  if (apiKey) await invoke("set_api_key", { provider, key: apiKey });

  const config = {
    provider,
    model,
    provider_url: providerUrl || null,
    temperature: parseFloat(document.getElementById("agent-temp").value),
    max_tokens: parseInt(document.getElementById("agent-max-tokens").value),
  };
  await invoke("save_creator_agent_config", { workbenchId: state.workbenchId, config });
  state.agentConfig = config;
}

async function fetchAgentModels() {
  try {
    const provider = document.getElementById("agent-provider").value;
    const providerUrl = document.getElementById("agent-provider-url").value.trim();
    const apiKeyInput = document.getElementById("agent-api-key").value.trim();
    const apiKey = apiKeyInput || await invoke("get_raw_api_key", { provider });
    if (!apiKey) throw new Error("Enter an API key first");
    const providerInfo = state.providers.find(p => p.id === provider);
    const apiUrl = providerUrl || providerInfo?.default_url || "";
    const models = await invoke("fetch_models", { provider, apiKey, apiUrl });
    const modelInput = document.getElementById("agent-model");
    modelInput.setAttribute("list", "agent-model-list");
    let list = document.getElementById("agent-model-list");
    if (!list) {
      list = document.createElement("datalist");
      list.id = "agent-model-list";
      document.body.appendChild(list);
    }
    list.innerHTML = models.map(m => `<option value="${escapeHtml(m)}"></option>`).join("");
    if (!modelInput.value && models[0]) modelInput.value = models[0];
    showToast(`Loaded ${models.length} models`, "success");
  } catch (e) {
    showToast("Fetch models failed: " + e, "error");
  }
}

async function handleExport() {
  await flushSave();
  try {
    const path = await invoke("export_workbench", { workbenchId: state.workbenchId });
    showToast("Exported to " + path, "success");
  } catch (e) {
    showToast("Export failed: " + e, "error");
  }
}

// ── Helpers ────────────────────────────────

let toastTimeout;
function showToast(message, type) {
  toast.textContent = message;
  toast.className = "cr-toast " + type;
  clearTimeout(toastTimeout);
  toastTimeout = setTimeout(() => toast.classList.add("hidden"), 3000);
}

function escapeHtml(text) {
  const div = document.createElement("div");
  div.textContent = text;
  return div.innerHTML;
}

// ── Exports for tabs ───────────────────────

export { state, scheduleAutoSave, markDirty, showToast, escapeHtml, invoke, schedulePreviewRefresh };

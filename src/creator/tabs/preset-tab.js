import { state, scheduleAutoSave, showToast } from "../creator.js";

let editingIndex = -1;

const PINNED_IDS = new Set(["main_prompt", "auxiliary_prompt", "authors_note", "post_history_instructions"]);

export function renderPreset(panel) {
  normalizePreset();
  const p = state.preset;
  const providers = state.providers || [];
  panel.innerHTML = `
    <div style="display:flex;align-items:center;justify-content:space-between;margin-bottom:12px;">
      <h2 class="cr-section-title" style="margin:0;border:none;padding:0;">Prompt Entries</h2>
      <button id="preset-add-entry" class="btn btn-primary btn-sm">+ Add Entry</button>
    </div>
    <div id="preset-entry-list" class="cr-entry-list"></div>
    <div id="preset-modal-container"></div>

    <h3 class="cr-section-title" style="margin-top:20px;">Model Settings</h3>
    <div class="cr-form-row">
      <div class="cr-form-group">
        <label><span>Provider</span>
        <select id="preset-provider">
          <option value="">(Use profile default)</option>
          ${providers.map(pr => `<option value="${esc(pr.id)}" ${p.provider === pr.id ? "selected" : ""}>${esc(pr.display_name)}</option>`).join("")}
        </select></label>
      </div>
      <div class="cr-form-group">
        <label><span>Model</span><input type="text" id="preset-model" value="${esc(p.model || "")}" placeholder="e.g. gpt-4o" /></label>
      </div>
    </div>
    <div class="cr-form-row">
      <div class="cr-form-group">
        <label><span>Provider URL (optional)</span><input type="text" id="preset-url" value="${esc(p.provider_url || "")}" placeholder="Custom API endpoint" /></label>
      </div>
      <div class="cr-form-group">
        <label><span>Chat Format</span>
        <select id="preset-format">
          <option value="" ${!p.chat_format ? "selected" : ""}>chatml (default)</option>
          <option value="chatml" ${p.chat_format === "chatml" ? "selected" : ""}>chatml</option>
          <option value="alpaca" ${p.chat_format === "alpaca" ? "selected" : ""}>alpaca</option>
        </select></label>
      </div>
    </div>

    <h3 class="cr-section-title" style="margin-top:18px;">Parameters</h3>
    <div class="cr-form-row-3">
      <div class="cr-form-group">
        <label><span>Temperature</span><input type="number" id="preset-temp" value="${p.temperature ?? 0.7}" step="0.1" min="0" max="2" /></label>
      </div>
      <div class="cr-form-group">
        <label><span>Max Tokens</span><input type="number" id="preset-max-tokens" value="${p.max_tokens ?? 4096}" step="256" min="1" /></label>
      </div>
      <div class="cr-form-group">
        <label><span>Context Window</span><input type="number" id="preset-ctx" value="${p.context_window_size ?? 8000}" step="1024" min="512" /></label>
      </div>
    </div>

    <h3 class="cr-section-title" style="margin-top:18px;">Name Macros</h3>
    <div class="cr-form-row">
      <div class="cr-form-group">
        <label><span>{{user}} Name</span><input type="text" id="preset-user-name" value="${esc(p.user_name || "User")}" /></label>
      </div>
      <div class="cr-form-group">
        <label><span>{{char}} Name</span><input type="text" id="preset-char-name" value="${esc(p.char_name || "Character")}" /></label>
      </div>
    </div>
  `;

  document.getElementById("preset-add-entry").addEventListener("click", () => openEntryEditor(-1));
  bindInput("preset-model", "model");
  bindInput("preset-url", "provider_url");
  bindInput("preset-user-name", "user_name");
  bindInput("preset-char-name", "char_name");
  bindNumber("preset-temp", "temperature", parseFloat);
  bindNumber("preset-max-tokens", "max_tokens", parseInt);
  bindNumber("preset-ctx", "context_window_size", parseInt);

  const selProvider = document.getElementById("preset-provider");
  if (selProvider) selProvider.addEventListener("change", () => { state.preset.provider = selProvider.value || null; scheduleAutoSave("preset"); });
  const selFormat = document.getElementById("preset-format");
  if (selFormat) selFormat.addEventListener("change", () => { state.preset.chat_format = selFormat.value || null; scheduleAutoSave("preset"); });
  renderEntryList();
}

function renderEntryList() {
  const list = document.getElementById("preset-entry-list");
  if (!list) return;
  list.innerHTML = state.preset.prompt_entries.map((entry, i) => `
    <div class="cr-entry-card ${entry.enabled === false ? "disabled" : ""}" data-index="${i}">
      <div class="cr-entry-card-header">
        <span class="cr-entry-card-title">${esc(entry.name || entry.id)}${entry.pinned ? " · pinned" : ""}</span>
        <div class="cr-entry-card-actions">
          <button class="btn btn-ghost btn-sm preset-up" data-index="${i}" ${i === 0 ? "disabled" : ""}>Up</button>
          <button class="btn btn-ghost btn-sm preset-down" data-index="${i}" ${i === state.preset.prompt_entries.length - 1 ? "disabled" : ""}>Down</button>
          <button class="btn btn-ghost btn-sm preset-toggle" data-index="${i}">${entry.enabled === false ? "Enable" : "Disable"}</button>
          <button class="btn btn-ghost btn-sm preset-edit" data-index="${i}">Edit</button>
          <button class="btn btn-ghost btn-sm preset-delete" data-index="${i}" style="color:var(--danger);" ${entry.pinned ? "disabled" : ""}>Del</button>
        </div>
      </div>
      <div class="cr-entry-card-keys">
        ${esc(entry.position || "relative")} | role: ${esc(entry.role || "system")} | depth: ${entry.depth ?? "none"} | order: ${entry.order ?? i}
      </div>
      <div class="cr-entry-card-content">${esc((entry.content || "").slice(0, 140)) || "(empty)"}</div>
      <div style="font-size:0.7rem;color:var(--text-muted);margin-top:4px;">Triggers: ${(entry.triggers || []).join(", ") || "(always)"}</div>
    </div>
  `).join("");

  list.querySelectorAll(".preset-edit").forEach(b => b.addEventListener("click", e => { e.stopPropagation(); openEntryEditor(parseInt(b.dataset.index)); }));
  list.querySelectorAll(".preset-toggle").forEach(b => b.addEventListener("click", e => { e.stopPropagation(); toggleEntry(parseInt(b.dataset.index)); }));
  list.querySelectorAll(".preset-delete").forEach(b => b.addEventListener("click", e => { e.stopPropagation(); deleteEntry(parseInt(b.dataset.index)); }));
  list.querySelectorAll(".preset-up").forEach(b => b.addEventListener("click", e => { e.stopPropagation(); moveEntry(parseInt(b.dataset.index), -1); }));
  list.querySelectorAll(".preset-down").forEach(b => b.addEventListener("click", e => { e.stopPropagation(); moveEntry(parseInt(b.dataset.index), 1); }));
  list.querySelectorAll(".cr-entry-card").forEach(card => card.addEventListener("click", () => openEntryEditor(parseInt(card.dataset.index))));
}

function openEntryEditor(index) {
  editingIndex = index;
  const entry = index >= 0 ? state.preset.prompt_entries[index] : newPromptEntry();
  const container = document.getElementById("preset-modal-container");
  container.innerHTML = `
    <div class="cr-modal-overlay" id="preset-modal-overlay">
      <div class="cr-modal cr-modal-wide">
        <div class="cr-modal-header">
          <h3>${index >= 0 ? "Edit Prompt Entry" : "New Prompt Entry"}</h3>
          <button class="btn btn-ghost btn-sm" id="preset-modal-close">&times;</button>
        </div>
        <div class="cr-modal-body">
          <div class="cr-form-row">
            <div class="cr-form-group">
              <label><span>Name</span><input type="text" id="pe-name" value="${esc(entry.name || "")}" /></label>
            </div>
            <div class="cr-form-group">
              <label><span>ID</span><input type="text" id="pe-id" value="${esc(entry.id || "")}" ${entry.pinned ? "readonly" : ""} /></label>
            </div>
          </div>
          <div class="cr-form-row-3">
            <div class="cr-form-group">
              <label><span>Position</span>
              <select id="pe-position">
                <option value="relative" ${entry.position !== "in_chat" ? "selected" : ""}>relative</option>
                <option value="in_chat" ${entry.position === "in_chat" ? "selected" : ""}>in_chat</option>
              </select></label>
            </div>
            <div class="cr-form-group">
              <label><span>Role</span>
              <select id="pe-role">
                <option value="system" ${entry.role === "system" ? "selected" : ""}>system</option>
                <option value="user" ${entry.role === "user" ? "selected" : ""}>user</option>
                <option value="assistant" ${entry.role === "assistant" ? "selected" : ""}>assistant</option>
              </select></label>
            </div>
            <div class="cr-form-group">
              <label><span>Order</span><input type="number" id="pe-order" value="${entry.order ?? index}" /></label>
            </div>
          </div>
          <div class="cr-form-row">
            <div class="cr-form-group">
              <label><span>Depth (in_chat only)</span><input type="number" id="pe-depth" value="${entry.depth ?? ""}" min="0" /></label>
            </div>
            <div class="cr-form-group">
              <label><span>Triggers</span><input type="text" id="pe-triggers" value="${esc((entry.triggers || []).join(", "))}" placeholder="empty = always active" /></label>
            </div>
          </div>
          <div class="cr-form-group">
            <label style="flex-direction:row;align-items:center;gap:8px;">
              <input type="checkbox" id="pe-enabled" ${entry.enabled !== false ? "checked" : ""} />
              <span>Enabled</span>
            </label>
          </div>
          <div class="cr-form-group">
            <label><span>Content</span><textarea id="pe-content" rows="10">${esc(entry.content || "")}</textarea></label>
            <div class="cr-form-hint">Use {{user}}, {{char}}, and {{input}} macros.</div>
          </div>
        </div>
        <div class="cr-modal-footer">
          <button class="btn btn-ghost btn-sm" id="preset-modal-cancel">Cancel</button>
          <button class="btn btn-primary btn-sm" id="preset-modal-save">Save Entry</button>
        </div>
      </div>
    </div>
  `;
  document.getElementById("preset-modal-overlay").addEventListener("click", e => { if (e.target.id === "preset-modal-overlay") closeModal(); });
  document.getElementById("preset-modal-close").addEventListener("click", closeModal);
  document.getElementById("preset-modal-cancel").addEventListener("click", closeModal);
  document.getElementById("preset-modal-save").addEventListener("click", saveEntry);
}

function saveEntry() {
  const id = document.getElementById("pe-id").value.trim() || makeId(document.getElementById("pe-name").value);
  const existing = editingIndex >= 0 ? state.preset.prompt_entries[editingIndex] : null;
  const entry = {
    id,
    name: document.getElementById("pe-name").value.trim() || id,
    enabled: document.getElementById("pe-enabled").checked,
    role: document.getElementById("pe-role").value,
    content: document.getElementById("pe-content").value,
    position: document.getElementById("pe-position").value,
    depth: parseOptionalInt(document.getElementById("pe-depth").value),
    order: parseInt(document.getElementById("pe-order").value || "0"),
    triggers: parseKeys(document.getElementById("pe-triggers").value),
    pinned: existing?.pinned || PINNED_IDS.has(id),
  };
  if (!entry.name) {
    showToast("Name is required", "error");
    return;
  }
  if (editingIndex >= 0) {
    state.preset.prompt_entries[editingIndex] = entry;
  } else {
    state.preset.prompt_entries.push(entry);
  }
  syncLegacyFields();
  scheduleAutoSave("preset");
  closeModal();
  renderEntryList();
}

function closeModal() {
  document.getElementById("preset-modal-container").innerHTML = "";
  editingIndex = -1;
}

function toggleEntry(index) {
  state.preset.prompt_entries[index].enabled = state.preset.prompt_entries[index].enabled === false;
  syncLegacyFields();
  scheduleAutoSave("preset");
  renderEntryList();
}

function deleteEntry(index) {
  const entry = state.preset.prompt_entries[index];
  if (entry.pinned) return;
  if (!confirm("Delete this prompt entry?")) return;
  state.preset.prompt_entries.splice(index, 1);
  syncOrders();
  syncLegacyFields();
  scheduleAutoSave("preset");
  renderEntryList();
}

function moveEntry(index, delta) {
  const target = index + delta;
  if (target < 0 || target >= state.preset.prompt_entries.length) return;
  const [entry] = state.preset.prompt_entries.splice(index, 1);
  state.preset.prompt_entries.splice(target, 0, entry);
  syncOrders();
  syncLegacyFields();
  scheduleAutoSave("preset");
  renderEntryList();
}

function bindInput(id, key) {
  const el = document.getElementById(id);
  if (!el) return;
  el.addEventListener("input", () => { state.preset[key] = el.value || null; scheduleAutoSave("preset"); });
}

function bindNumber(id, key, parser) {
  const el = document.getElementById(id);
  if (!el) return;
  el.addEventListener("input", () => { state.preset[key] = el.value.trim() ? parser(el.value) : null; scheduleAutoSave("preset"); });
}

function normalizePreset() {
  const p = state.preset;
  if (!Array.isArray(p.prompt_entries) || p.prompt_entries.length === 0) {
    p.prompt_entries = defaultPromptEntries(p);
  }
  p.prompt_entries.forEach((entry, i) => {
    entry.id ||= makeId(entry.name || "prompt");
    entry.name ||= entry.id;
    entry.enabled = entry.enabled !== false;
    entry.role ||= "system";
    entry.content ||= "";
    entry.position = entry.position === "in_chat" ? "in_chat" : "relative";
    entry.order = Number.isFinite(entry.order) ? entry.order : i * 100;
    entry.triggers ||= [];
    entry.pinned = entry.pinned || PINNED_IDS.has(entry.id);
  });
  syncLegacyFields();
}

function defaultPromptEntries(p) {
  return [
    { id: "main_prompt", name: "Main Prompt", enabled: !!(p.system_prompt || "").trim(), role: "system", content: p.system_prompt || "You are a helpful assistant.", position: "relative", depth: null, order: 0, triggers: [], pinned: true },
    { id: "auxiliary_prompt", name: "Auxiliary Prompt", enabled: false, role: "system", content: "", position: "relative", depth: null, order: 100, triggers: [], pinned: true },
    { id: "authors_note", name: "Author's Note", enabled: !!(p.authors_note || "").trim(), role: "system", content: p.authors_note || "", position: "in_chat", depth: p.authors_note_depth ?? 2, order: 200, triggers: [], pinned: true },
    { id: "post_history_instructions", name: "Post-History Instructions", enabled: false, role: "system", content: "", position: "in_chat", depth: 0, order: 300, triggers: [], pinned: true },
  ];
}

function syncLegacyFields() {
  const main = state.preset.prompt_entries.find(e => e.id === "main_prompt");
  const note = state.preset.prompt_entries.find(e => e.id === "authors_note");
  if (main) state.preset.system_prompt = main.content || "";
  if (note) {
    state.preset.authors_note = note.content || null;
    state.preset.authors_note_depth = note.depth ?? null;
  }
}

function syncOrders() {
  state.preset.prompt_entries.forEach((entry, i) => { entry.order = i * 100; });
}

function newPromptEntry() {
  const index = state.preset.prompt_entries.length + 1;
  return { id: `custom_prompt_${Date.now()}`, name: `Custom Prompt ${index}`, enabled: true, role: "system", content: "", position: "relative", depth: null, order: index * 100, triggers: [], pinned: false };
}

function makeId(input) {
  return (input || "prompt").toLowerCase().replace(/[^a-z0-9]+/g, "_").replace(/^_+|_+$/g, "") || "prompt";
}

function parseKeys(input) {
  return input.split(",").map(s => s.trim()).filter(Boolean);
}

function parseOptionalInt(s) {
  return s.trim() ? parseInt(s) : null;
}

function esc(s) { return (s || "").replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;"); }

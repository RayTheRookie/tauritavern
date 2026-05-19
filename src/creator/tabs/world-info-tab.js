import { state, scheduleAutoSave, showToast } from "../creator.js";

let editingIndex = -1;

export function renderWorldInfo(panel) {
  panel.innerHTML = `
    <div style="display:flex;align-items:center;justify-content:space-between;margin-bottom:12px;">
      <h2 class="cr-section-title" style="margin:0;border:none;padding:0;">World Info Entries</h2>
      <button id="wi-add-entry" class="btn btn-primary btn-sm">+ Add Entry</button>
    </div>
    <div id="wi-entry-list" class="cr-entry-list"></div>
    <div id="wi-modal-container"></div>
  `;

  document.getElementById("wi-add-entry").addEventListener("click", () => openEntryEditor(-1));
  renderEntryList();
}

function renderEntryList() {
  const list = document.getElementById("wi-entry-list");
  if (!list) return;

  if (state.worldInfo.length === 0) {
    list.innerHTML = '<div class="cr-form-hint" style="text-align:center;padding:24px;">No entries yet. Click "+ Add Entry" to create one.</div>';
    return;
  }

  list.innerHTML = state.worldInfo.map((entry, i) => `
    <div class="cr-entry-card ${isEntryDisabled(entry) ? 'disabled' : ''}" data-index="${i}">
      <div class="cr-entry-card-header">
        <span class="cr-entry-card-title">${esc(entry.id || "Entry #" + (i + 1))}</span>
        <div class="cr-entry-card-actions">
          <button class="btn btn-ghost btn-sm wi-edit" data-index="${i}">Edit</button>
          <button class="btn btn-ghost btn-sm wi-toggle" data-index="${i}">${isEntryDisabled(entry) ? 'Enable' : 'Disable'}</button>
          <button class="btn btn-ghost btn-sm wi-delete" data-index="${i}" style="color:var(--danger);">Del</button>
        </div>
      </div>
      <div class="cr-entry-card-keys">Keys: ${(entry.keys || []).join(", ") || "(none)"}</div>
      <div class="cr-entry-card-content">${esc((entry.content || "").slice(0, 100))}</div>
      <div style="font-size:0.7rem;color:var(--text-muted);margin-top:4px;">
        Position: ${displayPosition(entry)} | Depth: ${entry.insertion_depth ?? "auto"} | Role: ${entry.role || "system"} | Order: ${entry.order ?? i} | ${entry.constant ? "Constant" : "Keyword"}${entry.selective ? " + Filter" : ""}${entry.enable_semantic_search ? " + RAG" : ""} | ${isEntryDisabled(entry) ? "DISABLED" : "Active"}
      </div>
    </div>
  `).join("");

  list.querySelectorAll(".wi-edit").forEach(b => b.addEventListener("click", e => { e.stopPropagation(); openEntryEditor(parseInt(b.dataset.index)); }));
  list.querySelectorAll(".wi-toggle").forEach(b => b.addEventListener("click", e => { e.stopPropagation(); toggleEntry(parseInt(b.dataset.index)); }));
  list.querySelectorAll(".wi-delete").forEach(b => b.addEventListener("click", e => { e.stopPropagation(); deleteEntry(parseInt(b.dataset.index)); }));
  list.querySelectorAll(".cr-entry-card").forEach(card => {
    card.addEventListener("click", () => openEntryEditor(parseInt(card.dataset.index)));
  });
}

function openEntryEditor(index) {
  editingIndex = index;
  const entry = normalizeEntry(index >= 0 ? state.worldInfo[index] : null, index);
  const container = document.getElementById("wi-modal-container");
  container.innerHTML = `
    <div class="cr-modal-overlay" id="wi-modal-overlay">
      <div class="cr-modal">
        <div class="cr-modal-header">
          <h3>${index >= 0 ? "Edit Entry" : "New Entry"}</h3>
          <button class="btn btn-ghost btn-sm" id="wi-modal-close">&times;</button>
        </div>
        <div class="cr-modal-body">
          <div class="cr-form-group">
            <label><span>ID</span><input type="text" id="wi-edit-id" value="${esc(entry.id || "")}" placeholder="e.g. char_backstory" /></label>
          </div>
          <div class="cr-form-group">
            <label><span>Trigger Keys (comma separated)</span><input type="text" id="wi-edit-keys" value="${esc((entry.keys || []).join(", "))}" placeholder="e.g. sword, combat, battle" /></label>
            <div class="cr-form-hint">Use <code>re:pattern</code> prefix for regex matching.</div>
          </div>
          <div class="cr-form-group">
            <label><span>Secondary Keys</span><input type="text" id="wi-edit-sec-keys" value="${esc((entry.secondary_keys || []).join(", "))}" placeholder="(optional)" /></label>
          </div>
          <div class="cr-form-row">
            <div class="cr-form-group">
              <label><span>Selective Logic</span>
              <select id="wi-edit-selective-logic">
                <option value="and_any" ${entry.selective_logic === "and_any" ? "selected" : ""}>AND ANY</option>
                <option value="and_all" ${entry.selective_logic === "and_all" ? "selected" : ""}>AND ALL</option>
                <option value="not_any" ${entry.selective_logic === "not_any" ? "selected" : ""}>NOT ANY</option>
                <option value="not_all" ${entry.selective_logic === "not_all" ? "selected" : ""}>NOT ALL</option>
              </select></label>
            </div>
            <div class="cr-form-group">
              <label><span>Trigger Probability</span><input type="number" id="wi-edit-probability" value="${entry.probability ?? 100}" min="0" max="100" step="1" /></label>
            </div>
          </div>
          <div class="cr-form-group">
            <label><span>Content *</span><textarea id="wi-edit-content" rows="5">${esc(entry.content || "")}</textarea></label>
          </div>
          <div class="cr-form-row">
            <div class="cr-form-group">
              <label><span>Position</span>
              <select id="wi-edit-position">
                <option value="auto" ${(!entry.position || entry.position === "auto") ? "selected" : ""}>auto</option>
                <option value="top" ${entry.position === "top" ? "selected" : ""}>top</option>
                <option value="relative" ${entry.position === "relative" ? "selected" : ""}>relative</option>
                <option value="in_chat" ${entry.position === "in_chat" ? "selected" : ""}>in_chat</option>
              </select></label>
            </div>
            <div class="cr-form-group">
              <label><span>Insertion Depth</span><input type="number" id="wi-edit-depth" value="${entry.insertion_depth ?? ""}" placeholder="0 = near top, empty = auto" min="0" /></label>
            </div>
          </div>
          <div class="cr-form-row">
            <div class="cr-form-group">
              <label><span>Order</span><input type="number" id="wi-edit-order" value="${entry.order ?? index * 100}" /></label>
            </div>
            <div class="cr-form-group">
              <label><span>Role</span>
              <select id="wi-edit-role">
                <option value="system" ${entry.role === "system" ? "selected" : ""}>system</option>
                <option value="user" ${entry.role === "user" ? "selected" : ""}>user</option>
                <option value="assistant" ${entry.role === "assistant" ? "selected" : ""}>assistant</option>
              </select></label>
            </div>
          </div>
          <div class="cr-form-row">
            <div class="cr-form-group">
              <label><span>Scan Depth Override</span><input type="number" id="wi-edit-scan-depth" value="${entry.scan_depth ?? ""}" placeholder="empty = global" min="1" /></label>
            </div>
            <div class="cr-form-group">
              <label><span>Case / Word Match</span>
              <select id="wi-edit-match-mode">
                <option value="global" ${entry.case_sensitive == null && entry.match_whole_words == null ? "selected" : ""}>Use global defaults</option>
                <option value="case" ${entry.case_sensitive === true && entry.match_whole_words !== true ? "selected" : ""}>Case sensitive</option>
                <option value="whole" ${entry.case_sensitive !== true && entry.match_whole_words === true ? "selected" : ""}>Whole words</option>
                <option value="case_whole" ${entry.case_sensitive === true && entry.match_whole_words === true ? "selected" : ""}>Case + whole words</option>
              </select></label>
            </div>
          </div>
          <div class="cr-form-row" style="align-items:flex-start;">
            <label style="flex-direction:row;align-items:center;gap:8px;">
              <input type="checkbox" id="wi-edit-constant" ${entry.constant ? "checked" : ""} />
              <span>Constant</span>
            </label>
            <label style="flex-direction:row;align-items:center;gap:8px;">
              <input type="checkbox" id="wi-edit-selective" ${entry.selective ? "checked" : ""} />
              <span>Selective</span>
            </label>
          </div>
          <div class="cr-form-row" style="align-items:flex-start;">
            <label style="flex-direction:row;align-items:center;gap:8px;">
              <input type="checkbox" id="wi-edit-recursive" ${entry.recursive !== false ? "checked" : ""} />
              <span>Can be triggered recursively</span>
            </label>
            <label style="flex-direction:row;align-items:center;gap:8px;">
              <input type="checkbox" id="wi-edit-prevent-recursion" ${entry.prevent_recursion ? "checked" : ""} />
              <span>Prevent recursion from this entry</span>
            </label>
          </div>
          <div class="cr-form-group">
            <label style="flex-direction:row;align-items:center;gap:8px;">
              <input type="checkbox" id="wi-edit-delay-recursion" ${entry.delay_until_recursion ? "checked" : ""} />
              <span>Delay until recursion</span>
            </label>
          </div>
          <div class="cr-form-group">
            <label style="flex-direction:row;align-items:center;gap:8px;">
              <input type="checkbox" id="wi-edit-rag" ${entry.enable_semantic_search ? "checked" : ""} />
              <span>Enable Semantic Search (RAG vector matching)</span>
            </label>
          </div>
          <div class="cr-form-hint">
            <strong>Trigger modes:</strong><br>
            - <strong>Global (always active):</strong> Set keys to <code>*</code> and use depth 0<br>
            - <strong>Keyword-triggered:</strong> Entry activates when any key matches the user's message<br>
            - <strong>RAG vector search:</strong> Enable checkbox above for semantic similarity matching<br>
            - <strong>Disabled:</strong> Close editor and click "Disable" on the entry card
          </div>
        </div>
        <div class="cr-modal-footer">
          <button class="btn btn-ghost btn-sm" id="wi-modal-cancel">Cancel</button>
          <button class="btn btn-primary btn-sm" id="wi-modal-save">Save Entry</button>
        </div>
      </div>
    </div>
  `;

  document.getElementById("wi-modal-overlay").addEventListener("click", e => { if (e.target.id === "wi-modal-overlay") closeModal(); });
  document.getElementById("wi-modal-close").addEventListener("click", closeModal);
  document.getElementById("wi-modal-cancel").addEventListener("click", closeModal);
  document.getElementById("wi-modal-save").addEventListener("click", saveEntry);
}

function closeModal() {
  document.getElementById("wi-modal-container").innerHTML = "";
  editingIndex = -1;
}

function saveEntry() {
  const matchMode = document.getElementById("wi-edit-match-mode").value;
  const entry = {
    id: document.getElementById("wi-edit-id").value.trim(),
    enabled: editingIndex >= 0 ? !isEntryDisabled(state.worldInfo[editingIndex]) : true,
    keys: parseKeys(document.getElementById("wi-edit-keys").value),
    secondary_keys: parseKeys(document.getElementById("wi-edit-sec-keys").value),
    content: document.getElementById("wi-edit-content").value,
    constant: document.getElementById("wi-edit-constant").checked,
    selective: document.getElementById("wi-edit-selective").checked,
    selective_logic: document.getElementById("wi-edit-selective-logic").value,
    case_sensitive: matchMode.includes("case") ? true : null,
    match_whole_words: matchMode.includes("whole") ? true : null,
    scan_depth: parseOptionalInt(document.getElementById("wi-edit-scan-depth").value),
    probability: clampNumber(parseFloat(document.getElementById("wi-edit-probability").value || "100"), 0, 100),
    recursive: document.getElementById("wi-edit-recursive").checked,
    prevent_recursion: document.getElementById("wi-edit-prevent-recursion").checked,
    delay_until_recursion: document.getElementById("wi-edit-delay-recursion").checked,
    insertion_depth: parseOptionalInt(document.getElementById("wi-edit-depth").value),
    position: document.getElementById("wi-edit-position").value,
    order: parseInt(document.getElementById("wi-edit-order").value || "0"),
    role: document.getElementById("wi-edit-role").value,
    enable_semantic_search: document.getElementById("wi-edit-rag").checked,
  };

  if (!entry.content.trim()) {
    showToast("Content is required", "error");
    return;
  }

  if (editingIndex >= 0) {
    state.worldInfo[editingIndex] = entry;
  } else {
    state.worldInfo.push(entry);
  }

  scheduleAutoSave("worldInfo");
  closeModal();
  renderEntryList();
}

function toggleEntry(index) {
  state.worldInfo[index].enabled = isEntryDisabled(state.worldInfo[index]);
  delete state.worldInfo[index]._disabled;
  scheduleAutoSave("worldInfo");
  renderEntryList();
}

function deleteEntry(index) {
  if (!confirm("Delete this entry?")) return;
  state.worldInfo.splice(index, 1);
  scheduleAutoSave("worldInfo");
  renderEntryList();
}

function parseKeys(input) {
  return input.split(",").map(s => s.trim()).filter(s => s.length > 0);
}

function parseOptionalInt(s) {
  return s.trim() ? parseInt(s) : null;
}

function clampNumber(value, min, max) {
  if (!Number.isFinite(value)) return max;
  return Math.max(min, Math.min(max, value));
}

function normalizeEntry(entry, index) {
  return {
    id: "",
    enabled: true,
    keys: [],
    content: "",
    secondary_keys: [],
    enable_semantic_search: false,
    constant: false,
    selective: false,
    selective_logic: "and_any",
    case_sensitive: null,
    match_whole_words: null,
    scan_depth: null,
    probability: 100,
    recursive: true,
    prevent_recursion: false,
    delay_until_recursion: false,
    insertion_depth: null,
    position: "auto",
    order: Math.max(index, 0) * 100 || state.worldInfo.length * 100,
    role: "system",
    ...(entry || {}),
  };
}

function isEntryDisabled(entry) {
  return entry.enabled === false || entry._disabled === true;
}

function displayPosition(entry) {
  if (entry.position && entry.position !== "auto") return entry.position;
  return entry.insertion_depth == null ? "top(auto)" : "in_chat(auto)";
}

function esc(s) { return (s || "").replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;"); }

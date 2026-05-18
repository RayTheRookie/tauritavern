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
        Position: ${displayPosition(entry)} | Depth: ${entry.insertion_depth ?? "auto"} | Role: ${entry.role || "system"} | Order: ${entry.order ?? i} | ${entry.enable_semantic_search ? "RAG" : "Keyword"} | ${isEntryDisabled(entry) ? "DISABLED" : "Active"}
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
  const entry = index >= 0 ? state.worldInfo[index] : { id: "", enabled: true, keys: [], content: "", secondary_keys: [], enable_semantic_search: false, insertion_depth: null, position: "auto", order: state.worldInfo.length * 100, role: "system" };
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
  const entry = {
    id: document.getElementById("wi-edit-id").value.trim(),
    enabled: editingIndex >= 0 ? !isEntryDisabled(state.worldInfo[editingIndex]) : true,
    keys: parseKeys(document.getElementById("wi-edit-keys").value),
    secondary_keys: parseKeys(document.getElementById("wi-edit-sec-keys").value),
    content: document.getElementById("wi-edit-content").value,
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

function isEntryDisabled(entry) {
  return entry.enabled === false || entry._disabled === true;
}

function displayPosition(entry) {
  if (entry.position && entry.position !== "auto") return entry.position;
  return entry.insertion_depth == null ? "top(auto)" : "in_chat(auto)";
}

function esc(s) { return (s || "").replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;"); }

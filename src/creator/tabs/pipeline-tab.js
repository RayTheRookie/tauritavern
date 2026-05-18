import { state, scheduleAutoSave, showToast } from "../creator.js";

let mutatorEditingIndex = -1;

export function renderPipeline(panel) {
  const cs = state.pipeline.context_strategy;
  panel.innerHTML = `
    <h2 class="cr-section-title">Context Strategy</h2>
    <div class="cr-form-row">
      <div class="cr-form-group">
        <label><span>Max Context Tokens</span><input type="number" id="pl-max-ctx" value="${cs.max_context_tokens}" step="512" min="512" /></label>
        <div class="cr-form-hint">Total context window budget.</div>
      </div>
      <div class="cr-form-group">
        <label><span>History Fetch Limit</span><input type="number" id="pl-history-limit" value="${cs.history_fetch_limit}" step="10" min="1" /></label>
        <div class="cr-form-hint">Max recent history messages to consider.</div>
      </div>
    </div>
    <div class="cr-form-row">
      <div class="cr-form-group">
        <label><span>RAG Fetch Count</span><input type="number" id="pl-rag-count" value="${cs.rag_fetch_count}" step="1" min="0" max="20" /></label>
        <div class="cr-form-hint">Max world entries fetched via semantic search.</div>
      </div>
      <div class="cr-form-group">
        <label><span>RAG Similarity Threshold</span><input type="number" id="pl-rag-threshold" value="${cs.rag_similarity_threshold}" step="0.05" min="0" max="1" /></label>
        <div class="cr-form-hint">Minimum cosine similarity (0-1). Higher = stricter matching.</div>
      </div>
    </div>

    <h2 class="cr-section-title" style="margin-top:20px;">Regex</h2>
    <div style="margin-bottom:8px;">
      <button id="pl-add-mutator" class="btn btn-primary btn-sm">+ Add Regex</button>
    </div>
    <div id="pl-mutator-list" class="cr-entry-list"></div>
    <div id="pl-modal-container"></div>
  `;

  bindNum("pl-max-ctx", "max_context_tokens");
  bindNum("pl-history-limit", "history_fetch_limit");
  bindNum("pl-rag-count", "rag_fetch_count");
  bindFloat("pl-rag-threshold", "rag_similarity_threshold");

  document.getElementById("pl-add-mutator").addEventListener("click", () => openMutatorEditor(-1));
  renderMutatorList();
}

function bindNum(id, key) {
  const el = document.getElementById(id);
  if (!el) return;
  el.addEventListener("input", () => { state.pipeline.context_strategy[key] = parseInt(el.value); scheduleAutoSave("pipeline"); });
}

function bindFloat(id, key) {
  const el = document.getElementById(id);
  if (!el) return;
  el.addEventListener("input", () => { state.pipeline.context_strategy[key] = parseFloat(el.value); scheduleAutoSave("pipeline"); });
}

function renderMutatorList() {
  const list = document.getElementById("pl-mutator-list");
  if (!list) return;

  if (state.pipeline.regex_mutators.length === 0) {
    list.innerHTML = '<div class="cr-form-hint" style="text-align:center;padding:16px;">No regex rules. Add one for prompt history cleanup or frontend display replacement.</div>';
    return;
  }

  list.innerHTML = state.pipeline.regex_mutators.map((m, i) => `
    <div class="cr-entry-card" data-index="${i}">
      <div class="cr-entry-card-header">
        <span class="cr-entry-card-title">${esc(m.id || "Mutator #" + (i + 1))}</span>
        <div class="cr-entry-card-actions">
          <button class="btn btn-ghost btn-sm pl-code" data-index="${i}">Code</button>
          <button class="btn btn-ghost btn-sm pl-edit" data-index="${i}">Edit</button>
          <button class="btn btn-ghost btn-sm pl-delete" data-index="${i}" style="color:var(--danger);">Del</button>
        </div>
      </div>
      <div style="font-size:0.75rem;color:var(--text-muted);">Target: ${esc(m.target || "history")} | ${m.enabled === false ? "Disabled" : "Enabled"} | Flags: ${esc(m.flags || "gs")} | Depth: [${m.depth_range?.[0] ?? 0}, ${m.depth_range?.[1] ?? "∞"}]</div>
      <div style="font-size:0.72rem;color:var(--text-muted);font-family:monospace;margin-top:2px;">/${esc(m.pattern || "")}/ → ${esc(previewOneLine(m.replacement || ""))}</div>
    </div>
  `).join("");

  list.querySelectorAll(".pl-code").forEach(b => b.addEventListener("click", e => { e.stopPropagation(); openReplacementInCode(parseInt(b.dataset.index)); }));
  list.querySelectorAll(".pl-edit").forEach(b => b.addEventListener("click", e => { e.stopPropagation(); openMutatorEditor(parseInt(b.dataset.index)); }));
  list.querySelectorAll(".pl-delete").forEach(b => b.addEventListener("click", e => { e.stopPropagation(); deleteMutator(parseInt(b.dataset.index)); }));
  list.querySelectorAll(".cr-entry-card").forEach(card => {
    card.addEventListener("click", () => openMutatorEditor(parseInt(card.dataset.index)));
  });
}

function openMutatorEditor(index) {
  mutatorEditingIndex = index;
  const m = index >= 0 ? state.pipeline.regex_mutators[index] : { id: "", enabled: true, target: "display", depth_range: [], pattern: "", replacement: "", flags: "gs", sample: "", description: "" };
  const container = document.getElementById("pl-modal-container");
  container.innerHTML = `
    <div class="cr-modal-overlay" id="pl-modal-overlay">
      <div class="cr-modal regex-modal">
        <div class="cr-modal-header">
          <h3>${index >= 0 ? "Edit Regex" : "New Regex"}</h3>
          <button class="btn btn-ghost btn-sm" id="pl-modal-close">&times;</button>
        </div>
        <div class="cr-modal-body">
          <div class="cr-form-row">
            <div class="cr-form-group">
              <label><span>ID *</span><input type="text" id="pl-edit-id" value="${esc(m.id || "")}" /></label>
            </div>
            <div class="cr-form-group">
              <label><span>Target</span>
                <select id="pl-edit-target">
                  <option value="display" ${(m.target || "display") === "display" ? "selected" : ""}>display / frontend</option>
                  <option value="history" ${m.target === "history" ? "selected" : ""}>history / prompt</option>
                </select>
              </label>
            </div>
          </div>
          <label class="code-toggle" style="margin-bottom:10px;"><input id="pl-edit-enabled" type="checkbox" ${m.enabled === false ? "" : "checked"} /> Enabled</label>
          <div class="cr-form-row" id="pl-depth-row">
            <div class="cr-form-group">
              <label><span>Depth Start</span><input type="number" id="pl-edit-depth-start" value="${m.depth_range?.[0] ?? 0}" min="0" /></label>
            </div>
            <div class="cr-form-group">
              <label><span>Depth End (empty = ∞)</span><input type="number" id="pl-edit-depth-end" value="${m.depth_range?.[1] ?? ""}" min="0" placeholder="∞" /></label>
            </div>
          </div>
          <div class="cr-form-group">
            <label><span>Regex Pattern *</span><input type="text" id="pl-edit-pattern" value="${esc(m.pattern || "")}" placeholder="e.g. <text>[\\s\\S]*?</text>" /></label>
          </div>
          <div class="cr-form-group">
            <label><span>Flags</span><input type="text" id="pl-edit-flags" value="${esc(m.flags || "gs")}" placeholder="gimsu" /></label>
          </div>
          <div class="regex-editor-grid">
            <div class="cr-form-group">
              <label><span>Replacement / Frontend Code</span><textarea id="pl-edit-replacement" rows="12" spellcheck="false">${esc(m.replacement || "")}</textarea></label>
              <button class="btn btn-ghost btn-sm" id="pl-edit-code" type="button">Edit in Code Area</button>
            </div>
            <div class="cr-form-group">
              <label><span>Preview Sample</span><textarea id="pl-edit-sample" rows="5" spellcheck="false" placeholder="Text matched from a message, e.g. <Gui>...</Gui>">${esc(m.sample || "")}</textarea></label>
              <div class="regex-preview" id="pl-regex-preview" title="Click to edit replacement in the code area"></div>
            </div>
          </div>
          <div class="cr-form-group">
            <label><span>Description</span><input type="text" id="pl-edit-desc" value="${esc(m.description || "")}" /></label>
          </div>
        </div>
        <div class="cr-modal-footer">
          <button class="btn btn-ghost btn-sm" id="pl-modal-cancel">Cancel</button>
          <button class="btn btn-primary btn-sm" id="pl-modal-save">Save Regex</button>
        </div>
      </div>
    </div>
  `;

  document.getElementById("pl-modal-overlay").addEventListener("click", e => { if (e.target.id === "pl-modal-overlay") closeMutatorModal(); });
  document.getElementById("pl-modal-close").addEventListener("click", closeMutatorModal);
  document.getElementById("pl-modal-cancel").addEventListener("click", closeMutatorModal);
  document.getElementById("pl-modal-save").addEventListener("click", saveMutator);
  document.getElementById("pl-edit-code").addEventListener("click", () => {
    saveMutator({ stayOpen: false, openCode: true });
  });
  document.getElementById("pl-regex-preview").addEventListener("click", () => {
    saveMutator({ stayOpen: false, openCode: true });
  });
  ["pl-edit-pattern", "pl-edit-replacement", "pl-edit-sample", "pl-edit-flags"].forEach(id => {
    document.getElementById(id).addEventListener("input", renderRegexPreview);
  });
  document.getElementById("pl-edit-target").addEventListener("change", updateDepthVisibility);
  updateDepthVisibility();
  renderRegexPreview();
}

function closeMutatorModal() {
  document.getElementById("pl-modal-container").innerHTML = "";
  mutatorEditingIndex = -1;
}

function saveMutator(options = {}) {
  const depthEnd = document.getElementById("pl-edit-depth-end").value.trim();
  const depthRange = depthEnd ? [parseInt(document.getElementById("pl-edit-depth-start").value), parseInt(depthEnd)] : [parseInt(document.getElementById("pl-edit-depth-start").value)];

  const mutator = {
    id: document.getElementById("pl-edit-id").value.trim(),
    target: document.getElementById("pl-edit-target").value.trim() || "history",
    enabled: document.getElementById("pl-edit-enabled").checked,
    depth_range: depthRange,
    pattern: document.getElementById("pl-edit-pattern").value,
    replacement: document.getElementById("pl-edit-replacement").value,
    flags: document.getElementById("pl-edit-flags").value.trim() || "gs",
    sample: document.getElementById("pl-edit-sample").value,
    description: document.getElementById("pl-edit-desc").value.trim(),
  };

  if (!mutator.id || !mutator.pattern) {
    showToast("ID and Pattern are required", "error");
    return;
  }

  let savedIndex = mutatorEditingIndex;
  if (mutatorEditingIndex >= 0) {
    state.pipeline.regex_mutators[mutatorEditingIndex] = mutator;
  } else {
    state.pipeline.regex_mutators.push(mutator);
    savedIndex = state.pipeline.regex_mutators.length - 1;
  }

  scheduleAutoSave("pipeline");
  if (!options.stayOpen) closeMutatorModal();
  renderMutatorList();
  if (options.openCode) openReplacementInCode(savedIndex);
}

function deleteMutator(index) {
  if (!confirm("Delete this mutator?")) return;
  state.pipeline.regex_mutators.splice(index, 1);
  scheduleAutoSave("pipeline");
  renderMutatorList();
}

function updateDepthVisibility() {
  const isHistory = document.getElementById("pl-edit-target").value === "history";
  document.getElementById("pl-depth-row").classList.toggle("hidden", !isHistory);
}

function renderRegexPreview() {
  const preview = document.getElementById("pl-regex-preview");
  if (!preview) return;
  const pattern = document.getElementById("pl-edit-pattern").value;
  const flags = document.getElementById("pl-edit-flags").value || "gs";
  const sample = document.getElementById("pl-edit-sample").value || "";
  const replacement = document.getElementById("pl-edit-replacement").value || "";
  try {
    const normalized = normalizeRegex(pattern, flags);
    const rendered = sample
      ? sample.replace(new RegExp(normalized.pattern, normalized.flags), replacement)
      : replacement;
    preview.innerHTML = rendered;
  } catch (e) {
    preview.textContent = "Invalid regex: " + e.message;
  }
}

function openReplacementInCode(index) {
  window.dispatchEvent(new CustomEvent("creator-edit-regex-replacement", { detail: { index } }));
}

function normalizeRegex(pattern, fallbackFlags) {
  const match = String(pattern || "").match(/^\/([\s\S]*)\/([a-z]*)$/);
  if (!match) return { pattern: String(pattern || ""), flags: uniqueFlags(fallbackFlags || "gs") };
  return { pattern: match[1], flags: uniqueFlags(match[2] || fallbackFlags || "gs") };
}

function uniqueFlags(flags) {
  return Array.from(new Set(String(flags || "").replace(/[^dgimsuvy]/g, "").split(""))).join("");
}

function previewOneLine(text) {
  const value = String(text || "").replace(/\s+/g, " ").trim();
  return value.length > 120 ? value.slice(0, 117) + "..." : value;
}

function esc(s) { return (s || "").replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;"); }

import { state, scheduleAutoSave, showToast, schedulePreviewRefresh } from "../creator.js";

let activeCodeFile = "index.html";
let selectFileListenerBound = false;

const FILES = ["index.html", "script.js", "style.css"];

export function renderUICode(panel) {
  ensureFileExists(activeCodeFile);
  const settings = state.editorSettings || { fontSize: 13, tabSize: 2, wordWrap: false, autoPreview: true };
  panel.innerHTML = `
    <div class="code-workbench">
      <div class="code-topbar">
        <div class="cr-code-tabs" id="ui-code-tabs">
          ${FILES.map(f => `<button class="cr-code-tab ${f === activeCodeFile ? "active" : ""}" data-file="${f}">${f}</button>`).join("")}
          ${isRegexFile(activeCodeFile) ? `<button class="cr-code-tab active" data-file="${activeCodeFile}">${esc(codeFileLabel(activeCodeFile))}</button>` : ""}
        </div>
        <div class="code-settings">
          <label title="Font size"><span>Font</span><input id="code-font-size" type="number" min="10" max="22" value="${settings.fontSize}" /></label>
          <label title="Tab size"><span>Tab</span><input id="code-tab-size" type="number" min="2" max="8" value="${settings.tabSize}" /></label>
          <label class="code-toggle"><input id="code-word-wrap" type="checkbox" ${settings.wordWrap ? "checked" : ""} /> Wrap</label>
          <label class="code-toggle"><input id="code-auto-preview" type="checkbox" ${settings.autoPreview !== false ? "checked" : ""} /> Live</label>
          <button id="code-refresh-preview" class="btn btn-primary btn-sm">Preview</button>
        </div>
      </div>
      <div class="code-editor-shell">
        <div class="code-gutter" id="code-gutter"></div>
        <div class="code-editor-stack">
          <pre id="code-highlight" aria-hidden="true"></pre>
          <textarea id="ui-code-textarea" spellcheck="false" autocomplete="off" autocapitalize="off"></textarea>
        </div>
      </div>
      <div class="code-statusbar">
        <span id="code-status-file">${activeCodeFile}</span>
        <span id="code-status-position">Ln 1, Col 1</span>
        <span>UTF-8</span>
      </div>
    </div>
  `;

  bindFileTabs();
  bindEditor();
  bindSettings();
  renderEditorValue();
  bindExternalFileSelection();
  schedulePreviewRefresh(0);
}

function bindFileTabs() {
  document.querySelectorAll("#ui-code-tabs .cr-code-tab").forEach(btn => {
    btn.addEventListener("click", () => switchCodeFile(btn.dataset.file));
  });
}

function refreshCodeTabs() {
  const tabs = document.getElementById("ui-code-tabs");
  if (!tabs) return;
  tabs.innerHTML = `
    ${FILES.map(f => `<button class="cr-code-tab ${f === activeCodeFile ? "active" : ""}" data-file="${f}">${f}</button>`).join("")}
    ${isRegexFile(activeCodeFile) ? `<button class="cr-code-tab active" data-file="${activeCodeFile}">${esc(codeFileLabel(activeCodeFile))}</button>` : ""}
  `;
  bindFileTabs();
}

function bindEditor() {
  const textarea = editor();
  textarea.addEventListener("input", () => {
    writeActiveValue(textarea.value);
    updateEditorChrome();
    if (state.editorSettings.autoPreview !== false) {
      schedulePreviewRefresh(80);
    }
  });

  textarea.addEventListener("scroll", syncScroll);
  textarea.addEventListener("click", updateCursorStatus);
  textarea.addEventListener("keyup", updateCursorStatus);
  textarea.addEventListener("keydown", e => {
    if (e.key === "Tab") {
      e.preventDefault();
      insertText(" ".repeat(state.editorSettings.tabSize || 2));
    }
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
      e.preventDefault();
      scheduleAutoSave("uiFiles");
      schedulePreviewRefresh(0);
      showToast("Saved " + activeCodeFile, "success");
    }
    if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
      e.preventDefault();
      schedulePreviewRefresh(0);
    }
  });
}

function bindSettings() {
  const font = document.getElementById("code-font-size");
  const tab = document.getElementById("code-tab-size");
  const wrap = document.getElementById("code-word-wrap");
  const live = document.getElementById("code-auto-preview");
  const refresh = document.getElementById("code-refresh-preview");

  font.addEventListener("input", () => {
    state.editorSettings.fontSize = clamp(parseInt(font.value || "13"), 10, 22);
    applyEditorSettings();
  });
  tab.addEventListener("input", () => {
    state.editorSettings.tabSize = clamp(parseInt(tab.value || "2"), 2, 8);
    applyEditorSettings();
  });
  wrap.addEventListener("change", () => {
    state.editorSettings.wordWrap = wrap.checked;
    applyEditorSettings();
  });
  live.addEventListener("change", () => {
    state.editorSettings.autoPreview = live.checked;
    if (live.checked) schedulePreviewRefresh(0);
  });
  refresh.addEventListener("click", () => schedulePreviewRefresh(0));
}

function bindExternalFileSelection() {
  if (selectFileListenerBound) return;
  selectFileListenerBound = true;
  window.addEventListener("creator-select-ui-file", event => {
    const file = event.detail?.file;
    if (FILES.includes(file)) {
      switchCodeFile(file);
    }
  });
  window.addEventListener("creator-select-regex-replacement", event => {
    const index = event.detail?.index;
    if (Number.isInteger(index)) {
      switchCodeFile(`regex:${index}`);
    }
  });
}

function switchCodeFile(filename) {
  if (!FILES.includes(filename) && !isRegexFile(filename)) return;
  if (editor()) {
    writeActiveValue(editor().value);
  }
  activeCodeFile = filename;
  ensureFileExists(activeCodeFile);
  refreshCodeTabs();
  renderEditorValue();
}

function renderEditorValue() {
  const textarea = editor();
  textarea.value = readActiveValue();
  applyEditorSettings();
  updateEditorChrome();
  textarea.focus();
}

function applyEditorSettings() {
  const textarea = editor();
  const highlight = document.getElementById("code-highlight");
  const gutter = document.getElementById("code-gutter");
  const size = `${state.editorSettings.fontSize || 13}px`;
  const tabSize = state.editorSettings.tabSize || 2;
  const wrap = state.editorSettings.wordWrap;
  for (const el of [textarea, highlight, gutter]) {
    if (!el) continue;
    el.style.fontSize = size;
    el.style.tabSize = tabSize;
  }
  textarea.classList.toggle("wrap", wrap);
  highlight.classList.toggle("wrap", wrap);
}

function updateEditorChrome() {
  const text = editor().value;
  document.getElementById("code-highlight").innerHTML = highlightCode(text, activeCodeFile);
  document.getElementById("code-gutter").textContent = buildLineNumbers(text);
  document.getElementById("code-status-file").textContent = codeFileLabel(activeCodeFile);
  syncScroll();
  updateCursorStatus();
}

function syncScroll() {
  const textarea = editor();
  const highlight = document.getElementById("code-highlight");
  const gutter = document.getElementById("code-gutter");
  highlight.scrollTop = textarea.scrollTop;
  highlight.scrollLeft = textarea.scrollLeft;
  gutter.scrollTop = textarea.scrollTop;
}

function updateCursorStatus() {
  const textarea = editor();
  const pos = textarea.selectionStart;
  const before = textarea.value.slice(0, pos);
  const lines = before.split("\n");
  const line = lines.length;
  const col = lines[lines.length - 1].length + 1;
  document.getElementById("code-status-position").textContent = `Ln ${line}, Col ${col}`;
}

function insertText(text) {
  const textarea = editor();
  const start = textarea.selectionStart;
  const end = textarea.selectionEnd;
  textarea.setRangeText(text, start, end, "end");
  textarea.dispatchEvent(new Event("input", { bubbles: true }));
}

function highlightCode(text, filename) {
  const escaped = esc(text);
  if (filename.endsWith(".html") || isRegexFile(filename)) {
    return escaped
      .replace(/(&lt;!--[\s\S]*?--&gt;)/g, '<span class="tok-comment">$1</span>')
      .replace(/(&lt;\/?)([a-zA-Z0-9-]+)/g, '$1<span class="tok-tag">$2</span>')
      .replace(/\s([a-zA-Z-:]+)=(&quot;.*?&quot;|'.*?')/g, ' <span class="tok-attr">$1</span>=<span class="tok-string">$2</span>');
  }
  if (filename.endsWith(".css")) {
    return escaped
      .replace(/(\/\*[\s\S]*?\*\/)/g, '<span class="tok-comment">$1</span>')
      .replace(/([.#]?[a-zA-Z_][\w-]*)(\s*\{)/g, '<span class="tok-selector">$1</span>$2')
      .replace(/([a-zA-Z-]+)(\s*:)/g, '<span class="tok-attr">$1</span>$2')
      .replace(/(:\s*)([^;{}]+)/g, '$1<span class="tok-string">$2</span>');
  }
  return escaped
    .replace(/(\/\/.*?$|\/\*[\s\S]*?\*\/)/gm, '<span class="tok-comment">$1</span>')
    .replace(/(&quot;.*?&quot;|'.*?'|`[\s\S]*?`)/g, '<span class="tok-string">$1</span>')
    .replace(/\b(const|let|var|function|return|if|else|for|while|class|new|await|async|try|catch|import|export|from)\b/g, '<span class="tok-keyword">$1</span>')
    .replace(/\b([A-Z][A-Za-z0-9_]*)\b/g, '<span class="tok-type">$1</span>');
}

function buildLineNumbers(text) {
  const count = Math.max(1, text.split("\n").length);
  return Array.from({ length: count }, (_, i) => String(i + 1)).join("\n");
}

function ensureFileExists(file) {
  if (isRegexFile(file)) {
    const index = regexIndex(file);
    if (!state.pipeline.regex_mutators[index]) {
      state.pipeline.regex_mutators[index] = {
        id: `display_regex_${index + 1}`,
        enabled: true,
        placement: "display",
        target: "display",
        depth_range: [],
        pattern: "",
        replacement: "",
        flags: "gs",
        sample: "",
        description: "",
        markdown_only: true,
        prompt_only: false,
        run_on_edit: true,
      };
    }
    return;
  }
  if (state.uiFiles[file] == null) state.uiFiles[file] = "";
}

function readActiveValue() {
  if (isRegexFile(activeCodeFile)) {
    return state.pipeline.regex_mutators[regexIndex(activeCodeFile)]?.replacement || "";
  }
  return state.uiFiles[activeCodeFile] || "";
}

function writeActiveValue(value) {
  if (isRegexFile(activeCodeFile)) {
    const index = regexIndex(activeCodeFile);
    ensureFileExists(activeCodeFile);
    state.pipeline.regex_mutators[index].replacement = value;
    scheduleAutoSave("pipeline");
    return;
  }
  state.uiFiles[activeCodeFile] = value;
  scheduleAutoSave("uiFiles");
}

function isRegexFile(file) {
  return /^regex:\d+$/.test(file || "");
}

function regexIndex(file) {
  return parseInt(String(file).split(":")[1], 10);
}

function codeFileLabel(file) {
  if (!isRegexFile(file)) return file;
  const index = regexIndex(file);
  const mutator = state.pipeline.regex_mutators[index];
  return `regex/${mutator?.id || index + 1}.html`;
}

function editor() {
  return document.getElementById("ui-code-textarea");
}

function clamp(value, min, max) {
  return Math.max(min, Math.min(max, Number.isFinite(value) ? value : min));
}

function esc(s) { return (s || "").replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;"); }

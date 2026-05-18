import { state, scheduleAutoSave, invoke, showToast } from "../creator.js";
import { open } from "@tauri-apps/plugin-dialog";

export function renderMetadata(panel) {
  const m = state.manifest;
  panel.innerHTML = `
    <h2 class="cr-section-title">Cartridge Metadata</h2>
    <div class="cr-form-row">
      <div class="cr-form-group">
        <label><span>Name *</span><input type="text" id="meta-name" value="${esc(m.name)}" /></label>
      </div>
      <div class="cr-form-group">
        <label><span>Author *</span><input type="text" id="meta-author" value="${esc(m.author)}" /></label>
      </div>
    </div>
    <div class="cr-form-row">
      <div class="cr-form-group">
        <label><span>Version</span><input type="text" id="meta-version" value="${esc(m.version)}" /></label>
      </div>
      <div class="cr-form-group">
        <label><span>Entry File</span><input type="text" id="meta-entry" value="${esc(m.entry_file)}" /></label>
      </div>
    </div>
    <div class="cr-form-group">
      <label><span>Description</span><textarea id="meta-desc" rows="3">${esc(m.description)}</textarea></label>
    </div>
    <div class="cr-form-group">
      <label><span>Cover Image (path relative to cartridge root, e.g. assets/cover.png)</span>
      <input type="text" id="meta-cover" value="${esc(m.cover_image)}" placeholder="assets/cover.png" /></label>
      <div class="cover-picker-row">
        <button id="meta-choose-cover" class="btn btn-primary btn-sm" type="button">Choose Image</button>
        <button id="meta-clear-cover" class="btn btn-ghost btn-sm" type="button">Clear</button>
      </div>
      <div id="meta-cover-preview" class="cover-preview">${coverPreview(m.cover_image)}</div>
      <div class="cr-form-hint">Selected images are copied into <code>assets/</code> so exported cards stay portable.</div>
    </div>
  `;

  ["name","author","version","entry","desc","cover"].forEach(id => {
    const el = document.getElementById("meta-" + id);
    if (!el) return;
    el.addEventListener("input", () => {
      const key = id === "entry" ? "entry_file" : id === "desc" ? "description" : id === "cover" ? "cover_image" : id;
      state.manifest[key] = el.value;
      scheduleAutoSave("manifest");
    });
  });

  document.getElementById("meta-choose-cover").addEventListener("click", chooseCoverImage);
  document.getElementById("meta-clear-cover").addEventListener("click", () => {
    state.manifest.cover_image = "";
    document.getElementById("meta-cover").value = "";
    document.getElementById("meta-cover-preview").innerHTML = coverPreview("");
    scheduleAutoSave("manifest");
  });
}

async function chooseCoverImage() {
  try {
    const selected = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp", "gif", "svg"] }],
    });
    if (!selected) return;
    const manifest = await invoke("import_workbench_cover_image", {
      workbenchId: state.workbenchId,
      sourcePath: selected,
    });
    state.manifest = manifest;
    document.getElementById("meta-cover").value = manifest.cover_image || "";
    document.getElementById("meta-cover-preview").innerHTML = coverPreview(manifest.cover_image);
    showToast("Cover image imported", "success");
  } catch (e) {
    showToast("Cover import failed: " + e, "error");
  }
}

function coverPreview(path) {
  if (!path) return '<div class="cover-preview-empty">No cover image</div>';
  const src = `tavern://localhost/workbench/${state.workbenchId}/${path}`;
  return `<img src="${esc(src)}" alt="Cover preview" />`;
}

function esc(s) { return (s || "").replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;"); }

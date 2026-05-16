import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

// ── State ──────────────────────────────────

let cartridges = [];

// ── DOM refs ───────────────────────────────

const grid = document.getElementById("cartridge-grid");
const btnImport = document.getElementById("btn-import");
const btnSettings = document.getElementById("btn-settings");
const btnCloseSettings = document.getElementById("btn-close-settings");
const settingsModal = document.getElementById("settings-modal");
const toast = document.getElementById("toast");

// ── Init ───────────────────────────────────

document.addEventListener("DOMContentLoaded", async () => {
  await loadCartridges();
  await loadApiKeys();
  setupEventListeners();
});

function setupEventListeners() {
  btnImport.addEventListener("click", handleImport);
  btnSettings.addEventListener("click", () => settingsModal.classList.remove("hidden"));
  btnCloseSettings.addEventListener("click", () => settingsModal.classList.add("hidden"));
  settingsModal.querySelector(".modal-backdrop").addEventListener("click", () => {
    settingsModal.classList.add("hidden");
  });

  // Save API key buttons
  settingsModal.querySelectorAll("[data-provider]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const provider = btn.dataset.provider;
      const input = document.getElementById(`api-key-${provider}`);
      const key = input.value.trim();
      try {
        if (key) {
          await invoke("set_api_key", { provider, key });
          showToast(`API key for ${provider} saved`, "success");
        } else {
          await invoke("delete_api_key", { provider });
          showToast(`API key for ${provider} removed`, "success");
        }
      } catch (e) {
        showToast(`Error: ${e}`, "error");
      }
    });
  });
}

// ── Cartridge Operations ───────────────────

async function loadCartridges() {
  try {
    cartridges = await invoke("list_cartridges");
    renderGrid();
  } catch (e) {
    showToast(`Failed to load cartridges: ${e}`, "error");
  }
}

async function handleImport() {
  try {
    const file = await open({
      multiple: false,
      filters: [{ name: "TauriTavern Card", extensions: ["taurichar"] }],
    });

    if (!file) return;

    const info = await invoke("import_cartridge", { filePath: file });
    showToast(`Imported "${info.name}"`, "success");
    await loadCartridges();
  } catch (e) {
    showToast(`Import failed: ${e}`, "error");
  }
}

async function handleOpenCartridge(id) {
  try {
    await invoke("open_cartridge", { cartridgeId: id });
  } catch (e) {
    showToast(`Failed to open: ${e}`, "error");
  }
}

async function handleDeleteCartridge(id, name) {
  if (!confirm(`Delete "${name}" and all its chats?`)) return;
  try {
    await invoke("delete_cartridge", { id });
    showToast(`Deleted "${name}"`, "success");
    await loadCartridges();
  } catch (e) {
    showToast(`Delete failed: ${e}`, "error");
  }
}

// ── API Keys ───────────────────────────────

async function loadApiKeys() {
  try {
    const keys = await invoke("get_all_api_keys");
    for (const { key, value } of keys) {
      const provider = key.replace("api_key_", "");
      const input = document.getElementById(`api-key-${provider}`);
      if (input) {
        input.value = value;
      }
    }
  } catch (e) {
    // Settings might not exist yet — that's fine
  }
}

// ── Render ─────────────────────────────────

function renderGrid() {
  if (cartridges.length === 0) {
    grid.innerHTML = `
      <div class="empty-state">
        <p>No cartridges installed.</p>
        <p class="sub">Click "Import Cartridge" to add a <code>.taurichar</code> file.</p>
      </div>`;
    return;
  }

  grid.innerHTML = cartridges
    .map(
      (c) => `
    <div class="card" data-id="${c.id}">
      ${
        c.cover_image
          ? `<img class="card-cover" src="data:image/png;base64,${c.cover_image}" alt="${c.name}" />`
          : `<div class="card-cover-placeholder">&#128214;</div>`
      }
      <div class="card-body">
        <div class="card-title">${escapeHtml(c.name)}</div>
        <div class="card-author">by ${escapeHtml(c.author)}</div>
      </div>
      <div class="card-actions">
        <button class="btn btn-primary btn-sm btn-open">Open</button>
        <button class="btn btn-ghost btn-sm btn-delete">Delete</button>
      </div>
    </div>`
    )
    .join("");

  // Attach event listeners
  grid.querySelectorAll(".card").forEach((card) => {
    const id = card.dataset.id;
    card.querySelector(".btn-open").addEventListener("click", (e) => {
      e.stopPropagation();
      handleOpenCartridge(id);
    });
    card.querySelector(".btn-delete").addEventListener("click", (e) => {
      e.stopPropagation();
      const name = card.querySelector(".card-title").textContent;
      handleDeleteCartridge(id, name);
    });
    // Click card body to open
    card.addEventListener("click", () => handleOpenCartridge(id));
  });
}

// ── Helpers ────────────────────────────────

function escapeHtml(text) {
  const div = document.createElement("div");
  div.textContent = text;
  return div.innerHTML;
}

function showToast(message, type = "info") {
  toast.textContent = message;
  toast.className = `toast ${type}`;
  const timeout = setTimeout(() => {
    toast.classList.add("hidden");
  }, 3000);
  toast.dataset.timeout = timeout;
}

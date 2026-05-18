import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

// ── State ──────────────────────────────────

let cartridges = [];
let providers = [];
let selectedProvider = null;
let fetchedModels = [];
let selectedModel = null;
let profiles = [];
let activeProfileId = null;

// ── DOM refs ───────────────────────────────

const grid = document.getElementById("cartridge-grid");
const btnImport = document.getElementById("btn-import");
const btnSettings = document.getElementById("btn-settings");
const btnCloseSettings = document.getElementById("btn-close-settings");
const settingsModal = document.getElementById("settings-modal");
const providerDropdown = document.getElementById("provider-dropdown");
const providerConfig = document.getElementById("provider-config");
const apiKeyInput = document.getElementById("api-key-input");
const providerUrlGroup = document.getElementById("provider-url-group");
const providerUrlInput = document.getElementById("provider-url-input");
const btnFetchModels = document.getElementById("btn-fetch-models");
const modelSelectGroup = document.getElementById("model-select-group");
const modelDropdown = document.getElementById("model-dropdown");
const connectionStatus = document.getElementById("connection-status");
const profileSaveGroup = document.getElementById("profile-save-group");
const profileNameInput = document.getElementById("profile-name-input");
const btnConnect = document.getElementById("btn-connect");
const profileList = document.getElementById("profile-list");
const toast = document.getElementById("toast");

// ── Init ───────────────────────────────────

document.addEventListener("DOMContentLoaded", async () => {
  await loadCartridges();
  await loadProviders();
  await loadActiveProfile();
  await loadProfiles();
  setupEventListeners();
});

function setupEventListeners() {
  document.getElementById("btn-create").addEventListener("click", async () => {
    try {
      const workbenchId = await invoke("open_creator");
      console.log("Creator opened, workbench:", workbenchId);
    } catch (e) {
      showToast("Failed to open creator: " + e, "error");
    }
  });
  btnImport.addEventListener("click", handleImport);
  btnSettings.addEventListener("click", () => settingsModal.classList.remove("hidden"));
  btnCloseSettings.addEventListener("click", () => settingsModal.classList.add("hidden"));
  settingsModal.querySelector(".modal-backdrop").addEventListener("click", () => {
    settingsModal.classList.add("hidden");
  });

  // Provider dropdown change
  providerDropdown.addEventListener("change", async () => {
    const id = providerDropdown.value;
    selectedProvider = providers.find((p) => p.id === id) || null;
    if (selectedProvider) {
      await showProviderForm(selectedProvider);
    } else {
      hideProviderForm();
    }
  });

  // Fetch models
  btnFetchModels.addEventListener("click", handleFetchModels);

  // Model selection
  modelDropdown.addEventListener("change", () => {
    const model = modelDropdown.value;
    selectedModel = model || null;
    updateConnectVisibility();

    if (selectedModel && selectedProvider) {
      profileNameInput.value = `${selectedProvider.id}-${selectedModel}`;
    }
  });

  // Connect (test + save + activate in one step)
  btnConnect.addEventListener("click", handleConnect);

  // Diagnose
  const btnDiagnose = document.getElementById("btn-diagnose");
  const diagnoseOutput = document.getElementById("diagnose-output");
  if (btnDiagnose) {
    btnDiagnose.addEventListener("click", async () => {
      try {
        const result = await invoke("diagnose");
        diagnoseOutput.textContent = JSON.stringify(result, null, 2);
        diagnoseOutput.classList.remove("hidden");
      } catch (e) {
        diagnoseOutput.textContent = "Error: " + e;
        diagnoseOutput.classList.remove("hidden");
      }
    });
  }

  // Check Keyring
  const btnCheckKeyring = document.getElementById("btn-check-keyring");
  if (btnCheckKeyring) {
    btnCheckKeyring.addEventListener("click", async () => {
      try {
        const result = await invoke("check_keyring");
        diagnoseOutput.textContent = result;
        diagnoseOutput.classList.remove("hidden");
      } catch (e) {
        diagnoseOutput.textContent = "Keyring check: " + e;
        diagnoseOutput.classList.remove("hidden");
      }
    });
  }
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
      filters: [{ name: "Character Card", extensions: ["taurichar", "png"] }],
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

async function handleEditCartridge(id) {
  try {
    const workbenchId = await invoke("open_creator_for_cartridge", { cartridgeId: id });
    console.log("Creator opened for cartridge, workbench:", workbenchId);
  } catch (e) {
    showToast(`Failed to edit: ${e}`, "error");
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

// ── Providers & Settings ────────────────────

async function loadProviders() {
  try {
    providers = await invoke("get_providers");

    providerDropdown.innerHTML = '<option value="">-- Select Provider --</option>';
    for (const p of providers) {
      const opt = document.createElement("option");
      opt.value = p.id;
      opt.textContent = p.display_name;
      providerDropdown.appendChild(opt);
    }
  } catch (e) {
    showToast(`Failed to load providers: ${e}`, "error");
  }
}

async function showProviderForm(info) {
  providerConfig.classList.remove("hidden");
  resetProviderForm();

  // Determine URL field behavior
  if (info.requires_url) {
    providerUrlGroup.classList.remove("hidden");
    providerUrlInput.readOnly = false;
    providerUrlInput.placeholder = "https://api.example.com/v1/chat/completions";
    providerUrlInput.value = "";
  } else {
    providerUrlGroup.classList.remove("hidden");
    providerUrlInput.readOnly = true;
    providerUrlInput.value = info.default_url || "";
    providerUrlInput.placeholder = "";
  }

  apiKeyInput.placeholder = info.key_placeholder;
  apiKeyInput.value = "";

  try {
    const keys = await invoke("get_all_api_keys");
    const entry = keys.find((k) => k.key === `api_key_${info.id}`);
    if (entry && entry.has_key) {
      apiKeyInput.placeholder = "Key saved (masked)";
    }

    const savedUrl = await invoke("get_provider_url", { provider: info.id });
    if (savedUrl) {
      providerUrlInput.value = savedUrl;
      if (info.requires_url) {
        providerUrlInput.readOnly = false;
      }
    }
  } catch (e) {
    // fine if nothing saved yet
  }

  btnFetchModels.classList.remove("hidden");
}

function hideProviderForm() {
  providerConfig.classList.add("hidden");
  resetProviderForm();
}

function resetProviderForm() {
  fetchedModels = [];
  selectedModel = null;
  modelDropdown.innerHTML = '<option value="">-- Select Model --</option>';
  modelSelectGroup.classList.add("hidden");
  connectionStatus.classList.add("hidden");
  profileSaveGroup.classList.add("hidden");
  profileNameInput.value = "";
}

async function handleFetchModels() {
  if (!selectedProvider) return;

  let apiKey = apiKeyInput.value.trim();
  const apiUrl = providerUrlInput.value.trim();

  if (!apiKey) {
    try {
      apiKey = await invoke("get_raw_api_key", { provider: selectedProvider.id });
      if (!apiKey) {
        showToast("Please enter an API key first", "error");
        return;
      }
    } catch (e) {
      showToast("Please enter an API key first", "error");
      return;
    }
  }

  if (!apiUrl) {
    showToast("API URL is required", "error");
    return;
  }

  btnFetchModels.disabled = true;
  btnFetchModels.textContent = "Fetching...";

  try {
    fetchedModels = await invoke("fetch_models", {
      provider: selectedProvider.id,
      apiKey: apiKey,
      apiUrl: apiUrl,
    });

    // Save key to keyring for later use
    await invoke("set_api_key", { provider: selectedProvider.id, key: apiKey });
    if (selectedProvider.requires_url && apiUrl) {
      await invoke("set_provider_url", { provider: selectedProvider.id, url: apiUrl });
    }

    modelDropdown.innerHTML = '<option value="">-- Select Model --</option>';
    for (const m of fetchedModels) {
      const opt = document.createElement("option");
      opt.value = m;
      opt.textContent = m;
      modelDropdown.appendChild(opt);
    }

    modelSelectGroup.classList.remove("hidden");
    connectionStatus.classList.add("hidden");
    updateConnectVisibility();
    showToast(`Loaded ${fetchedModels.length} models`, "success");
  } catch (e) {
    showToast(`Failed to fetch models: ${e}`, "error");
  } finally {
    btnFetchModels.disabled = false;
    btnFetchModels.textContent = "Fetch Models";
  }
}

// ── Connect = test + save + activate ─────────

function updateConnectVisibility() {
  if (selectedModel) {
    profileSaveGroup.classList.remove("hidden");
  } else {
    profileSaveGroup.classList.add("hidden");
    connectionStatus.classList.add("hidden");
  }
}

async function handleConnect() {
  if (!selectedProvider || !selectedModel) return;
  if (fetchedModels.length === 0) {
    showToast("Please fetch models first to verify the connection", "error");
    return;
  }

  const apiUrl = providerUrlInput.value.trim();
  if (!apiUrl) {
    showToast("API URL is required", "error");
    return;
  }

  btnConnect.disabled = true;
  btnConnect.textContent = "Saving...";

  // Connection already verified by fetch_models — save profile
  const name = profileNameInput.value.trim() || `${selectedProvider.id}-${selectedModel}`;
  try {
    const saved = await invoke("save_profile", {
      name: name,
      providerId: selectedProvider.id,
      model: selectedModel,
      apiUrl: apiUrl,
    });
    activeProfileId = saved.id;
    console.log("[handleConnect] calling set_active_profile with:", saved.id);
    await invoke("set_active_profile", { profileId: saved.id });
    console.log("[handleConnect] set_active_profile succeeded");

    connectionStatus.classList.remove("hidden");
    connectionStatus.textContent = `Connected to ${selectedModel}`;
    connectionStatus.className = "connection-ok";
    showToast(`已选择 "${name}"`, "success");
    await loadProfiles();
  } catch (e) {
    connectionStatus.classList.remove("hidden");
    connectionStatus.textContent = `Save failed: ${e}`;
    connectionStatus.className = "connection-error";
  } finally {
    btnConnect.disabled = false;
    btnConnect.textContent = "Connect";
  }
}

// ── Profiles ────────────────────────────────

async function loadActiveProfile() {
  try {
    const active = await invoke("get_active_profile");
    if (active) {
      activeProfileId = active.id;
    }
  } catch (e) {
    activeProfileId = null;
  }
}

async function loadProfiles() {
  try {
    profiles = await invoke("list_profiles");
    renderProfiles();
  } catch (e) {
    profiles = [];
  }
}

function renderProfiles() {
  if (!profileList) return;

  if (profiles.length === 0) {
    profileList.innerHTML = '<p class="sub">No saved profiles.</p>';
    return;
  }

  profileList.innerHTML = profiles
    .map(
      (p) => `
    <div class="profile-item${p.id === activeProfileId ? " active" : ""}" data-id="${p.id}">
      <div class="profile-info">
        <span class="profile-name">${escapeHtml(p.name)}</span>
        <span class="profile-detail">${escapeHtml(p.provider_id)} / ${escapeHtml(p.model)}</span>
      </div>
      <div class="profile-item-actions">
        <button class="btn btn-ghost btn-sm btn-profile-view" data-id="${p.id}">View</button>
        <button class="btn btn-ghost btn-sm btn-profile-delete" data-id="${p.id}">Delete</button>
      </div>
    </div>`
    )
    .join("");

  // View button
  profileList.querySelectorAll(".btn-profile-view").forEach((btn) => {
    btn.addEventListener("click", async (e) => {
      e.stopPropagation();
      const id = btn.dataset.id;
      try {
        const profile = await invoke("view_profile", { id });
        alert(`Profile: ${profile.name}\nProvider: ${profile.provider_id}\nModel: ${profile.model}\nURL: ${profile.api_url || "(default)"}\nHas API Key: ${profile.has_api_key ? "Yes" : "No"}\nCreated: ${profile.created_at}`);
      } catch (err) {
        showToast(`Failed to view profile: ${err}`, "error");
      }
    });
  });

  // Click profile item → auto-connect
  profileList.querySelectorAll(".profile-item").forEach((item) => {
    item.addEventListener("click", async (e) => {
      if (e.target.closest(".btn-profile-delete")) return;
      const id = item.dataset.id;
      await connectProfile(id);
    });
  });

  // Delete button
  profileList.querySelectorAll(".btn-profile-delete").forEach((btn) => {
    btn.addEventListener("click", async (e) => {
      e.stopPropagation();
      const id = btn.dataset.id;
      try {
        if (id === activeProfileId) {
          activeProfileId = null;
        }
        await invoke("delete_profile", { id });
        showToast("Profile deleted", "success");
        await loadProfiles();
      } catch (err) {
        showToast(`Failed to delete profile: ${err}`, "error");
      }
    });
  });
}

async function connectProfile(profileId) {
  const profile = profiles.find((p) => p.id === profileId);
  if (!profile) return;

  // Set as active immediately — validation happens when user actually sends a message
  console.log("[connectProfile] calling set_active_profile with:", profile.id, profile);
  activeProfileId = profile.id;
  await invoke("set_active_profile", { profileId: profile.id });
  console.log("[connectProfile] set_active_profile succeeded");
  showToast(`已选择 "${profile.name}"`, "success");
  renderProfiles();
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
          ? `<img class="card-cover" src="${c.cover_image}" alt="${c.name}" />`
          : `<div class="card-cover-placeholder">&#128214;</div>`
      }
      <div class="card-body">
        <div class="card-title">${escapeHtml(c.name)}</div>
        <div class="card-author">by ${escapeHtml(c.author)}</div>
      </div>
      <div class="card-actions">
        <button class="btn btn-primary btn-sm btn-open">Open</button>
        <button class="btn btn-ghost btn-sm btn-edit">Edit</button>
        <button class="btn btn-ghost btn-sm btn-delete">Delete</button>
      </div>
    </div>`
    )
    .join("");

  grid.querySelectorAll(".card").forEach((card) => {
    const id = card.dataset.id;
    card.querySelector(".btn-open").addEventListener("click", (e) => {
      e.stopPropagation();
      handleOpenCartridge(id);
    });
    card.querySelector(".btn-edit").addEventListener("click", (e) => {
      e.stopPropagation();
      handleEditCartridge(id);
    });
    card.querySelector(".btn-delete").addEventListener("click", (e) => {
      e.stopPropagation();
      const name = card.querySelector(".card-title").textContent;
      handleDeleteCartridge(id, name);
    });
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
  clearTimeout(Number(toast.dataset.timeout));
  const timeout = setTimeout(() => {
    toast.classList.add("hidden");
  }, 3000);
  toast.dataset.timeout = timeout;
}

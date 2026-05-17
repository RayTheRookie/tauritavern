// ── Wine Sword Immortal — Chat UI ─────────────────

const SDK = window.TavernSDK;

// ── State ──────────────────────────────────

let currentChatId = null;
let isStreaming = false;

// ── DOM ────────────────────────────────────

const messageArea = document.getElementById("message-area");
const messageInput = document.getElementById("message-input");
const btnSend = document.getElementById("btn-send");
const btnNewChat = document.getElementById("btn-new-chat");
const btnDryRun = document.getElementById("btn-dry-run");
const btnCloseChatList = document.getElementById("btn-close-chat-list");
const btnCloseXray = document.getElementById("btn-close-xray");
const chatListPanel = document.getElementById("chat-list-panel");
const chatList = document.getElementById("chat-list");
const xrayPanel = document.getElementById("xray-panel");
const xrayContent = document.getElementById("xray-content");
const streamingIndicator = document.getElementById("streaming-indicator");
const welcomeMessage = messageArea.querySelector(".welcome-message");

// ── Init ───────────────────────────────────

document.addEventListener("DOMContentLoaded", async () => {
  btnSend.addEventListener("click", handleSend);
  btnNewChat.addEventListener("click", toggleChatList);
  btnDryRun.addEventListener("click", handleDryRun);
  btnCloseChatList.addEventListener("click", () => chatListPanel.classList.add("hidden"));
  btnCloseXray.addEventListener("click", () => xrayPanel.classList.add("hidden"));

  messageInput.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  });

  // Auto-resize textarea
  messageInput.addEventListener("input", () => {
    messageInput.style.height = "auto";
    messageInput.style.height = Math.min(messageInput.scrollHeight, 120) + "px";
  });

  // Load existing chats
  await loadChatList();
});

// ── Chat Operations ────────────────────────

async function loadChatList() {
  try {
    const chats = await SDK.listChats();
    renderChatList(chats);
  } catch (e) {
    console.error("Failed to load chats:", e);
  }
}

function renderChatList(chats) {
  if (chats.length === 0) {
    chatList.innerHTML =
      '<div style="padding:16px;color:var(--text-muted);text-align:center;">No chats yet.</div>';
    return;
  }

  chatList.innerHTML = chats
    .map(
      (c) => `
    <div class="chat-list-item ${c.id === currentChatId ? "active" : ""}" data-id="${c.id}">
      <button class="chat-list-item-delete" data-id="${c.id}">&times;</button>
      <div class="chat-list-item-title">${escapeHtml(c.title)}</div>
      <div class="chat-list-item-time">${formatTime(c.updated_at)}</div>
    </div>`
    )
    .join("");

  chatList.querySelectorAll(".chat-list-item").forEach((item) => {
    item.addEventListener("click", (e) => {
      if (e.target.classList.contains("chat-list-item-delete")) return;
      switchChat(item.dataset.id);
      chatListPanel.classList.add("hidden");
    });
  });

  chatList.querySelectorAll(".chat-list-item-delete").forEach((btn) => {
    btn.addEventListener("click", async (e) => {
      e.stopPropagation();
      const id = btn.dataset.id;
      await SDK.deleteChat(id);
      if (currentChatId === id) {
        currentChatId = null;
        messageArea.innerHTML = "";
        messageArea.appendChild(welcomeMessage);
        welcomeMessage.style.display = "";
      }
      await loadChatList();
    });
  });
}

async function switchChat(chatId) {
  currentChatId = chatId;
  messageArea.innerHTML = "";

  try {
    const messages = await SDK.getMessages(chatId);
    for (const msg of messages) {
      appendMessage(msg.role, msg.content);
    }
  } catch (e) {
    console.error("Failed to load messages:", e);
  }

  scrollToBottom();
}

async function handleSend() {
  if (isStreaming) return;

  const message = messageInput.value.trim();
  if (!message) return;

  // Create chat if needed
  if (!currentChatId) {
    try {
      const chat = await SDK.createChat();
      currentChatId = chat.id;
      hideWelcome();
      await loadChatList();
    } catch (e) {
      console.error("Failed to create chat:", e);
      return;
    }
  }

  hideWelcome();

  // Show user message
  appendMessage("user", message);
  messageInput.value = "";
  messageInput.style.height = "auto";
  scrollToBottom();

  // Start streaming
  isStreaming = true;
  btnSend.disabled = true;
  showStreamingIndicator();

  const assistantBubble = createAssistantBubble();
  messageArea.appendChild(assistantBubble);
  scrollToBottom();

  try {
    await SDK.sendMessage(
      currentChatId,
      message,
      (chunk) => {
        assistantBubble.textContent += chunk;
        scrollToBottom();
      },
      () => {
        // done
        isStreaming = false;
        btnSend.disabled = false;
        hideStreamingIndicator();
        messageInput.focus();
      },
      (error) => {
        // error
        assistantBubble.textContent += `\n\n[Error: ${error}]`;
        isStreaming = false;
        btnSend.disabled = false;
        hideStreamingIndicator();
      }
    );
  } catch (e) {
    assistantBubble.textContent += `\n\n[Error: ${e}]`;
    isStreaming = false;
    btnSend.disabled = false;
    hideStreamingIndicator();
  }
}

async function handleDryRun() {
  const message = messageInput.value.trim() || "Tell me what you remember about wine and swords.";
  xrayPanel.classList.remove("hidden");
  xrayContent.innerHTML = '<div class="xray-loading">Running...</div>';

  try {
    const result = await SDK.dryRunPromptPipeline(currentChatId, message);
    renderXray(result);
  } catch (e) {
    xrayContent.innerHTML = `<div class="xray-error">${escapeHtml(String(e))}</div>`;
  }
}

function toggleChatList() {
  chatListPanel.classList.toggle("hidden");
  if (!chatListPanel.classList.contains("hidden")) {
    loadChatList();
  }
}

function renderXray(result) {
  const budget = result.budget || {};
  const total = Math.max(1, budget.total_tokens || 0);
  const systemPct = Math.round(((budget.system_tokens || 0) / total) * 100);
  const lorePct = Math.round(((budget.lore_tokens || 0) / total) * 100);
  const ragPct = Math.round(((budget.rag_tokens || 0) / total) * 100);
  const historyPct = Math.max(0, 100 - systemPct - lorePct - ragPct);

  const world = result.world_triggers || [];
  const recalls = result.rag_recalls || [];
  const mutations = result.regex_mutations || [];
  const insertions = result.insertions || [];

  xrayContent.innerHTML = `
    <div class="budget-row">
      <div class="budget-pie" style="background: conic-gradient(
        #d4956b 0 ${systemPct}%,
        #6bb7d4 ${systemPct}% ${systemPct + lorePct}%,
        #9bd46b ${systemPct + lorePct}% ${systemPct + lorePct + ragPct}%,
        #8f7ad4 ${systemPct + lorePct + ragPct}% 100%
      )"></div>
      <div class="budget-list">
        <div>System ${systemPct}%</div>
        <div>Lore ${lorePct}%</div>
        <div>RAG ${ragPct}%</div>
        <div>History ${historyPct}%</div>
        <div>${budget.total_tokens || 0}/${budget.max_context_tokens || 0} tokens</div>
      </div>
    </div>

    <div class="xray-section">
      <h4>Triggers</h4>
      ${renderTriggerList(world, recalls)}
    </div>

    <div class="xray-section">
      <h4>Mutations</h4>
      ${mutations.length ? mutations.map((m) => `
        <div class="xray-item">${escapeHtml(m.mutator_id)} depth ${m.depth}: ${m.before_tokens} -> ${m.after_tokens}</div>
      `).join("") : '<div class="xray-muted">None</div>'}
    </div>

    <div class="xray-section">
      <h4>Insertions</h4>
      ${insertions.length ? insertions.map((item) => `
        <div class="xray-item">${escapeHtml(item.label)} at index ${item.index}</div>
      `).join("") : '<div class="xray-muted">None</div>'}
    </div>

    <div class="xray-section">
      <h4>Payload</h4>
      <pre class="payload-waterfall">${escapeHtml(result.final_text || "")}</pre>
    </div>
  `;
}

function renderTriggerList(world, recalls) {
  const worldHtml = world.map((entry) => `
    <div class="xray-item">World: ${escapeHtml(entry.id || entry.keys.join(", "))} (${escapeHtml(entry.trigger)})</div>
  `);
  const recallHtml = recalls.map((entry) => `
    <div class="xray-item">Recall: ${Math.round(entry.similarity * 100)}% ${escapeHtml(entry.content.slice(0, 80))}</div>
  `);
  const html = [...worldHtml, ...recallHtml];
  return html.length ? html.join("") : '<div class="xray-muted">None</div>';
}

// ── UI Helpers ─────────────────────────────

function appendMessage(role, content) {
  const div = document.createElement("div");
  div.className = `message ${role}`;
  div.textContent = content;
  messageArea.appendChild(div);
}

function createAssistantBubble() {
  const div = document.createElement("div");
  div.className = "message assistant";
  div.textContent = "";
  return div;
}

function hideWelcome() {
  if (welcomeMessage) {
    welcomeMessage.style.display = "none";
  }
}

function showStreamingIndicator() {
  streamingIndicator.classList.remove("hidden");
}

function hideStreamingIndicator() {
  streamingIndicator.classList.add("hidden");
}

function scrollToBottom() {
  messageArea.scrollTop = messageArea.scrollHeight;
}

function escapeHtml(text) {
  const div = document.createElement("div");
  div.textContent = text;
  return div.innerHTML;
}

function formatTime(isoString) {
  try {
    const d = new Date(isoString);
    return d.toLocaleString();
  } catch {
    return isoString;
  }
}

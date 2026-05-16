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
const btnCloseChatList = document.getElementById("btn-close-chat-list");
const chatListPanel = document.getElementById("chat-list-panel");
const chatList = document.getElementById("chat-list");
const streamingIndicator = document.getElementById("streaming-indicator");
const welcomeMessage = messageArea.querySelector(".welcome-message");

// ── Init ───────────────────────────────────

document.addEventListener("DOMContentLoaded", async () => {
  btnSend.addEventListener("click", handleSend);
  btnNewChat.addEventListener("click", toggleChatList);
  btnCloseChatList.addEventListener("click", () => chatListPanel.classList.add("hidden"));

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

function toggleChatList() {
  chatListPanel.classList.toggle("hidden");
  if (!chatListPanel.classList.contains("hidden")) {
    loadChatList();
  }
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

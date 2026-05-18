// ── 林晓悦 — Galgame Chat UI ─────────────────

const SDK = window.TavernSDK;

// ── State ──────────────────────────────────
let currentChatId = null;
let isStreaming = false;

// ── DOM ────────────────────────────────────
const dialogBox = document.getElementById("dialog-box");
const dialogNameTag = document.getElementById("dialog-name-tag");
const dialogText = document.getElementById("dialog-text");
const dialogHint = document.getElementById("dialog-hint");
const userInput = document.getElementById("user-input");
const btnSend = document.getElementById("btn-send");
const streamingDot = document.getElementById("streaming-dot");
const historyPanel = document.getElementById("history-panel");
const historyContent = document.getElementById("history-content");
const btnHistory = document.getElementById("btn-history");
const btnNewChat = document.getElementById("btn-new-chat");
const btnCloseHistory = document.getElementById("btn-close-history");

// ── Init ───────────────────────────────────

document.addEventListener("DOMContentLoaded", () => {
  userInput.focus();

  // Send button
  btnSend.addEventListener("click", () => {
    const text = userInput.value.trim();
    if (text) {
      userInput.value = "";
      handleSend(text);
    }
  });

  // Enter to send
  userInput.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && !isStreaming) {
      const text = userInput.value.trim();
      if (text) {
        userInput.value = "";
        handleSend(text);
      }
    }
  });

  // Click dialog box → focus input
  dialogBox.addEventListener("click", () => userInput.focus());

  // History
  btnHistory.addEventListener("click", () => {
    historyPanel.classList.remove("hidden");
    loadHistory();
  });
  btnCloseHistory.addEventListener("click", () => {
    historyPanel.classList.add("hidden");
  });

  // New chat
  btnNewChat.addEventListener("click", async () => {
    currentChatId = null;
    dialogNameTag.textContent = "林晓悦";
    dialogText.textContent = "嗨！我是林晓悦，高二三班的学生～\n今天有什么想聊的吗？ (｡･ω･｡)";
    dialogHint.style.display = "";
    userInput.value = "";
    userInput.focus();
  });

  // Keep focus on input
  document.addEventListener("click", () => userInput.focus());
});

// ── Send Message ───────────────────────────

async function handleSend(message) {
  if (isStreaming) return;

  // Create chat if needed
  if (!currentChatId) {
    try {
      const chat = await SDK.createChat();
      currentChatId = chat.id;
    } catch (e) {
      dialogNameTag.textContent = "系统";
      dialogText.textContent = "创建对话失败: " + e;
      return;
    }
  }

  // Show user message in dialog
  dialogNameTag.textContent = "你";
  dialogText.textContent = message;
  dialogHint.style.display = "none";

  // Start streaming
  isStreaming = true;
  btnSend.disabled = true;
  userInput.disabled = true;
  streamingDot.classList.remove("hidden");

  let fullResponse = "";

  try {
    await SDK.sendMessage(
      currentChatId,
      message,
      (chunk) => {
        fullResponse += chunk;
        dialogNameTag.textContent = "林晓悦";
        dialogText.textContent = fullResponse;
      },
      () => {
        // done
        finishResponse();
      },
      (error) => {
        fullResponse += `\n[错误: ${error}]`;
        dialogNameTag.textContent = "系统";
        dialogText.textContent = fullResponse;
        finishResponse();
      }
    );
  } catch (e) {
    dialogNameTag.textContent = "系统";
    dialogText.textContent = "发送失败: " + e;
    finishResponse();
  }
}

function finishResponse() {
  isStreaming = false;
  btnSend.disabled = false;
  userInput.disabled = false;
  streamingDot.classList.add("hidden");
  dialogHint.style.display = "";
  userInput.focus();
}

// ── History ────────────────────────────────

async function loadHistory() {
  if (!currentChatId) {
    historyContent.innerHTML =
      '<div style="color:var(--text-muted);text-align:center;padding:32px;">还没有对话记录呢～</div>';
    return;
  }

  try {
    const messages = await SDK.getMessages(currentChatId);
    if (messages.length === 0) {
      historyContent.innerHTML =
        '<div style="color:var(--text-muted);text-align:center;padding:32px;">还没有对话记录呢～</div>';
      return;
    }
    historyContent.innerHTML = messages
      .map(
        (m) =>
          `<div class="history-msg ${m.role}">${escapeHtml(m.content)}</div>`
      )
      .join("");
    historyContent.scrollTop = historyContent.scrollHeight;
  } catch (e) {
    historyContent.innerHTML =
      `<div style="color:#e74c5e;text-align:center;padding:32px;">加载失败: ${escapeHtml(String(e))}</div>`;
  }
}

// ── Helpers ────────────────────────────────

function escapeHtml(text) {
  const div = document.createElement("div");
  div.textContent = text;
  return div.innerHTML;
}

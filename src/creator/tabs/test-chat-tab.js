import { state, showToast } from "../creator.js";

let chatId = null;
let isStreaming = false;

export function renderTestChat(panel) {
  panel.innerHTML = `
    <div class="cr-chat-area">
      <div class="cr-chat-messages" id="tc-messages">
        <div class="cr-chat-msg system">Test your character here. Messages use the <strong>active profile</strong> from Console settings.</div>
      </div>
      <div class="cr-chat-input-row">
        <input type="text" id="tc-input" placeholder="Type a message to test your character..." autofocus />
        <button class="btn btn-primary btn-sm" id="tc-send">Send</button>
        <button class="btn btn-ghost btn-sm" id="tc-new-chat">New Chat</button>
      </div>
    </div>
  `;

  document.getElementById("tc-send").addEventListener("click", handleSend);
  document.getElementById("tc-new-chat").addEventListener("click", () => { chatId = null; document.getElementById("tc-messages").innerHTML = '<div class="cr-chat-msg system">New chat started.</div>'; });
  document.getElementById("tc-input").addEventListener("keydown", e => {
    if (e.key === "Enter" && !isStreaming) handleSend();
  });
}

async function handleSend() {
  if (isStreaming) return;
  const input = document.getElementById("tc-input");
  const message = input.value.trim();
  if (!message) return;
  input.value = "";

  const msgArea = document.getElementById("tc-messages");
  msgArea.appendChild(createMsgEl("user", message));
  msgArea.scrollTop = msgArea.scrollHeight;

  isStreaming = true;
  document.getElementById("tc-send").disabled = true;
  document.getElementById("tc-input").disabled = true;

  const assistantEl = createMsgEl("assistant", "");
  msgArea.appendChild(assistantEl);

  try {
    const { invoke } = await import("../creator.js");
    const response = await invoke("test_chat_workbench", {
      workbenchId: state.workbenchId,
      message: message,
    });
    assistantEl.textContent = response;
  } catch (e) {
    assistantEl.textContent = "Error: " + e;
    assistantEl.style.color = "var(--danger)";
  } finally {
    isStreaming = false;
    document.getElementById("tc-send").disabled = false;
    document.getElementById("tc-input").disabled = false;
    document.getElementById("tc-input").focus();
    msgArea.scrollTop = msgArea.scrollHeight;
  }
}

function createMsgEl(role, text) {
  const div = document.createElement("div");
  div.className = "cr-chat-msg " + role;
  div.textContent = text;
  return div;
}

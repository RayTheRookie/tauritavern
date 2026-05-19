/**
 * TauriTavern SDK v0.1.0
 *
 * Include this script in your cartridge's ui/index.html to access
 * the TauriTavern chat API from a sandboxed webview.
 *
 * Usage:
 *   <script src="../tauri-tavern-sdk.js"></script>
 *   <script>
 *     const cartridgeId = await TavernSDK.getCartridgeId();
 *     const chats = await TavernSDK.listChats();
 *     await TavernSDK.sendMessage(chatId, "Hello!", (chunk) => {
 *       document.getElementById("output").textContent += chunk;
 *     });
 *   </script>
 */

(function () {
  "use strict";

  const TAVERN_PROTOCOL = "tavern://localhost";

  function resolveUrl(path) {
    return `${TAVERN_PROTOCOL}/${path}`;
  }

  function getInvoke() {
    if (typeof window !== "undefined" && window.__TAURI_TAVERN_BRIDGE__?.invoke) {
      return window.__TAURI_TAVERN_BRIDGE__.invoke;
    }
    if (typeof window !== "undefined" && window.__TAURI__) {
      return window.__TAURI__.core.invoke;
    }
    if (typeof window !== "undefined" && window.__TAURI_INTERNALS__) {
      return window.__TAURI_INTERNALS__.invoke;
    }
    throw new Error(
      "Tauri IPC not available. Make sure this page is loaded inside a TauriTavern webview."
    );
  }

  function getListen() {
    if (typeof window !== "undefined" && window.__TAURI_TAVERN_BRIDGE__?.listen) {
      return window.__TAURI_TAVERN_BRIDGE__.listen;
    }
    if (typeof window !== "undefined" && window.__TAURI__?.event?.listen) {
      return window.__TAURI__.event.listen;
    }
    if (typeof window !== "undefined" && window.__TAURI_INTERNALS__?.event?.listen) {
      return window.__TAURI_INTERNALS__.event.listen;
    }
    throw new Error("Tauri events not available.");
  }

  function getCartridgeIdFromUrl() {
    const href = window.location.href;
    const match = href.match(/cartridge\/([^/]+)/);
    return match ? match[1] : null;
  }

  /**
   * @typedef {Object} ChatInfo
   * @property {string} id
   * @property {string} cartridge_id
   * @property {string} title
   * @property {string} created_at
   * @property {string} updated_at
   */

  /**
   * @typedef {Object} ChatMessage
   * @property {string} role
   * @property {string} content
   */

  /**
   * @typedef {Object} PresetConfig
   * @property {string} system_prompt
   * @property {string} [model]
   * @property {number} [temperature]
   * @property {number} [max_tokens]
   * @property {string} [provider]
   * @property {string} [provider_url]
   */

  window.TavernSDK = {
    /** Get the current cartridge ID */
    getCartridgeId() {
      return getCartridgeIdFromUrl();
    },

    /**
     * Get the preset configuration for this cartridge.
     * @returns {Promise<PresetConfig>}
     */
    async getPreset() {
      const invoke = getInvoke();
      const cartridgeId = getCartridgeIdFromUrl();
      return invoke("get_preset", { cartridgeId });
    },

    /**
     * Create a new chat session.
     * @param {string} [title] - Optional chat title.
     * @returns {Promise<ChatInfo>}
     */
    async createChat(title) {
      const invoke = getInvoke();
      const cartridgeId = getCartridgeIdFromUrl();
      return invoke("create_chat", {
        cartridgeId,
        title: title || null,
      });
    },

    /**
     * List all chat sessions for this cartridge.
     * @returns {Promise<ChatInfo[]>}
     */
    async listChats() {
      const invoke = getInvoke();
      const cartridgeId = getCartridgeIdFromUrl();
      return invoke("list_chats", { cartridgeId });
    },

    /**
     * Get all messages in a chat.
     * @param {string} chatId
     * @returns {Promise<ChatMessage[]>}
     */
    async getMessages(chatId) {
      const invoke = getInvoke();
      const cartridgeId = getCartridgeIdFromUrl();
      return invoke("get_messages", { cartridgeId, chatId });
    },

    /**
     * Send a message and receive the full response.
     * @param {string} chatId
     * @param {string} message - The user's message.
     * @param {function(string):void} onChunk - Called with the full response text.
     * @param {function():void} [onDone] - Called when complete.
     * @param {function(string):void} [onError] - Called on error.
     * @returns {Promise<void>}
     */
    async sendMessage(chatId, message, onChunk, onDone, onError) {
      const invoke = getInvoke();
      const cartridgeId = getCartridgeIdFromUrl();

      try {
        const response = await invoke("send_chat", {
          cartridgeId,
          chatId,
          message,
        });
        if (onChunk) onChunk(response);
        if (onDone) onDone();
      } catch (e) {
        if (onError) onError(String(e));
      }
    },

    /**
     * Execute a SillyTavern-style slash command/STScript snippet.
     * The command is handled locally and does not call the model unless a future
     * command explicitly does so.
     * @param {string} chatId
     * @param {string} command - e.g. "/setvar mood=happy" or "/getvar mood | /echo {{pipe}}"
     * @returns {Promise<string>} Command output.
     */
    async executeCommand(chatId, command) {
      const invoke = getInvoke();
      const cartridgeId = getCartridgeIdFromUrl();
      return invoke("execute_st_command", {
        cartridgeId,
        chatId,
        command,
      });
    },

    /**
     * Run the headless prompt pipeline without sending a network request.
     * @param {string|null} chatId - Existing chat ID, or null for an empty dry run.
     * @param {string} message - Simulated user message.
     * @returns {Promise<Object>} Prompt X-Ray metadata and final payload.
     */
    async dryRunPromptPipeline(chatId, message) {
      const invoke = getInvoke();
      const cartridgeId = getCartridgeIdFromUrl();
      return invoke("dry_run_prompt_pipeline", {
        cartridgeId,
        chatId: chatId || null,
        message,
      });
    },

    /**
     * Delete a chat and all its messages.
     * @param {string} chatId
     * @returns {Promise<void>}
     */
    async deleteChat(chatId) {
      const invoke = getInvoke();
      const cartridgeId = getCartridgeIdFromUrl();
      return invoke("delete_chat", { cartridgeId, chatId });
    },

    /**
     * Set a chat-scoped variable for ST-style macros and scripts.
     * @param {string} chatId
     * @param {string} name
     * @param {string} value
     * @returns {Promise<void>}
     */
    async setChatVariable(chatId, name, value) {
      const invoke = getInvoke();
      const cartridgeId = getCartridgeIdFromUrl();
      return invoke("set_chat_variable", {
        cartridgeId,
        chatId,
        name,
        value: String(value ?? ""),
      });
    },

    /**
     * Read a chat-scoped variable.
     * @param {string} chatId
     * @param {string} name
     * @returns {Promise<string|null>}
     */
    async getChatVariable(chatId, name) {
      const invoke = getInvoke();
      const cartridgeId = getCartridgeIdFromUrl();
      return invoke("get_chat_variable", { cartridgeId, chatId, name });
    },

    /**
     * List all chat-scoped variables.
     * @param {string} chatId
     * @returns {Promise<Array<{chat_id:string,name:string,value:string,updated_at:string}>>}
     */
    async listChatVariables(chatId) {
      const invoke = getInvoke();
      const cartridgeId = getCartridgeIdFromUrl();
      return invoke("list_chat_variables", { cartridgeId, chatId });
    },

    /**
     * Delete a chat-scoped variable.
     * @param {string} chatId
     * @param {string} name
     * @returns {Promise<void>}
     */
    async deleteChatVariable(chatId, name) {
      const invoke = getInvoke();
      const cartridgeId = getCartridgeIdFromUrl();
      return invoke("delete_chat_variable", { cartridgeId, chatId, name });
    },

    /**
     * Load an asset file from the cartridge's assets directory.
     * Returns a base64 data URL string.
     * @param {string} assetPath - Path relative to cartridge root (e.g. "assets/avatar.png")
     * @returns {Promise<string>} - Base64-encoded data URL.
     */
    async loadAsset(assetPath) {
      const invoke = getInvoke();
      const cartridgeId = getCartridgeIdFromUrl();
      const result = await invoke("load_asset", {
        cartridgeId,
        path: assetPath,
      });
      // Backend now returns a full data: URI — use directly
      return result;
    },

    /**
     * Get a URL for an asset that can be used in <img>, <audio>, etc.
     * @param {string} assetPath - Path relative to cartridge root.
     * @returns {string} - A tavern:// URL for the asset.
     */
    getAssetUrl(assetPath) {
      const cartridgeId = getCartridgeIdFromUrl();
      if (!cartridgeId) return assetPath;
      return `tavern://localhost/cartridge/${cartridgeId}/${assetPath}`;
    },

    /**
     * Match world info entries against a message.
     * @param {string} message - The message to match against.
     * @returns {Promise<Array<{keys: string[], content: string}>>}
     */
    async matchWorldInfo(message) {
      const invoke = getInvoke();
      const cartridgeId = getCartridgeIdFromUrl();
      return invoke("match_world_info", { cartridgeId, message });
    },

    /**
     * Play background music from the cartridge's assets.
     * @param {string} assetPath - Path to audio file (e.g. "assets/bgm.mp3").
     * @param {Object} [options] - { loop: boolean, volume: number (0-1) }.
     * @returns {HTMLAudioElement}
     */
    playBgm(assetPath, options) {
      const url = this.getAssetUrl(assetPath);
      const audio = new Audio(url);
      audio.loop = options?.loop ?? true;
      audio.volume = options?.volume ?? 0.5;
      audio.play().catch(console.warn);
      this._bgm = audio;
      return audio;
    },

    /** Stop currently playing background music. */
    stopBgm() {
      if (this._bgm) {
        this._bgm.pause();
        this._bgm = null;
      }
    },
  };
})();

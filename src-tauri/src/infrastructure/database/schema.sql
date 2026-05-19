CREATE TABLE IF NOT EXISTS cartridges (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    author TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    version TEXT NOT NULL DEFAULT '1.0.0',
    entry_file TEXT NOT NULL DEFAULT 'ui/index.html',
    cover_image TEXT NOT NULL DEFAULT '',
    installed_at TEXT NOT NULL,
    directory_path TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS chats (
    id TEXT PRIMARY KEY,
    cartridge_id TEXT NOT NULL,
    title TEXT NOT NULL DEFAULT 'New Chat',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (cartridge_id) REFERENCES cartridges(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS messages (
    id TEXT PRIMARY KEY,
    chat_id TEXT NOT NULL,
    role TEXT NOT NULL CHECK(role IN ('user', 'assistant', 'system')),
    content TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY (chat_id) REFERENCES chats(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS message_swipes (
    id TEXT PRIMARY KEY,
    message_id TEXT NOT NULL,
    swipe_index INTEGER NOT NULL,
    content TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(message_id, swipe_index),
    FOREIGN KEY (message_id) REFERENCES messages(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_message_swipes_message
    ON message_swipes(message_id, swipe_index);

CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS rag_memories (
    id TEXT PRIMARY KEY,
    cartridge_id TEXT NOT NULL,
    chat_id TEXT,
    message_id TEXT,
    source_type TEXT NOT NULL CHECK(source_type IN ('turn', 'world_info')),
    source_id TEXT,
    content TEXT NOT NULL,
    vector_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY (cartridge_id) REFERENCES cartridges(id) ON DELETE CASCADE,
    FOREIGN KEY (chat_id) REFERENCES chats(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_rag_memories_cartridge_source
    ON rag_memories(cartridge_id, source_type);

CREATE INDEX IF NOT EXISTS idx_rag_memories_chat
    ON rag_memories(chat_id);

CREATE TABLE IF NOT EXISTS chat_variables (
    chat_id TEXT NOT NULL,
    name TEXT NOT NULL,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (chat_id, name),
    FOREIGN KEY (chat_id) REFERENCES chats(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS profiles (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    model TEXT NOT NULL,
    api_url TEXT,
    created_at TEXT NOT NULL
);

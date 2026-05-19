use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct CartridgeRow {
    pub id: String,
    pub name: String,
    pub author: String,
    pub description: String,
    pub version: String,
    pub entry_file: String,
    pub cover_image: String,
    pub installed_at: String,
    pub directory_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ChatRow {
    pub id: String,
    pub cartridge_id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct MessageRow {
    pub id: String,
    pub chat_id: String,
    pub role: String,
    pub content: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct MessageSwipeRow {
    pub id: String,
    pub message_id: String,
    pub swipe_index: i64,
    pub content: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct RagMemoryRow {
    pub id: String,
    pub cartridge_id: String,
    pub chat_id: Option<String>,
    pub message_id: Option<String>,
    pub source_type: String,
    pub source_id: Option<String>,
    pub content: String,
    pub vector_json: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ChatVariableRow {
    pub chat_id: String,
    pub name: String,
    pub value: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ProfileRow {
    pub id: String,
    pub name: String,
    pub provider_id: String,
    pub model: String,
    pub api_url: Option<String>,
    pub created_at: String,
}

#[derive(Clone)]
pub struct SqliteRepo {
    pool: SqlitePool,
}

impl SqliteRepo {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    // ── Cartridges ──────────────────────────────────────────────

    pub async fn insert_cartridge(&self, c: &CartridgeRow) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO cartridges (id, name, author, description, version, entry_file, cover_image, installed_at, directory_path)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&c.id)
        .bind(&c.name)
        .bind(&c.author)
        .bind(&c.description)
        .bind(&c.version)
        .bind(&c.entry_file)
        .bind(&c.cover_image)
        .bind(&c.installed_at)
        .bind(&c.directory_path)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_all_cartridges(&self) -> Result<Vec<CartridgeRow>, sqlx::Error> {
        sqlx::query_as::<_, CartridgeRow>(
            "SELECT id, name, author, description, version, entry_file, cover_image, installed_at, directory_path FROM cartridges ORDER BY installed_at DESC",
        )
        .fetch_all(&self.pool)
        .await
    }

    pub async fn get_cartridge(&self, id: &str) -> Result<Option<CartridgeRow>, sqlx::Error> {
        sqlx::query_as::<_, CartridgeRow>(
            "SELECT id, name, author, description, version, entry_file, cover_image, installed_at, directory_path FROM cartridges WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn delete_cartridge(&self, id: &str) -> Result<(), sqlx::Error> {
        // Manual cascade: delete messages → chats → cartridge
        sqlx::query("DELETE FROM rag_memories WHERE cartridge_id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        sqlx::query(
            "DELETE FROM chat_variables WHERE chat_id IN (SELECT id FROM chats WHERE cartridge_id = ?)",
        )
        .bind(id)
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "DELETE FROM message_swipes WHERE message_id IN (
                SELECT messages.id FROM messages
                INNER JOIN chats ON chats.id = messages.chat_id
                WHERE chats.cartridge_id = ?
            )",
        )
        .bind(id)
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "DELETE FROM messages WHERE chat_id IN (SELECT id FROM chats WHERE cartridge_id = ?)",
        )
        .bind(id)
        .execute(&self.pool)
        .await?;
        sqlx::query("DELETE FROM chats WHERE cartridge_id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM cartridges WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ── Chats ───────────────────────────────────────────────────

    pub async fn create_chat(&self, c: &ChatRow) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO chats (id, cartridge_id, title, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&c.id)
        .bind(&c.cartridge_id)
        .bind(&c.title)
        .bind(&c.created_at)
        .bind(&c.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_chats(&self, cartridge_id: &str) -> Result<Vec<ChatRow>, sqlx::Error> {
        sqlx::query_as::<_, ChatRow>(
            "SELECT id, cartridge_id, title, created_at, updated_at FROM chats WHERE cartridge_id = ? ORDER BY updated_at DESC",
        )
        .bind(cartridge_id)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn get_chat(&self, id: &str) -> Result<Option<ChatRow>, sqlx::Error> {
        sqlx::query_as::<_, ChatRow>(
            "SELECT id, cartridge_id, title, created_at, updated_at FROM chats WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn delete_chat(&self, id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM rag_memories WHERE chat_id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM chat_variables WHERE chat_id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM message_swipes WHERE message_id IN (SELECT id FROM messages WHERE chat_id = ?)")
            .bind(id)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM messages WHERE chat_id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM chats WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn touch_chat(&self, id: &str, updated_at: &str) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE chats SET updated_at = ? WHERE id = ?")
            .bind(updated_at)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ── Messages ────────────────────────────────────────────────

    pub async fn insert_message(&self, m: &MessageRow) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO messages (id, chat_id, role, content, created_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&m.id)
        .bind(&m.chat_id)
        .bind(&m.role)
        .bind(&m.content)
        .bind(&m.created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_message_content(&self, id: &str, content: &str) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE messages SET content = ? WHERE id = ?")
            .bind(content)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_messages_by_chat(
        &self,
        chat_id: &str,
        limit: i64,
    ) -> Result<Vec<MessageRow>, sqlx::Error> {
        sqlx::query_as::<_, MessageRow>(
            "SELECT id, chat_id, role, content, created_at FROM messages WHERE chat_id = ? ORDER BY created_at ASC LIMIT ?",
        )
        .bind(chat_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn get_recent_messages_by_chat(
        &self,
        chat_id: &str,
        limit: i64,
    ) -> Result<Vec<MessageRow>, sqlx::Error> {
        let mut rows = sqlx::query_as::<_, MessageRow>(
            "SELECT id, chat_id, role, content, created_at FROM messages WHERE chat_id = ? ORDER BY created_at DESC LIMIT ?",
        )
        .bind(chat_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.reverse();
        Ok(rows)
    }

    pub async fn get_last_message_by_role(
        &self,
        chat_id: &str,
        role: &str,
    ) -> Result<Option<MessageRow>, sqlx::Error> {
        sqlx::query_as::<_, MessageRow>(
            "SELECT id, chat_id, role, content, created_at FROM messages
             WHERE chat_id = ? AND role = ? ORDER BY created_at DESC LIMIT 1",
        )
        .bind(chat_id)
        .bind(role)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn delete_message(&self, id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM rag_memories WHERE message_id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM message_swipes WHERE message_id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM messages WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn insert_message_swipe(&self, swipe: &MessageSwipeRow) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT OR REPLACE INTO message_swipes (id, message_id, swipe_index, content, created_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&swipe.id)
        .bind(&swipe.message_id)
        .bind(swipe.swipe_index)
        .bind(&swipe.content)
        .bind(&swipe.created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_message_swipes(
        &self,
        message_id: &str,
    ) -> Result<Vec<MessageSwipeRow>, sqlx::Error> {
        sqlx::query_as::<_, MessageSwipeRow>(
            "SELECT id, message_id, swipe_index, content, created_at
             FROM message_swipes WHERE message_id = ? ORDER BY swipe_index ASC",
        )
        .bind(message_id)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn next_swipe_index(&self, message_id: &str) -> Result<i64, sqlx::Error> {
        let row: (Option<i64>,) =
            sqlx::query_as("SELECT MAX(swipe_index) FROM message_swipes WHERE message_id = ?")
                .bind(message_id)
                .fetch_one(&self.pool)
                .await?;
        Ok(row.0.map(|idx| idx + 1).unwrap_or(0))
    }

    // ── RAG Memories ────────────────────────────────────────────

    pub async fn insert_rag_memory(&self, memory: &RagMemoryRow) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT OR REPLACE INTO rag_memories
             (id, cartridge_id, chat_id, message_id, source_type, source_id, content, vector_json, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&memory.id)
        .bind(&memory.cartridge_id)
        .bind(&memory.chat_id)
        .bind(&memory.message_id)
        .bind(&memory.source_type)
        .bind(&memory.source_id)
        .bind(&memory.content)
        .bind(&memory.vector_json)
        .bind(&memory.created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_rag_memories(
        &self,
        cartridge_id: &str,
        source_type: Option<&str>,
    ) -> Result<Vec<RagMemoryRow>, sqlx::Error> {
        if let Some(source_type) = source_type {
            sqlx::query_as::<_, RagMemoryRow>(
                "SELECT id, cartridge_id, chat_id, message_id, source_type, source_id, content, vector_json, created_at
                 FROM rag_memories WHERE cartridge_id = ? AND source_type = ?",
            )
            .bind(cartridge_id)
            .bind(source_type)
            .fetch_all(&self.pool)
            .await
        } else {
            sqlx::query_as::<_, RagMemoryRow>(
                "SELECT id, cartridge_id, chat_id, message_id, source_type, source_id, content, vector_json, created_at
                 FROM rag_memories WHERE cartridge_id = ?",
            )
            .bind(cartridge_id)
            .fetch_all(&self.pool)
            .await
        }
    }

    pub async fn delete_rag_memories_by_source_type(
        &self,
        cartridge_id: &str,
        source_type: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM rag_memories WHERE cartridge_id = ? AND source_type = ?")
            .bind(cartridge_id)
            .bind(source_type)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ── Chat Variables ─────────────────────────────────────────

    pub async fn set_chat_variable(
        &self,
        chat_id: &str,
        name: &str,
        value: &str,
        updated_at: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT OR REPLACE INTO chat_variables (chat_id, name, value, updated_at)
             VALUES (?, ?, ?, ?)",
        )
        .bind(chat_id)
        .bind(name)
        .bind(value)
        .bind(updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_chat_variable(
        &self,
        chat_id: &str,
        name: &str,
    ) -> Result<Option<String>, sqlx::Error> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT value FROM chat_variables WHERE chat_id = ? AND name = ?")
                .bind(chat_id)
                .bind(name)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|r| r.0))
    }

    pub async fn list_chat_variables(
        &self,
        chat_id: &str,
    ) -> Result<Vec<ChatVariableRow>, sqlx::Error> {
        sqlx::query_as::<_, ChatVariableRow>(
            "SELECT chat_id, name, value, updated_at FROM chat_variables WHERE chat_id = ? ORDER BY name ASC",
        )
        .bind(chat_id)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn delete_chat_variable(&self, chat_id: &str, name: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM chat_variables WHERE chat_id = ? AND name = ?")
            .bind(chat_id)
            .bind(name)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ── Profiles ────────────────────────────────────────────────

    pub async fn insert_profile(&self, p: &ProfileRow) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT OR REPLACE INTO profiles (id, name, provider_id, model, api_url, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&p.id)
        .bind(&p.name)
        .bind(&p.provider_id)
        .bind(&p.model)
        .bind(&p.api_url)
        .bind(&p.created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_profiles(&self) -> Result<Vec<ProfileRow>, sqlx::Error> {
        sqlx::query_as::<_, ProfileRow>(
            "SELECT id, name, provider_id, model, api_url, created_at FROM profiles ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await
    }

    pub async fn get_profile(&self, id: &str) -> Result<Option<ProfileRow>, sqlx::Error> {
        sqlx::query_as::<_, ProfileRow>(
            "SELECT id, name, provider_id, model, api_url, created_at FROM profiles WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn delete_profile(&self, id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM profiles WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ── Settings key-value ───────────────────────────────────

    pub async fn set_setting(&self, key: &str, value: &str) -> Result<(), sqlx::Error> {
        sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES (?, ?)")
            .bind(key)
            .bind(value)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_setting(&self, key: &str) -> Result<Option<String>, sqlx::Error> {
        let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| r.0))
    }
}

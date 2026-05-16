use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRow {
    pub id: String,
    pub cartridge_id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageRow {
    pub id: String,
    pub chat_id: String,
    pub role: String,
    pub content: String,
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

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
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
        sqlx::query("DELETE FROM chats WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_chat_title(
        &self,
        id: &str,
        title: &str,
        updated_at: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE chats SET title = ?, updated_at = ? WHERE id = ?")
            .bind(title)
            .bind(updated_at)
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

    pub async fn delete_messages_by_chat(&self, chat_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM messages WHERE chat_id = ?")
            .bind(chat_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ── Settings ────────────────────────────────────────────────

    pub async fn get_setting(&self, key: &str) -> Result<Option<String>, sqlx::Error> {
        sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(&self.pool)
            .await
    }

    pub async fn set_setting(&self, key: &str, value: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_setting(&self, key: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM settings WHERE key = ?")
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_all_api_keys(&self) -> Result<Vec<(String, String)>, sqlx::Error> {
        sqlx::query_as::<_, (String, String)>(
            "SELECT key, value FROM settings WHERE key LIKE 'api_key_%'",
        )
        .fetch_all(&self.pool)
        .await
    }
}

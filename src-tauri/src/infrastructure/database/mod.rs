pub mod sqlite_repo;
pub use sqlite_repo::*;

pub async fn run_migrations(pool: &sqlx::SqlitePool) -> Result<(), sqlx::Error> {
    let schema = include_str!("schema.sql");
    for statement in schema.split(';') {
        let stmt = statement.trim();
        if stmt.is_empty() {
            continue;
        }
        sqlx::query(stmt).execute(pool).await?;
    }

    // Clean up orphaned rows from prior versions that ran without FK enforcement
    sqlx::query(
        "DELETE FROM messages WHERE chat_id NOT IN (SELECT id FROM chats)",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "DELETE FROM chats WHERE cartridge_id NOT IN (SELECT id FROM cartridges)",
    )
    .execute(pool)
    .await?;

    Ok(())
}

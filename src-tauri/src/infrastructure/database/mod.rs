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
    Ok(())
}

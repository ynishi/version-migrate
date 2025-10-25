use serde::{Deserialize, Serialize};
use version_migrate::{async_trait, AsyncIntoDomain, AsyncMigratesTo, MigrationError, Versioned};

// Version 1.0.0 of Task
#[derive(Serialize, Deserialize, Versioned, Clone)]
#[versioned(version = "1.0.0")]
struct TaskV1_0_0 {
    id: String,
    title: String,
}

// Version 1.1.0 of Task (added description field)
#[derive(Serialize, Deserialize, Versioned, Clone)]
#[versioned(version = "1.1.0")]
struct TaskV1_1_0 {
    id: String,
    title: String,
    description: Option<String>,
}

// Domain model (clean, version-agnostic)
#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct TaskEntity {
    id: String,
    title: String,
    description: Option<String>,
    enriched_data: Option<String>,
}

// Async migration from V1.0.0 to V1.1.0
#[async_trait]
impl AsyncMigratesTo<TaskV1_1_0> for TaskV1_0_0 {
    async fn migrate(self) -> Result<TaskV1_1_0, MigrationError> {
        // Simulate async I/O operation (e.g., fetching from database)
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        Ok(TaskV1_1_0 {
            id: self.id,
            title: self.title,
            description: None, // Default value for new field
        })
    }
}

// Async conversion from latest version to domain model
#[async_trait]
impl AsyncIntoDomain<TaskEntity> for TaskV1_1_0 {
    async fn into_domain(self) -> Result<TaskEntity, MigrationError> {
        // Simulate async I/O operation (e.g., API call to enrich data)
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        Ok(TaskEntity {
            id: self.id.clone(),
            title: self.title,
            description: self.description,
            enriched_data: Some(format!("enriched-{}", self.id)),
        })
    }
}

#[tokio::test]
async fn test_async_migration() {
    let task_v1 = TaskV1_0_0 {
        id: "task-1".to_string(),
        title: "Async Task".to_string(),
    };

    // Perform async migration
    let task_v1_1 = task_v1.migrate().await.expect("Migration failed");

    assert_eq!(task_v1_1.id, "task-1");
    assert_eq!(task_v1_1.title, "Async Task");
    assert_eq!(task_v1_1.description, None);
}

#[tokio::test]
async fn test_async_into_domain() {
    let task_v1_1 = TaskV1_1_0 {
        id: "task-2".to_string(),
        title: "Domain Task".to_string(),
        description: Some("A description".to_string()),
    };

    // Perform async conversion to domain
    let domain = task_v1_1.into_domain().await.expect("Conversion failed");

    assert_eq!(domain.id, "task-2");
    assert_eq!(domain.title, "Domain Task");
    assert_eq!(domain.description, Some("A description".to_string()));
    assert_eq!(domain.enriched_data, Some("enriched-task-2".to_string()));
}

#[tokio::test]
async fn test_async_full_migration_chain() {
    let task_v1 = TaskV1_0_0 {
        id: "task-3".to_string(),
        title: "Full Chain".to_string(),
    };

    // Full migration chain: V1 -> V1.1 -> Domain
    let task_v1_1 = task_v1.migrate().await.expect("Migration failed");
    let domain = task_v1_1.into_domain().await.expect("Conversion failed");

    assert_eq!(domain.id, "task-3");
    assert_eq!(domain.title, "Full Chain");
    assert_eq!(domain.description, None);
    assert_eq!(domain.enriched_data, Some("enriched-task-3".to_string()));
}

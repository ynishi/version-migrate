use serde::{Deserialize, Serialize};
use version_migrate::{migrator, IntoDomain, MigratesTo, Versioned};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct TaskV1 {
    id: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct TaskV2 {
    id: String,
    title: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct TaskV3 {
    id: String,
    title: String,
    description: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct TaskEntity {
    id: String,
    title: String,
    description: Option<String>,
}

// Implement Versioned for all types
impl Versioned for TaskV1 {
    const VERSION: &'static str = "1.0.0";
}

impl Versioned for TaskV2 {
    const VERSION: &'static str = "1.1.0";
}

impl Versioned for TaskV3 {
    const VERSION: &'static str = "1.2.0";
}

// Implement migrations
impl MigratesTo<TaskV2> for TaskV1 {
    fn migrate(self) -> TaskV2 {
        TaskV2 {
            id: self.id,
            title: "Untitled".to_string(),
        }
    }
}

impl MigratesTo<TaskV3> for TaskV2 {
    fn migrate(self) -> TaskV3 {
        TaskV3 {
            id: self.id,
            title: self.title,
            description: None,
        }
    }
}

impl IntoDomain<TaskEntity> for TaskV3 {
    fn into_domain(self) -> TaskEntity {
        TaskEntity {
            id: self.id,
            title: self.title,
            description: self.description,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_migrator_single_entity() {
        // migrator! should return a ready-to-use Migrator
        let migrator = migrator!("task" => [TaskV1, TaskV2, TaskV3, TaskEntity]).unwrap();

        // Should be able to use it immediately
        let json = r#"{"version":"1.0.0","data":{"id":"123"}}"#;
        let result: TaskEntity = migrator.load("task", json).unwrap();

        assert_eq!(result.id, "123");
        assert_eq!(result.title, "Untitled");
        assert_eq!(result.description, None);
    }

    #[test]
    fn test_migrator_multiple_entities() -> Result<(), Box<dyn std::error::Error>> {
        // Define another entity for testing
        #[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
        struct UserV1 {
            name: String,
        }

        #[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
        struct UserV2 {
            name: String,
            email: String,
        }

        #[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
        struct UserEntity {
            name: String,
            email: String,
        }

        impl Versioned for UserV1 {
            const VERSION: &'static str = "1.0.0";
        }

        impl Versioned for UserV2 {
            const VERSION: &'static str = "2.0.0";
        }

        impl MigratesTo<UserV2> for UserV1 {
            fn migrate(self) -> UserV2 {
                UserV2 {
                    name: self.name,
                    email: "unknown@example.com".to_string(),
                }
            }
        }

        impl IntoDomain<UserEntity> for UserV2 {
            fn into_domain(self) -> UserEntity {
                UserEntity {
                    name: self.name,
                    email: self.email,
                }
            }
        }

        // Register multiple entities at once
        let migrator = migrator!(
            "task" => [TaskV1, TaskV2, TaskV3, TaskEntity],
            "user" => [UserV1, UserV2, UserEntity]
        )?;

        // Test task migration
        let task_json = r#"{"version":"1.0.0","data":{"id":"123"}}"#;
        let task: TaskEntity = migrator.load("task", task_json)?;
        assert_eq!(task.id, "123");

        // Test user migration
        let user_json = r#"{"version":"1.0.0","data":{"name":"Alice"}}"#;
        let user: UserEntity = migrator.load("user", user_json)?;
        assert_eq!(user.name, "Alice");
        assert_eq!(user.email, "unknown@example.com");

        Ok(())
    }

    #[test]
    fn test_migrator_with_domain_entity() {
        let migrator = migrator!("task" => [TaskV1, TaskV2, TaskV3, TaskEntity]).unwrap();

        let json = r#"{"version":"1.0.0","data":{"id":"456"}}"#;
        let entity: TaskEntity = migrator.load("task", json).unwrap();

        assert_eq!(entity.id, "456");
        assert_eq!(entity.title, "Untitled");
        assert_eq!(entity.description, None);
    }

    #[test]
    fn test_migrator_preserves_latest_version() {
        let migrator = migrator!("task" => [TaskV1, TaskV2, TaskV3, TaskEntity]).unwrap();

        // Latest version should return correct version string
        assert_eq!(migrator.get_latest_version("task"), Some("1.2.0"));
    }
}

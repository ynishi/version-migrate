use serde::{Deserialize, Serialize};
use version_migrate::{migrator, FromDomain, IntoDomain, MigratesTo, Versioned};

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

impl FromDomain<TaskEntity> for TaskV3 {
    fn from_domain(entity: TaskEntity) -> Self {
        TaskV3 {
            id: entity.id,
            title: entity.title,
            description: entity.description,
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

    #[test]
    fn test_migrator_single_entity_with_custom_keys() {
        // Test single entity with custom keys
        let migrator = migrator!(
            "task" => [TaskV1, TaskV2, TaskV3, TaskEntity],
            version_key = "v",
            data_key = "d"
        )
        .unwrap();

        // Use custom keys in JSON
        let json = r#"{"v":"1.0.0","d":{"id":"789"}}"#;
        let result: TaskEntity = migrator.load("task", json).unwrap();

        assert_eq!(result.id, "789");
        assert_eq!(result.title, "Untitled");
        assert_eq!(result.description, None);
    }

    #[test]
    fn test_migrator_multiple_entities_with_custom_keys() -> Result<(), Box<dyn std::error::Error>>
    {
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

        // Register multiple entities at once with custom keys
        // Note: @keys prefix is required to disambiguate from single entity syntax
        let migrator = migrator!(
            @keys version_key = "v", data_key = "d";
            "task" => [TaskV1, TaskV2, TaskV3, TaskEntity],
            "user" => [UserV1, UserV2, UserEntity]
        )?;

        // Test task migration with custom keys
        let task_json = r#"{"v":"1.0.0","d":{"id":"123"}}"#;
        let task: TaskEntity = migrator.load("task", task_json)?;
        assert_eq!(task.id, "123");

        // Test user migration with custom keys
        let user_json = r#"{"v":"1.0.0","d":{"name":"Bob"}}"#;
        let user: UserEntity = migrator.load("user", user_json)?;
        assert_eq!(user.name, "Bob");
        assert_eq!(user.email, "unknown@example.com");

        Ok(())
    }

    #[test]
    fn test_migrator_with_save_single_entity() {
        let migrator =
            migrator!("task" => [TaskV1, TaskV2, TaskV3, TaskEntity], save = true).unwrap();

        // Test load
        let json = r#"{"version":"1.0.0","data":{"id":"save-test"}}"#;
        let entity: TaskEntity = migrator.load("task", json).unwrap();
        assert_eq!(entity.id, "save-test");
        assert_eq!(entity.title, "Untitled");

        // Test save
        let saved = migrator.save_domain("task", entity).unwrap();
        assert!(saved.contains("\"version\":\"1.2.0\""));
        assert!(saved.contains("\"id\":\"save-test\""));
    }

    #[test]
    fn test_migrator_with_save_multiple_entities() -> Result<(), Box<dyn std::error::Error>> {
        // Define UserV1, UserV2, UserEntity
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

        impl FromDomain<UserEntity> for UserV2 {
            fn from_domain(entity: UserEntity) -> Self {
                UserV2 {
                    name: entity.name,
                    email: entity.email,
                }
            }
        }

        let migrator = migrator!(
            @save;
            "task" => [TaskV1, TaskV2, TaskV3, TaskEntity],
            "user" => [UserV1, UserV2, UserEntity]
        )?;

        // Test task save
        let task_entity = TaskEntity {
            id: "multi-save-task".to_string(),
            title: "Test Task".to_string(),
            description: Some("Description".to_string()),
        };
        let task_saved = migrator.save_domain("task", task_entity)?;
        assert!(task_saved.contains("\"version\":\"1.2.0\""));

        // Test user save
        let user_entity = UserEntity {
            name: "Alice".to_string(),
            email: "alice@example.com".to_string(),
        };
        let user_saved = migrator.save_domain("user", user_entity)?;
        assert!(user_saved.contains("\"version\":\"2.0.0\""));
        assert!(user_saved.contains("\"name\":\"Alice\""));

        Ok(())
    }

    #[test]
    fn test_migrator_with_save_and_custom_keys() {
        let migrator = migrator!(
            "task" => [TaskV1, TaskV2, TaskV3, TaskEntity],
            version_key = "v",
            data_key = "d",
            save = true
        )
        .unwrap();

        // Test load with custom keys
        let json = r#"{"v":"1.0.0","d":{"id":"custom-key-test"}}"#;
        let entity: TaskEntity = migrator.load("task", json).unwrap();
        assert_eq!(entity.id, "custom-key-test");

        // Test save with custom keys
        let saved = migrator.save_domain("task", entity).unwrap();
        assert!(saved.contains("\"v\":\"1.2.0\""));
        assert!(saved.contains("\"d\":{"));
    }
}

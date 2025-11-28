use serde::{Deserialize, Serialize};
use version_migrate::{migrator, IntoDomain, MigratesTo, Migrator, Versioned};

// Define test versions V1 through V10
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
struct TaskV4 {
    id: String,
    title: String,
    description: Option<String>,
    status: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct TaskV5 {
    id: String,
    title: String,
    description: Option<String>,
    status: String,
    priority: i32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct TaskV6 {
    id: String,
    title: String,
    description: Option<String>,
    status: String,
    priority: i32,
    tags: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct TaskV7 {
    id: String,
    title: String,
    description: Option<String>,
    status: String,
    priority: i32,
    tags: Vec<String>,
    due_date: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct TaskV8 {
    id: String,
    title: String,
    description: Option<String>,
    status: String,
    priority: i32,
    tags: Vec<String>,
    due_date: Option<String>,
    assignee: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct TaskV9 {
    id: String,
    title: String,
    description: Option<String>,
    status: String,
    priority: i32,
    tags: Vec<String>,
    due_date: Option<String>,
    assignee: Option<String>,
    created_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct TaskV10 {
    id: String,
    title: String,
    description: Option<String>,
    status: String,
    priority: i32,
    tags: Vec<String>,
    due_date: Option<String>,
    assignee: Option<String>,
    created_at: String,
    updated_at: String,
}

// Domain model
#[derive(Debug, PartialEq)]
struct TaskEntity {
    id: String,
    title: String,
    description: Option<String>,
    status: String,
    priority: i32,
    tags: Vec<String>,
    due_date: Option<String>,
    assignee: Option<String>,
    created_at: String,
    updated_at: String,
}

// Implement Versioned for all versions
impl Versioned for TaskV1 {
    const VERSION: &'static str = "1.0.0";
}

impl Versioned for TaskV2 {
    const VERSION: &'static str = "2.0.0";
}

impl Versioned for TaskV3 {
    const VERSION: &'static str = "3.0.0";
}

impl Versioned for TaskV4 {
    const VERSION: &'static str = "4.0.0";
}

impl Versioned for TaskV5 {
    const VERSION: &'static str = "5.0.0";
}

impl Versioned for TaskV6 {
    const VERSION: &'static str = "6.0.0";
}

impl Versioned for TaskV7 {
    const VERSION: &'static str = "7.0.0";
}

impl Versioned for TaskV8 {
    const VERSION: &'static str = "8.0.0";
}

impl Versioned for TaskV9 {
    const VERSION: &'static str = "9.0.0";
}

impl Versioned for TaskV10 {
    const VERSION: &'static str = "10.0.0";
}

// Implement migration chain
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

impl MigratesTo<TaskV4> for TaskV3 {
    fn migrate(self) -> TaskV4 {
        TaskV4 {
            id: self.id,
            title: self.title,
            description: self.description,
            status: "open".to_string(),
        }
    }
}

impl MigratesTo<TaskV5> for TaskV4 {
    fn migrate(self) -> TaskV5 {
        TaskV5 {
            id: self.id,
            title: self.title,
            description: self.description,
            status: self.status,
            priority: 0,
        }
    }
}

impl MigratesTo<TaskV6> for TaskV5 {
    fn migrate(self) -> TaskV6 {
        TaskV6 {
            id: self.id,
            title: self.title,
            description: self.description,
            status: self.status,
            priority: self.priority,
            tags: vec![],
        }
    }
}

impl MigratesTo<TaskV7> for TaskV6 {
    fn migrate(self) -> TaskV7 {
        TaskV7 {
            id: self.id,
            title: self.title,
            description: self.description,
            status: self.status,
            priority: self.priority,
            tags: self.tags,
            due_date: None,
        }
    }
}

impl MigratesTo<TaskV8> for TaskV7 {
    fn migrate(self) -> TaskV8 {
        TaskV8 {
            id: self.id,
            title: self.title,
            description: self.description,
            status: self.status,
            priority: self.priority,
            tags: self.tags,
            due_date: self.due_date,
            assignee: None,
        }
    }
}

impl MigratesTo<TaskV9> for TaskV8 {
    fn migrate(self) -> TaskV9 {
        TaskV9 {
            id: self.id,
            title: self.title,
            description: self.description,
            status: self.status,
            priority: self.priority,
            tags: self.tags,
            due_date: self.due_date,
            assignee: self.assignee,
            created_at: "2023-01-01T00:00:00Z".to_string(),
        }
    }
}

impl MigratesTo<TaskV10> for TaskV9 {
    fn migrate(self) -> TaskV10 {
        TaskV10 {
            id: self.id,
            title: self.title,
            description: self.description,
            status: self.status,
            priority: self.priority,
            tags: self.tags,
            due_date: self.due_date,
            assignee: self.assignee,
            created_at: self.created_at,
            updated_at: "2023-01-01T00:00:00Z".to_string(),
        }
    }
}

// Implement IntoDomain for each version to all subsequent versions and TaskEntity
impl IntoDomain<TaskV2> for TaskV1 {
    fn into_domain(self) -> TaskV2 {
        self.migrate()
    }
}

impl IntoDomain<TaskV3> for TaskV2 {
    fn into_domain(self) -> TaskV3 {
        self.migrate()
    }
}

impl IntoDomain<TaskV4> for TaskV3 {
    fn into_domain(self) -> TaskV4 {
        self.migrate()
    }
}

impl IntoDomain<TaskV5> for TaskV4 {
    fn into_domain(self) -> TaskV5 {
        self.migrate()
    }
}

impl IntoDomain<TaskV6> for TaskV5 {
    fn into_domain(self) -> TaskV6 {
        self.migrate()
    }
}

impl IntoDomain<TaskV7> for TaskV6 {
    fn into_domain(self) -> TaskV7 {
        self.migrate()
    }
}

impl IntoDomain<TaskV8> for TaskV7 {
    fn into_domain(self) -> TaskV8 {
        self.migrate()
    }
}

impl IntoDomain<TaskV9> for TaskV8 {
    fn into_domain(self) -> TaskV9 {
        self.migrate()
    }
}

impl IntoDomain<TaskV10> for TaskV9 {
    fn into_domain(self) -> TaskV10 {
        self.migrate()
    }
}

// Implement IntoDomain<TaskEntity> for intermediate versions through full migration chain
impl IntoDomain<TaskEntity> for TaskV5 {
    fn into_domain(self) -> TaskEntity {
        // V5 -> V6 -> V7 -> V8 -> V9 -> V10 -> TaskEntity
        self.migrate()
            .migrate()
            .migrate()
            .migrate()
            .migrate()
            .into_domain()
    }
}

impl IntoDomain<TaskEntity> for TaskV10 {
    fn into_domain(self) -> TaskEntity {
        TaskEntity {
            id: self.id,
            title: self.title,
            description: self.description,
            status: self.status,
            priority: self.priority,
            tags: self.tags,
            due_date: self.due_date,
            assignee: self.assignee,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

// Add Serialize/Deserialize to TaskEntity
impl Serialize for TaskEntity {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("TaskEntity", 10)?;
        state.serialize_field("id", &self.id)?;
        state.serialize_field("title", &self.title)?;
        state.serialize_field("description", &self.description)?;
        state.serialize_field("status", &self.status)?;
        state.serialize_field("priority", &self.priority)?;
        state.serialize_field("tags", &self.tags)?;
        state.serialize_field("due_date", &self.due_date)?;
        state.serialize_field("assignee", &self.assignee)?;
        state.serialize_field("created_at", &self.created_at)?;
        state.serialize_field("updated_at", &self.updated_at)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for TaskEntity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "snake_case")]
        enum Field {
            Id,
            Title,
            Description,
            Status,
            Priority,
            Tags,
            DueDate,
            Assignee,
            CreatedAt,
            UpdatedAt,
        }

        struct TaskEntityVisitor;

        impl<'de> serde::de::Visitor<'de> for TaskEntityVisitor {
            type Value = TaskEntity;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("struct TaskEntity")
            }

            fn visit_map<V>(self, mut map: V) -> Result<TaskEntity, V::Error>
            where
                V: serde::de::MapAccess<'de>,
            {
                let mut id = None;
                let mut title = None;
                let mut description = None;
                let mut status = None;
                let mut priority = None;
                let mut tags = None;
                let mut due_date = None;
                let mut assignee = None;
                let mut created_at = None;
                let mut updated_at = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        Field::Id => {
                            if id.is_some() {
                                return Err(serde::de::Error::duplicate_field("id"));
                            }
                            id = Some(map.next_value()?);
                        }
                        Field::Title => {
                            if title.is_some() {
                                return Err(serde::de::Error::duplicate_field("title"));
                            }
                            title = Some(map.next_value()?);
                        }
                        Field::Description => {
                            if description.is_some() {
                                return Err(serde::de::Error::duplicate_field("description"));
                            }
                            description = Some(map.next_value()?);
                        }
                        Field::Status => {
                            if status.is_some() {
                                return Err(serde::de::Error::duplicate_field("status"));
                            }
                            status = Some(map.next_value()?);
                        }
                        Field::Priority => {
                            if priority.is_some() {
                                return Err(serde::de::Error::duplicate_field("priority"));
                            }
                            priority = Some(map.next_value()?);
                        }
                        Field::Tags => {
                            if tags.is_some() {
                                return Err(serde::de::Error::duplicate_field("tags"));
                            }
                            tags = Some(map.next_value()?);
                        }
                        Field::DueDate => {
                            if due_date.is_some() {
                                return Err(serde::de::Error::duplicate_field("due_date"));
                            }
                            due_date = Some(map.next_value()?);
                        }
                        Field::Assignee => {
                            if assignee.is_some() {
                                return Err(serde::de::Error::duplicate_field("assignee"));
                            }
                            assignee = Some(map.next_value()?);
                        }
                        Field::CreatedAt => {
                            if created_at.is_some() {
                                return Err(serde::de::Error::duplicate_field("created_at"));
                            }
                            created_at = Some(map.next_value()?);
                        }
                        Field::UpdatedAt => {
                            if updated_at.is_some() {
                                return Err(serde::de::Error::duplicate_field("updated_at"));
                            }
                            updated_at = Some(map.next_value()?);
                        }
                    }
                }

                let id = id.ok_or_else(|| serde::de::Error::missing_field("id"))?;
                let title = title.ok_or_else(|| serde::de::Error::missing_field("title"))?;
                let description =
                    description.ok_or_else(|| serde::de::Error::missing_field("description"))?;
                let status = status.ok_or_else(|| serde::de::Error::missing_field("status"))?;
                let priority =
                    priority.ok_or_else(|| serde::de::Error::missing_field("priority"))?;
                let tags = tags.ok_or_else(|| serde::de::Error::missing_field("tags"))?;
                let due_date =
                    due_date.ok_or_else(|| serde::de::Error::missing_field("due_date"))?;
                let assignee =
                    assignee.ok_or_else(|| serde::de::Error::missing_field("assignee"))?;
                let created_at =
                    created_at.ok_or_else(|| serde::de::Error::missing_field("created_at"))?;
                let updated_at =
                    updated_at.ok_or_else(|| serde::de::Error::missing_field("updated_at"))?;

                Ok(TaskEntity {
                    id,
                    title,
                    description,
                    status,
                    priority,
                    tags,
                    due_date,
                    assignee,
                    created_at,
                    updated_at,
                })
            }
        }

        const FIELDS: &[&str] = &[
            "id",
            "title",
            "description",
            "status",
            "priority",
            "tags",
            "due_date",
            "assignee",
            "created_at",
            "updated_at",
        ];
        deserializer.deserialize_struct("TaskEntity", FIELDS, TaskEntityVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vec_notation_6_versions() {
        let path = migrator!("task", [TaskV1, TaskV2, TaskV3, TaskV4, TaskV5, TaskEntity]);
        let mut migrator = Migrator::new();
        migrator.register(path).unwrap();

        let original = TaskV1 {
            id: "task-1".to_string(),
        };
        let json = migrator.save(original).unwrap();

        // Should migrate through V5 and convert to domain
        let result: TaskEntity = migrator.load("task", &json).unwrap();
        assert_eq!(result.id, "task-1");
        assert_eq!(result.title, "Untitled");
        assert_eq!(result.tags, Vec::<String>::new());
    }

    #[test]
    fn test_vec_notation_7_versions() {
        let path = migrator!(
            "task",
            [TaskV1, TaskV2, TaskV3, TaskV4, TaskV5, TaskV6, TaskV7]
        );
        let mut migrator = Migrator::new();
        migrator.register(path).unwrap();

        let original = TaskV1 {
            id: "task-1".to_string(),
        };
        let json = migrator.save(original).unwrap();

        let result: TaskV7 = migrator.load("task", &json).unwrap();
        assert_eq!(result.id, "task-1");
        assert_eq!(result.due_date, None);
    }

    #[test]
    fn test_vec_notation_8_versions() {
        let path = migrator!(
            "task",
            [TaskV1, TaskV2, TaskV3, TaskV4, TaskV5, TaskV6, TaskV7, TaskV8]
        );
        let mut migrator = Migrator::new();
        migrator.register(path).unwrap();

        let original = TaskV1 {
            id: "task-1".to_string(),
        };
        let json = migrator.save(original).unwrap();

        let result: TaskV8 = migrator.load("task", &json).unwrap();
        assert_eq!(result.id, "task-1");
        assert_eq!(result.assignee, None);
    }

    #[test]
    fn test_vec_notation_9_versions() {
        let path = migrator!(
            "task",
            [TaskV1, TaskV2, TaskV3, TaskV4, TaskV5, TaskV6, TaskV7, TaskV8, TaskV9]
        );
        let mut migrator = Migrator::new();
        migrator.register(path).unwrap();

        let original = TaskV1 {
            id: "task-1".to_string(),
        };
        let json = migrator.save(original).unwrap();

        let result: TaskV9 = migrator.load("task", &json).unwrap();
        assert_eq!(result.id, "task-1");
        assert_eq!(result.created_at, "2023-01-01T00:00:00Z");
    }

    #[test]
    fn test_vec_notation_10_versions() {
        let path = migrator!(
            "task",
            [TaskV1, TaskV2, TaskV3, TaskV4, TaskV5, TaskV6, TaskV7, TaskV8, TaskV9, TaskV10]
        );
        let mut migrator = Migrator::new();
        migrator.register(path).unwrap();

        let original = TaskV1 {
            id: "task-1".to_string(),
        };
        let json = migrator.save(original).unwrap();

        let result: TaskEntity = migrator.load("task", &json).unwrap();
        assert_eq!(result.id, "task-1");
        assert_eq!(result.title, "Untitled");
        assert_eq!(result.created_at, "2023-01-01T00:00:00Z");
        assert_eq!(result.updated_at, "2023-01-01T00:00:00Z");
    }

    #[test]
    fn test_vec_notation_with_default_keys() {
        // Test that vec notation works with standard versioned types
        let path = migrator!(
            "task",
            [TaskV1, TaskV2, TaskV3, TaskV4, TaskV5, TaskV6, TaskV7]
        );
        let mut migrator = Migrator::new();
        migrator.register(path).unwrap();

        let original = TaskV1 {
            id: "task-custom".to_string(),
        };
        let json = migrator.save(original).unwrap();

        // Check that default keys are used
        assert!(json.contains("\"version\""));
        assert!(json.contains("\"data\""));

        let result: TaskV7 = migrator.load("task", &json).unwrap();
        assert_eq!(result.id, "task-custom");
    }

    #[test]
    fn test_middle_version_migration() {
        let path = migrator!(
            "task",
            [TaskV1, TaskV2, TaskV3, TaskV4, TaskV5, TaskV6, TaskV7, TaskV8]
        );
        let mut migrator = Migrator::new();
        migrator.register(path).unwrap();

        // Start from middle version
        let original = TaskV4 {
            id: "task-mid".to_string(),
            title: "Middle Task".to_string(),
            description: Some("From V4".to_string()),
            status: "in-progress".to_string(),
        };
        let json = migrator.save(original).unwrap();

        let result: TaskV8 = migrator.load("task", &json).unwrap();
        assert_eq!(result.id, "task-mid");
        assert_eq!(result.title, "Middle Task");
        assert_eq!(result.description, Some("From V4".to_string()));
        assert_eq!(result.status, "in-progress");
        assert_eq!(result.priority, 0); // Added in V5
        assert_eq!(result.tags, Vec::<String>::new()); // Added in V6
        assert_eq!(result.due_date, None); // Added in V7
        assert_eq!(result.assignee, None); // Added in V8
    }

    #[test]
    fn test_compile_time_vec_syntax() {
        // These should all compile successfully
        let _path1 = migrator!("two", [TaskV1, TaskV2]);
        let _path2 = migrator!("three", [TaskV1, TaskV2, TaskV3]);
        let _path3 = migrator!("four", [TaskV1, TaskV2, TaskV3, TaskV4]);
        let _path4 = migrator!("five", [TaskV1, TaskV2, TaskV3, TaskV4, TaskV5]);
        let _path5 = migrator!("six", [TaskV1, TaskV2, TaskV3, TaskV4, TaskV5, TaskV6]);
        let _path6 = migrator!(
            "seven",
            [TaskV1, TaskV2, TaskV3, TaskV4, TaskV5, TaskV6, TaskV7]
        );
        let _path7 = migrator!(
            "eight",
            [TaskV1, TaskV2, TaskV3, TaskV4, TaskV5, TaskV6, TaskV7, TaskV8]
        );
        let _path8 = migrator!(
            "nine",
            [TaskV1, TaskV2, TaskV3, TaskV4, TaskV5, TaskV6, TaskV7, TaskV8, TaskV9]
        );
        let _path9 = migrator!(
            "ten",
            [TaskV1, TaskV2, TaskV3, TaskV4, TaskV5, TaskV6, TaskV7, TaskV8, TaskV9, TaskV10]
        );

        // With custom keys
        let _path_custom = migrator!(
            "custom",
            [TaskV1, TaskV2, TaskV3, TaskV4, TaskV5, TaskV6, TaskV7],
            version_key = "version",
            data_key = "data"
        );
    }
}

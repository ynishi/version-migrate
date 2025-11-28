use serde::{Deserialize, Serialize};
use version_migrate::{migrator, IntoDomain, MigratesTo, Migrator, Versioned};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct TaskV1 {
    id: String,
    title: String,
}

impl Versioned for TaskV1 {
    const VERSION: &'static str = "1.0.0";
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct TaskV2 {
    id: String,
    title: String,
    description: Option<String>,
}

impl Versioned for TaskV2 {
    const VERSION: &'static str = "2.0.0";
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct TaskV3 {
    id: String,
    title: String,
    description: Option<String>,
    tags: Vec<String>,
}

impl Versioned for TaskV3 {
    const VERSION: &'static str = "3.0.0";
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct TaskEntity {
    id: String,
    title: String,
    description: Option<String>,
    tags: Vec<String>,
}

impl MigratesTo<TaskV2> for TaskV1 {
    fn migrate(self) -> TaskV2 {
        TaskV2 {
            id: self.id,
            title: self.title,
            description: None,
        }
    }
}

impl MigratesTo<TaskV3> for TaskV1 {
    fn migrate(self) -> TaskV3 {
        TaskV3 {
            id: self.id,
            title: self.title,
            description: None,
            tags: Vec::new(),
        }
    }
}

impl IntoDomain<TaskEntity> for TaskV1 {
    fn into_domain(self) -> TaskEntity {
        TaskEntity {
            id: self.id,
            title: self.title,
            description: None,
            tags: Vec::new(),
        }
    }
}

impl MigratesTo<TaskV3> for TaskV2 {
    fn migrate(self) -> TaskV3 {
        TaskV3 {
            id: self.id,
            title: self.title,
            description: self.description,
            tags: Vec::new(),
        }
    }
}

impl IntoDomain<TaskEntity> for TaskV2 {
    fn into_domain(self) -> TaskEntity {
        TaskEntity {
            id: self.id,
            title: self.title,
            description: self.description,
            tags: Vec::new(),
        }
    }
}

impl IntoDomain<TaskEntity> for TaskV3 {
    fn into_domain(self) -> TaskEntity {
        TaskEntity {
            id: self.id,
            title: self.title,
            description: self.description,
            tags: self.tags,
        }
    }
}

#[test]
fn test_migrator_basic() {
    // migrator!("task", [V1, TaskEntity]) - domain model as final target
    let path = migrator!("task", [TaskV1, TaskEntity]);

    let mut migrator = Migrator::new();
    migrator.register(path).unwrap();

    // Test migration from V1 to domain via V3
    let task_v1 = TaskV1 {
        id: "1".to_string(),
        title: "Test Task".to_string(),
    };

    let json = migrator.save(task_v1).unwrap();
    let task_entity: TaskEntity = migrator.load("task", &json).unwrap();

    assert_eq!(task_entity.id, "1");
    assert_eq!(task_entity.title, "Test Task");
    assert_eq!(task_entity.description, None);
    assert_eq!(task_entity.tags, Vec::<String>::new());
}

#[test]
fn test_migrator_multi_step() {
    // migrator!("task", [V1, V2, TaskEntity])
    let path = migrator!("task", [TaskV1, TaskV2, TaskEntity]);

    let mut migrator = Migrator::new();
    migrator.register(path).unwrap();

    // Test migration from V1 all the way to domain
    let task_v1 = TaskV1 {
        id: "1".to_string(),
        title: "Multi-step Task".to_string(),
    };

    let json = migrator.save(task_v1).unwrap();
    let task_entity: TaskEntity = migrator.load("task", &json).unwrap();

    assert_eq!(task_entity.id, "1");
    assert_eq!(task_entity.title, "Multi-step Task");
    assert_eq!(task_entity.description, None);
    assert_eq!(task_entity.tags, Vec::<String>::new());
}

#[test]
fn test_migrator_with_custom_keys() {
    // Test that the macro compiles with custom key syntax
    let _path = migrator!(
        "task",
        [TaskV1, TaskEntity],
        version_key = "schema_version",
        data_key = "payload"
    );

    // Simple compilation test for now - the with_keys functionality can be tested separately
    assert!(true);
}

#[test]
fn test_migrator_syntax_compilation() {
    // This test just ensures all macro patterns compile correctly
    let _path1 = migrator!("task1", [TaskV1, TaskEntity]);
    let _path2 = migrator!("task2", [TaskV1, TaskV2, TaskEntity]);
    let _path3 = migrator!(
        "task3",
        [TaskV1, TaskEntity],
        version_key = "v",
        data_key = "d"
    );
    let _path4 = migrator!(
        "task4",
        [TaskV1, TaskV2, TaskEntity],
        version_key = "v",
        data_key = "d"
    );

    // If we get here, all patterns compiled successfully
    assert!(true);
}

use serde::{Deserialize, Serialize};
use version_migrate::{migrator, IntoDomain, MigratesTo, Migrator, Versioned};

// Simple test versions for Vec notation
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct V1 {
    id: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct V2 {
    id: String,
    name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct V3 {
    id: String,
    name: String,
    age: i32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct V4 {
    id: String,
    name: String,
    age: i32,
    email: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct V5 {
    id: String,
    name: String,
    age: i32,
    email: String,
    active: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct V6 {
    id: String,
    name: String,
    age: i32,
    email: String,
    active: bool,
    tags: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct V7 {
    id: String,
    name: String,
    age: i32,
    email: String,
    active: bool,
    tags: Vec<String>,
    score: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct V8 {
    id: String,
    name: String,
    age: i32,
    email: String,
    active: bool,
    tags: Vec<String>,
    score: f64,
    level: i32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct V9 {
    id: String,
    name: String,
    age: i32,
    email: String,
    active: bool,
    tags: Vec<String>,
    score: f64,
    level: i32,
    created_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct V10 {
    id: String,
    name: String,
    age: i32,
    email: String,
    active: bool,
    tags: Vec<String>,
    score: f64,
    level: i32,
    created_at: String,
    updated_at: String,
}

// Implement Versioned for all versions
impl Versioned for V1 {
    const VERSION: &'static str = "1.0.0";
}
impl Versioned for V2 {
    const VERSION: &'static str = "2.0.0";
}
impl Versioned for V3 {
    const VERSION: &'static str = "3.0.0";
}
impl Versioned for V4 {
    const VERSION: &'static str = "4.0.0";
}
impl Versioned for V5 {
    const VERSION: &'static str = "5.0.0";
}
impl Versioned for V6 {
    const VERSION: &'static str = "6.0.0";
}
impl Versioned for V7 {
    const VERSION: &'static str = "7.0.0";
}
impl Versioned for V8 {
    const VERSION: &'static str = "8.0.0";
}
impl Versioned for V9 {
    const VERSION: &'static str = "9.0.0";
}
impl Versioned for V10 {
    const VERSION: &'static str = "10.0.0";
}

// Implement migration chain
impl MigratesTo<V2> for V1 {
    fn migrate(self) -> V2 {
        V2 {
            id: self.id,
            name: "Unknown".to_string(),
        }
    }
}

impl MigratesTo<V3> for V2 {
    fn migrate(self) -> V3 {
        V3 {
            id: self.id,
            name: self.name,
            age: 0,
        }
    }
}

impl MigratesTo<V4> for V3 {
    fn migrate(self) -> V4 {
        V4 {
            id: self.id,
            name: self.name,
            age: self.age,
            email: "unknown@example.com".to_string(),
        }
    }
}

impl MigratesTo<V5> for V4 {
    fn migrate(self) -> V5 {
        V5 {
            id: self.id,
            name: self.name,
            age: self.age,
            email: self.email,
            active: true,
        }
    }
}

impl MigratesTo<V6> for V5 {
    fn migrate(self) -> V6 {
        V6 {
            id: self.id,
            name: self.name,
            age: self.age,
            email: self.email,
            active: self.active,
            tags: vec![],
        }
    }
}

impl MigratesTo<V7> for V6 {
    fn migrate(self) -> V7 {
        V7 {
            id: self.id,
            name: self.name,
            age: self.age,
            email: self.email,
            active: self.active,
            tags: self.tags,
            score: 0.0,
        }
    }
}

impl MigratesTo<V8> for V7 {
    fn migrate(self) -> V8 {
        V8 {
            id: self.id,
            name: self.name,
            age: self.age,
            email: self.email,
            active: self.active,
            tags: self.tags,
            score: self.score,
            level: 1,
        }
    }
}

impl MigratesTo<V9> for V8 {
    fn migrate(self) -> V9 {
        V9 {
            id: self.id,
            name: self.name,
            age: self.age,
            email: self.email,
            active: self.active,
            tags: self.tags,
            score: self.score,
            level: self.level,
            created_at: "2023-01-01T00:00:00Z".to_string(),
        }
    }
}

impl MigratesTo<V10> for V9 {
    fn migrate(self) -> V10 {
        V10 {
            id: self.id,
            name: self.name,
            age: self.age,
            email: self.email,
            active: self.active,
            tags: self.tags,
            score: self.score,
            level: self.level,
            created_at: self.created_at,
            updated_at: "2023-01-01T00:00:00Z".to_string(),
        }
    }
}

// Domain entity
#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct UserEntity {
    id: String,
    name: String,
    age: i32,
    email: String,
    active: bool,
    tags: Vec<String>,
    score: f64,
    level: i32,
    created_at: String,
    updated_at: String,
}

// Implement IntoDomain for migration chain
impl IntoDomain<V2> for V1 {
    fn into_domain(self) -> V2 {
        self.migrate()
    }
}

impl IntoDomain<V2> for V2 {
    fn into_domain(self) -> V2 {
        self
    }
}

impl IntoDomain<V3> for V2 {
    fn into_domain(self) -> V3 {
        self.migrate()
    }
}

impl IntoDomain<V3> for V3 {
    fn into_domain(self) -> V3 {
        self
    }
}

impl IntoDomain<V4> for V3 {
    fn into_domain(self) -> V4 {
        self.migrate()
    }
}

impl IntoDomain<V5> for V4 {
    fn into_domain(self) -> V5 {
        self.migrate()
    }
}

impl IntoDomain<V6> for V5 {
    fn into_domain(self) -> V6 {
        self.migrate()
    }
}

impl IntoDomain<V6> for V6 {
    fn into_domain(self) -> V6 {
        self
    }
}

impl IntoDomain<V7> for V6 {
    fn into_domain(self) -> V7 {
        self.migrate()
    }
}

impl IntoDomain<V7> for V7 {
    fn into_domain(self) -> V7 {
        self
    }
}

impl IntoDomain<V8> for V7 {
    fn into_domain(self) -> V8 {
        self.migrate()
    }
}

impl IntoDomain<V8> for V8 {
    fn into_domain(self) -> V8 {
        self
    }
}

impl IntoDomain<V9> for V8 {
    fn into_domain(self) -> V9 {
        self.migrate()
    }
}

impl IntoDomain<V9> for V9 {
    fn into_domain(self) -> V9 {
        self
    }
}

impl IntoDomain<V10> for V9 {
    fn into_domain(self) -> V10 {
        self.migrate()
    }
}

impl IntoDomain<V10> for V10 {
    fn into_domain(self) -> V10 {
        self
    }
}

impl IntoDomain<UserEntity> for V9 {
    fn into_domain(self) -> UserEntity {
        // V9 -> V10 -> UserEntity
        self.migrate().into_domain()
    }
}

impl IntoDomain<UserEntity> for V10 {
    fn into_domain(self) -> UserEntity {
        UserEntity {
            id: self.id,
            name: self.name,
            age: self.age,
            email: self.email,
            active: self.active,
            tags: self.tags,
            score: self.score,
            level: self.level,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vec_notation_2_versions() {
        let path = migrator!("test", [V1, V2]);
        let mut migrator = Migrator::new();
        migrator.register(path).unwrap();

        let original = V1 {
            id: "1".to_string(),
        };
        let json = migrator.save(original).unwrap();
        let result: V2 = migrator.load("test", &json).unwrap();
        assert_eq!(result.id, "1");
        assert_eq!(result.name, "Unknown");
    }

    #[test]
    fn test_vec_notation_6_versions() {
        let path = migrator!("test", [V1, V2, V3, V4, V5, V6]);
        let mut migrator = Migrator::new();
        migrator.register(path).unwrap();

        let original = V1 {
            id: "1".to_string(),
        };
        let json = migrator.save(original).unwrap();
        let result: V6 = migrator.load("test", &json).unwrap();
        assert_eq!(result.id, "1");
        assert_eq!(result.name, "Unknown");
        assert_eq!(result.tags, Vec::<String>::new());
    }

    #[test]
    fn test_vec_notation_7_versions() {
        let path = migrator!("test", [V1, V2, V3, V4, V5, V6, V7]);
        let mut migrator = Migrator::new();
        migrator.register(path).unwrap();

        let original = V1 {
            id: "1".to_string(),
        };
        let json = migrator.save(original).unwrap();
        let result: V7 = migrator.load("test", &json).unwrap();
        assert_eq!(result.id, "1");
        assert_eq!(result.score, 0.0);
    }

    #[test]
    fn test_vec_notation_8_versions() {
        let path = migrator!("test", [V1, V2, V3, V4, V5, V6, V7, V8]);
        let mut migrator = Migrator::new();
        migrator.register(path).unwrap();

        let original = V1 {
            id: "1".to_string(),
        };
        let json = migrator.save(original).unwrap();
        let result: V8 = migrator.load("test", &json).unwrap();
        assert_eq!(result.id, "1");
        assert_eq!(result.level, 1);
    }

    #[test]
    fn test_vec_notation_9_versions() {
        let path = migrator!("test", [V1, V2, V3, V4, V5, V6, V7, V8, V9]);
        let mut migrator = Migrator::new();
        migrator.register(path).unwrap();

        let original = V1 {
            id: "1".to_string(),
        };
        let json = migrator.save(original).unwrap();
        let result: V9 = migrator.load("test", &json).unwrap();
        assert_eq!(result.id, "1");
        assert_eq!(result.created_at, "2023-01-01T00:00:00Z");
    }

    #[test]
    fn test_vec_notation_10_versions_to_domain() {
        let path = migrator!("test", [V1, V2, V3, V4, V5, V6, V7, V8, V9, UserEntity]);
        let mut migrator = Migrator::new();
        migrator.register(path).unwrap();

        let original = V1 {
            id: "1".to_string(),
        };
        let json = migrator.save(original).unwrap();
        let result: UserEntity = migrator.load("test", &json).unwrap();
        assert_eq!(result.id, "1");
        assert_eq!(result.name, "Unknown");
        assert_eq!(result.updated_at, "2023-01-01T00:00:00Z");
    }

    #[test]
    fn test_vec_notation_with_default_keys() {
        // Test vec notation with default version/data keys
        let path = migrator!("test", [V1, V2, V3, V4, V5]);
        let mut migrator = Migrator::new();
        migrator.register(path).unwrap();

        let original = V1 {
            id: "custom-1".to_string(),
        };
        let json = migrator.save(original).unwrap();

        // Check that default keys are used
        assert!(json.contains("\"version\""));
        assert!(json.contains("\"data\""));

        let result: V5 = migrator.load("test", &json).unwrap();
        assert_eq!(result.id, "custom-1");
        assert!(result.active);
    }

    #[test]
    fn test_compile_time_vec_syntax() {
        // These should all compile successfully
        let _path1 = migrator!("two", [V1, V2]);
        let _path2 = migrator!("three", [V1, V2, V3]);
        let _path3 = migrator!("four", [V1, V2, V3, V4]);
        let _path4 = migrator!("five", [V1, V2, V3, V4, V5]);
        let _path5 = migrator!("six", [V1, V2, V3, V4, V5, V6]);
        let _path6 = migrator!("seven", [V1, V2, V3, V4, V5, V6, V7]);
        let _path7 = migrator!("eight", [V1, V2, V3, V4, V5, V6, V7, V8]);
        let _path8 = migrator!("nine", [V1, V2, V3, V4, V5, V6, V7, V8, V9]);
        let _path9 = migrator!("ten", [V1, V2, V3, V4, V5, V6, V7, V8, V9, V10]);

        // With custom keys
        let _path_custom = migrator!(
            "custom",
            [V1, V2, V3, V4, V5],
            version_key = "version",
            data_key = "data"
        );
    }

    #[test]
    fn test_middle_version_start() {
        let path = migrator!("test", [V1, V2, V3, V4, V5, V6]);
        let mut migrator = Migrator::new();
        migrator.register(path).unwrap();

        // Start from middle version
        let original = V3 {
            id: "mid-1".to_string(),
            name: "Middle User".to_string(),
            age: 25,
        };
        let json = migrator.save(original).unwrap();

        let result: V6 = migrator.load("test", &json).unwrap();
        assert_eq!(result.id, "mid-1");
        assert_eq!(result.name, "Middle User");
        assert_eq!(result.age, 25);
        assert_eq!(result.email, "unknown@example.com"); // Added in V4
        assert!(result.active); // Added in V5
        assert_eq!(result.tags, Vec::<String>::new()); // Added in V6
    }
}

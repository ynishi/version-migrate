# DirStorage 設計書 (Rev2)

## 1. 概要

### 目的
ディレクトリベースで複数のエンティティファイルを管理する永続化層を提供する。

### FileStorage との比較

| 特徴 | FileStorage | DirStorage |
|------|------------|------------|
| 管理対象 | 1ファイル（複数キー） | 複数ファイル（各ファイル=1エンティティ） |
| データ構造 | `{"key1": [entity...], "key2": [...]}` | `{id}.json` ごとに1エンティティ |
| 用途 | 設定ファイル管理 | データストレージ（sessions, tasksなど） |
| 内部実装 | ConfigMigrator使用 | Migrator直接使用 |
| ファイル数 | 1個 | N個（エンティティ数分） |
| パス解決 | `FileStorage::new(PathBuf, ...)` | `DirStorage::new(AppPaths, ...)` |

### ユースケース
- セッション管理: `sessions/session-123.json`
- タスク管理: `tasks/task-456.json`
- ユーザーデータ: `users/user-789.json`

---

## 2. API設計

### 2.1 主要な型定義

```rust
/// ディレクトリベースのエンティティストレージ
pub struct DirStorage {
    /// ベースディレクトリパス
    base_path: PathBuf,
    /// マイグレーター
    migrator: Migrator,
    /// ストレージ戦略
    strategy: DirStorageStrategy,
}

/// DirStorage用の戦略設定
#[derive(Debug, Clone)]
pub struct DirStorageStrategy {
    /// ファイルフォーマット（JSON/TOML）
    pub format: FormatStrategy,
    /// Atomic write設定
    pub atomic_write: AtomicWriteConfig,
    /// ファイル拡張子（デフォルト: formatから自動決定）
    pub extension: Option<String>,
    /// ファイル名エンコーディング戦略
    pub filename_encoding: FilenameEncoding,
}

/// ファイル名のエンコーディング戦略
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilenameEncoding {
    /// IDをそのままファイル名に使用（安全な文字のみ想定）
    Direct,
    /// URLエンコード（特殊文字を含むID用）
    UrlEncode,
    /// Base64エンコード（完全な安全性が必要な場合）
    Base64,
}
```

### 2.2 公開メソッド

```rust
impl DirStorage {
    /// 新しいDirStorageインスタンスを作成
    ///
    /// # Arguments
    ///
    /// * `paths` - アプリケーションパス管理 (`AppPaths`)
    /// * `domain_name` - ドメイン名（データディレクトリ内のサブディレクトリ名）
    /// * `migrator` - マイグレーター（エンティティの変換パスが登録済みであること）
    /// * `strategy` - ストレージ戦略
    ///
    /// # Behavior
    ///
    /// - `paths.data_dir()` を基準に `domain_name` のサブディレクトリを作成
    /// - 既存ファイルは読み込まない（遅延ロード）
    ///
    /// # Errors
    ///
    /// - ディレクトリ作成に失敗した場合
    pub fn new(
        paths: AppPaths,
        domain_name: &str,
        migrator: Migrator,
        strategy: DirStorageStrategy,
    ) -> Result<Self, MigrationError>;

    /// 単一エンティティをロードして自動マイグレーション
    ///
    /// # Arguments
    ///
    /// * `entity_name` - エンティティ名（Migratorに登録済みの名前）
    /// * `id` - エンティティID（ファイル名の元になる）
    ///
    /// # Returns
    ///
    /// マイグレーション済みのドメインエンティティ
    ///
    /// # Errors
    ///
    /// - ファイルが存在しない場合は `MigrationError::IoError`
    /// - パース失敗時は `MigrationError::DeserializationError`
    /// - マイグレーション失敗時は該当するエラー
    ///
    /// # Example
    ///
    /// ```rust
    /// let session: SessionEntity = storage.load("session", "session-123")?;
    /// ```
    pub fn load<D>(
        &self,
        entity_name: &str,
        id: &str,
    ) -> Result<D, MigrationError>
    where
        D: for<'de> serde::Deserialize<'de>;

    /// 単一エンティティをアトミックに保存
    ///
    /// # Arguments
    ///
    /// * `entity_name` - エンティティ名（Migratorに登録済みの名前）
    /// * `id` - エンティティID
    /// * `entity` - 保存するドメインエンティティ
    ///
    /// # Behavior
    ///
    /// - Domain → Latest Versioned → JSON/TOML 変換
    /// - Atomic write（tmp file + rename）で保存
    /// - 親ディレクトリが存在しない場合は自動作成
    ///
    /// # Requirements
    ///
    /// - Migration pathが `into_with_save()` で登録されている必要がある
    ///
    /// # Errors
    ///
    /// - エンティティが `into_with_save()` で登録されていない場合
    /// - ファイル書き込みに失敗した場合
    ///
    /// # Example
    ///
    /// ```rust
    /// let session = SessionEntity { /* ... */ };
    /// storage.save("session", "session-123", session)?;
    /// ```
    pub fn save<T>(
        &self,
        entity_name: &str,
        id: &str,
        entity: T,
    ) -> Result<(), MigrationError>
    where
        T: serde::Serialize;

    /// すべてのエンティティIDをリスト
    ///
    /// # Returns
    ///
    /// ディレクトリ内の全ファイルのIDリスト（ソート済み）
    ///
    /// # Behavior
    ///
    /// - 指定された拡張子でフィルタリング
    /// - ファイル名をデコード（`filename_encoding`に従う）
    /// - アルファベット順でソート
    ///
    /// # Example
    ///
    /// ```rust
    /// let ids = storage.list_ids()?;
    /// // → ["session-123", "session-456", ...]
    /// ```
    pub fn list_ids(&self) -> Result<Vec<String>, MigrationError>;

    /// すべてのエンティティをロード
    ///
    /// # Arguments
    ///
    /// * `entity_name` - エンティティ名
    ///
    /// # Returns
    ///
    /// `(ID, エンティティ)` のタプルのベクター
    ///
    /// # Errors
    ///
    /// いずれかのファイルが読み込めない場合はエラーを返す
    /// （部分的な読み込みはサポートしない）
    ///
    /// # Example
    ///
    /// ```rust
    /// let all: Vec<(String, SessionEntity)> = storage.load_all("session")?;
    /// for (id, session) in all {
    ///     println!("{}: {:?}", id, session);
    /// }
    /// ```
    pub fn load_all<D>(
        &self,
        entity_name: &str,
    ) -> Result<Vec<(String, D)>, MigrationError>
    where
        D: for<'de> serde::Deserialize<'de>;

    /// エンティティを削除
    ///
    /// # Arguments
    ///
    /// * `id` - 削除するエンティティID
    ///
    /// # Behavior
    ///
    /// - ファイルが存在しない場合でもエラーにしない（冪等性）
    ///
    /// # Example
    ///
    /// ```rust
    /// storage.delete("session-123")?;
    /// ```
    pub fn delete(&self, id: &str) -> Result<(), MigrationError>;

    /// エンティティの存在確認
    ///
    /// # Returns
    ///
    /// ファイルが存在する場合は `true`
    ///
    /// # Example
    ///
    /// ```rust
    /// if storage.exists("session-123") {
    ///     // ...
    /// }
    /// ```
    pub fn exists(&self, id: &str) -> bool;
}
```

### 2.3 内部メソッド（private）

```rust
impl DirStorage {
    /// IDからファイルパスを構築
    ///
    /// # Behavior
    ///
    /// - `filename_encoding` に従ってIDをエンコード
    /// - 拡張子を付加
    /// - base_path と結合
    fn id_to_path(&self, id: &str) -> Result<PathBuf, MigrationError>;

    /// ファイルパスからIDを抽出
    ///
    /// # Behavior
    ///
    /// - 拡張子を除去
    /// - `filename_encoding` に従ってデコード
    fn path_to_id(&self, path: &Path) -> Result<String, MigrationError>;

    /// ファイル拡張子を取得
    ///
    /// # Returns
    ///
    /// - `strategy.extension` が設定されていればそれを返す
    /// - なければ `strategy.format` から自動決定（json/toml）
    fn get_extension(&self) -> &str;

    /// Atomic writeの実装
    ///
    /// FileStorageと同じロジック：
    /// 1. 一時ファイルに書き込み
    /// 2. fsync
    /// 3. atomic rename
    /// 4. リトライ処理
    fn atomic_write(
        &self,
        path: &Path,
        content: &str,
    ) -> Result<(), MigrationError>;

    /// Format変換：JSON Value → 文字列
    fn serialize_content(
        &self,
        value: &serde_json::Value,
    ) -> Result<String, MigrationError>;

    /// Format変換：文字列 → JSON Value
    fn deserialize_content(
        &self,
        content: &str,
    ) -> Result<serde_json::Value, MigrationError>;
}
```

---

## 3. DirStorageStrategy の詳細

```rust
impl DirStorageStrategy {
    /// デフォルト戦略
    ///
    /// - format: JSON
    /// - atomic_write: デフォルト設定（retry 3回）
    /// - extension: None（formatから自動決定）
    /// - filename_encoding: Direct
    pub fn default() -> Self {
        Self {
            format: FormatStrategy::Json,
            atomic_write: AtomicWriteConfig::default(),
            extension: None,
            filename_encoding: FilenameEncoding::Direct,
        }
    }

    /// Builder: フォーマットを設定
    pub fn with_format(mut self, format: FormatStrategy) -> Self {
        self.format = format;
        self
    }

    /// Builder: 拡張子を設定
    pub fn with_extension(mut self, ext: impl Into<String>) -> Self {
        self.extension = Some(ext.into());
        self
    }

    /// Builder: ファイル名エンコーディングを設定
    pub fn with_filename_encoding(mut self, encoding: FilenameEncoding) -> Self {
        self.filename_encoding = encoding;
        self
    }

    /// Builder: リトライ回数を設定
    pub fn with_retry_count(mut self, count: usize) -> Self {
        self.atomic_write.retry_count = count;
        self
    }

    /// Builder: クリーンアップ設定
    pub fn with_cleanup(mut self, cleanup: bool) -> Self {
        self.atomic_write.cleanup_tmp_files = cleanup;
        self
    }

    /// 拡張子を取得（内部用）
    pub fn get_extension(&self) -> String {
        self.extension.clone().unwrap_or_else(|| {
            match self.format {
                FormatStrategy::Json => "json".to_string(),
                FormatStrategy::Toml => "toml".to_string(),
            }
        })
    }
}
```

---

## 4. FilenameEncoding の実装詳細

```rust
impl FilenameEncoding {
    /// IDをファイル名にエンコード
    pub fn encode(&self, id: &str) -> Result<String, MigrationError> {
        match self {
            FilenameEncoding::Direct => {
                // 安全な文字のみ許可（検証）
                if id.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
                    Ok(id.to_string())
                } else {
                    Err(MigrationError::FilenameEncoding {
                        id: id.to_string(),
                        error: "ID contains unsafe characters for Direct encoding".to_string(),
                    })
                }
            }
            FilenameEncoding::UrlEncode => {
                // URLエンコード
                Ok(urlencoding::encode(id).to_string())
            }
            FilenameEncoding::Base64 => {
                // Base64エンコード（URL-safe）
                use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
                Ok(URL_SAFE_NO_PAD.encode(id.as_bytes()))
            }
        }
    }

    /// ファイル名をIDにデコード
    pub fn decode(&self, filename: &str) -> Result<String, MigrationError> {
        match self {
            FilenameEncoding::Direct => {
                Ok(filename.to_string())
            }
            FilenameEncoding::UrlEncode => {
                urlencoding::decode(filename)
                    .map(|s| s.to_string())
                    .map_err(|e| MigrationError::FilenameEncoding {
                        id: filename.to_string(),
                        error: format!("URL decode failed: {}", e),
                    })
            }
            FilenameEncoding::Base64 => {
                use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
                URL_SAFE_NO_PAD
                    .decode(filename.as_bytes())
                    .and_then(|bytes| {
                        String::from_utf8(bytes)
                            .map_err(|e| base64::DecodeError::InvalidByte(0, e.utf8_error().valid_up_to() as u8))
                    })
                    .map_err(|e| MigrationError::FilenameEncoding {
                        id: filename.to_string(),
                        error: format!("Base64 decode failed: {}", e),
                    })
            }
        }
    }
}
```

---

## 5. 使用例

### 5.1 基本的な使い方

```rust
use version_migrate::{AppPaths, Migrator, DirStorage, DirStorageStrategy, FormatStrategy};

// Migratorのセットアップ
let mut migrator = Migrator::new();

let session_path = Migrator::define("session")
    .from::<SessionV1_0_0>()
    .step::<SessionV1_1_0>()
    .into_with_save::<SessionEntity>();

migrator.register(session_path)?;

// AppPathsの準備
let paths = AppPaths::new("my-app");

// DirStorageの作成
let strategy = DirStorageStrategy::default()
    .with_format(FormatStrategy::Json);

let storage = DirStorage::new(
    paths,
    "sessions", // domain_name
    migrator,
    strategy,
)?;

// 保存
let session = SessionEntity {
    id: "session-123".into(),
    user: "alice".into(),
    created_at: Utc::now(),
};

storage.save("session", "session-123", session)?;
// → (e.g. on Linux) ~/.local/share/my-app/sessions/session-123.json が作成される

// ロード
let loaded: SessionEntity = storage.load("session", "session-123")?;

// 一覧取得
let ids = storage.list_ids()?;
println!("Sessions: {:?}", ids);

// 全件ロード
let all_sessions: Vec<(String, SessionEntity)> = storage.load_all("session")?;
for (id, session) in all_sessions {
    println!("{}: {:?}", id, session);
}

// 削除
storage.delete("session-123")?;
```

### 5.2 エンコーディング戦略の使い分け

```rust
// 安全なID（英数字とハイフンのみ）
let strategy_direct = DirStorageStrategy::default()
    .with_filename_encoding(FilenameEncoding::Direct);

// 特殊文字を含むID（例: "user@example.com"）
let strategy_url = DirStorageStrategy::default()
    .with_filename_encoding(FilenameEncoding::UrlEncode);

// 完全に安全なエンコーディング
let strategy_base64 = DirStorageStrategy::default()
    .with_filename_encoding(FilenameEncoding::Base64);
```

### 5.3 TOMLフォーマットの使用

```rust
let strategy = DirStorageStrategy::default()
    .with_format(FormatStrategy::Toml);

let storage = DirStorage::new(
    paths, // AppPaths instance
    "tasks",
    migrator,
    strategy,
)?;

// tasks/task-456.toml として保存される
```

---

## 6. エラーハンドリング

### 6.1 新規追加のエラー型

```rust
pub enum MigrationError {
    // ... 既存のエラー ...

    /// ファイル名エンコーディングエラー
    FilenameEncoding {
        id: String,
        error: String,
    },

    /// ディレクトリ操作エラー
    DirectoryError {
        path: String,
        error: String,
    },
}
```

### 6.2 エラーケース

| 操作 | エラーケース | エラー型 |
|------|-------------|---------|
| `new()` | ディレクトリ作成失敗 | `IoError` |
| `new()` | ホームディレクトリ解決不可 | `HomeDirNotFound` |
| `load()` | ファイル不存在 | `IoError` |
| `load()` | パース失敗 | `DeserializationError` |
| `load()` | マイグレーション失敗 | `MigrationStepFailed` |
| `save()` | エンティティ未登録 | `EntityNotFound` |
| `save()` | ファイル書き込み失敗 | `IoError` |
| `id_to_path()` | 不正なID（Direct時） | `FilenameEncoding` |
| `list_ids()` | ディレクトリ読み込み失敗 | `DirectoryError` |

---

## 7. ディレクトリ構造例

```
# Linux (System Strategy)
~/.local/share/
└── my-app/
    ├── sessions/
    │   ├── session-123.json
    │   ├── session-456.json
    │   └── session-789.json
    ├── tasks/
    │   ├── task-001.toml
    │   ├── task-002.toml
    │   └── task-003.toml
    └── users/
        ├── user%40example.com.json  (URLエンコード)
        └── dXNlci0xMjM.json         (Base64エンコード)
```

---

## 8. 実装の優先順位

### Phase 1: 基本機能（MVP）
- `DirStorage` 構造体とコンストラクタ (`AppPaths` 対応)
- `save()`, `load()`, `delete()` メソッド
- `FilenameEncoding::Direct` のみサポート
- JSON形式のみサポート
- 基本的なエラーハンドリング

### Phase 2: 拡張機能
- `list_ids()`, `load_all()`, `exists()` メソッド
- TOML形式のサポート
- `FilenameEncoding::UrlEncode`, `Base64` の実装
- 詳細なエラーメッセージ

### Phase 3: 最適化・高度な機能
- パフォーマンス最適化
- バッチ操作（複数エンティティの一括保存など）
- ファイルウォッチャー（変更検知）
- トランザクション的な操作（複数ファイルのアトミック更新）

---

## 9. テスト計画

### 9.1 ユニットテスト

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_save_and_load_json() { /* ... */ }

    #[test]
    fn test_save_and_load_toml() { /* ... */ }

    #[test]
    fn test_filename_encoding_direct() { /* ... */ }

    #[test]
    fn test_filename_encoding_url() { /* ... */ }

    #[test]
    fn test_filename_encoding_base64() { /* ... */ }

    #[test]
    fn test_list_ids() { /* ... */ }

    #[test]
    fn test_load_all() { /* ... */ }

    #[test]
    fn test_delete_idempotent() { /* ... */ }

    #[test]
    fn test_atomic_write() { /* ... */ }

    #[test]
    fn test_migration_on_load() { /* ... */ }
}
```

### 9.2 統合テスト

- FileStorage と DirStorage の併用
- 実際のファイルシステムでの動作確認
- マイグレーション処理の検証

---

## 10. 実装時の注意点

1. **Atomic write の重要性**
   - 必ず tmp file + rename パターンを使用
   - fsync を忘れない
   - リトライロジックを実装

2. **ファイル名の安全性**
   - Direct encoding では厳密なバリデーション
   - パストラバーサル攻撃の防止（`..` などを拒否）

3. **エラーメッセージ**
   - ユーザーが問題を特定できる詳細な情報を含める
   - ファイルパスや具体的なエラー原因を明記

4. **パフォーマンス**
   - `load_all()` は大量ファイルで遅くなる可能性
   - 必要に応じてストリーミングAPIを検討

5. **後方互換性**
   - FileStorage と同じ FormatStrategy, AtomicWriteConfig を再利用
   - 既存のエラー型を拡張

---

## 11. 将来の拡張性

### 11.1 考えられる機能追加

- **フィルタリング**: `list_ids_filtered(predicate)` など
- **ページネーション**: 大量ファイル向け
- **並行アクセス**: ファイルロック機構
- **キャッシング**: メモリキャッシュによる高速化
- **バックアップ**: 自動バックアップ機能
- **圧縮**: gzip などでの保存

### 11.2 他のストレージバックエンド

同じインターフェースで以下も実装可能：
- `S3Storage`: AWS S3ベース
- `DbStorage`: SQLiteベース
- `MemStorage`: メモリベース（テスト用）

---

## 12. 参考実装

FileStorage の実装（`storage.rs`）を参考にする：
- Atomic write のロジック
- Format 変換の処理
- エラーハンドリングのパターン
- テストの構造
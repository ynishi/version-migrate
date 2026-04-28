# local-store

[![Crates.io](https://img.shields.io/crates/v/local-store.svg)](https://crates.io/crates/local-store)
[![Documentation](https://docs.rs/local-store/badge.svg)](https://docs.rs/local-store)
[![License](https://img.shields.io/crates/l/local-store.svg)](https://github.com/ynishi/version-migrate#license)

ローカルストレージ管理を完全に閉じる装置 — path 解決 + ACID file/dir storage + atomic IO + format dispatch。

`version-migrate` の基盤として使われているほか、スキーマバージョン管理が不要なアプリケーションでも standalone で利用できます。

## API Philosophy

**fallback 所有**: `PathStrategy::default()` は `System` (OS 標準ディレクトリ) に解決します。caller 側に fallback ロジックを漏らしません。

**category 受け**: `DirStorage` の category 引数は `impl Into<String>` で受けます。enum で固定せず、typo 防止の責務は caller 側に委ねます。

**PathBuf 露出は現状維持**: `AppPaths::config_dir()` 等は `PathBuf` を返します。高レベルな読み書きは `FileStorage` / `DirStorage` が担うため、raw path の完全な非露出化は将来の検討事項です。

## Features

- **Platform-Agnostic Paths**: `AppPaths` + `PathStrategy` で Linux / macOS / Windows のパス解決を統一
- **Atomic File IO**: write-to-temp + rename による ACID 保証 (`atomic_io` モジュール)
- **FileStorage**: 単一ファイル設定向けの ACID ストレージ (TOML / JSON、retry、format 変換)
- **DirStorage**: エンティティをファイル単位で管理するディレクトリストレージ (`FilenameEncoding` 付き)
- **AsyncDirStorage**: `async` feature フラグで有効になる非同期版 DirStorage (`tokio::fs` 使用)
- **Format Dispatch**: `FormatStrategy` で TOML / JSON を切り替え、`format_convert` で相互変換
- **Minimal Dependencies**: `dirs` + `thiserror` のみが必須依存

## Quick Start

```toml
[dependencies]
local-store = "0.1.0"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
```

```rust
use local_store::{AppPaths, FileStorage, FileStorageStrategy, FormatStrategy};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Config {
    name: String,
    value: u32,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // OS 標準ディレクトリに config.json を置く
    let paths = AppPaths::new("myapp");
    let config_path = paths.config_file("config.json")?;

    let strategy = FileStorageStrategy::default()
        .with_format(FormatStrategy::Json);

    let mut storage = FileStorage::new(config_path, strategy)?;

    // atomic write (temp + rename)
    storage.save("config", &Config { name: "hello".into(), value: 42 })?;

    let loaded: Config = storage.load("config")?;
    println!("{}: {}", loaded.name, loaded.value);
    Ok(())
}
```

## Main API

| 型 / モジュール | 役割 |
|---|---|
| `AppPaths` | アプリ名からコンフィグ・データディレクトリを解決 |
| `PathStrategy` | `System` / `Xdg` / `CustomBase` の解決戦略 |
| `PrefPath` | 個別ファイルパスの抽象 |
| `FileStorage` | 単一ファイル ACID ストレージ |
| `FileStorageStrategy` | FileStorage の設定 (format / retry / load behavior) |
| `FormatStrategy` | `Json` / `Toml` の切り替え |
| `LoadBehavior` | `CreateIfMissing` / `SaveIfMissing` / `ErrorIfMissing` |
| `DirStorage` | ディレクトリ単位のエンティティ管理 (sync) |
| `AsyncDirStorage` | 非同期版 DirStorage (`async` feature 必須) |
| `DirStorageStrategy` | DirStorage の設定 |
| `FilenameEncoding` | `Plain` / `UrlEncode` / `Base64` |
| `StoreError` | ストレージ操作の統合エラー型 |
| `IoOperationKind` | エラー診断用の操作種別 |
| `atomic_io` | write-to-temp + atomic rename の低レベル操作 |
| `format_convert` | JSON ↔ TOML 変換ユーティリティ |

## Crate Relationship

```
local-store         ← 基盤: path 解決 + ACID IO + format dispatch
    ↑
version-migrate     ← schema 進化を上に乗せる (FileStorage / DirStorage を再エクスポート)
```

`version-migrate` の `FileStorage` / `DirStorage` は `local-store` の薄いラッパーです。raw IO と format 変換のロジックはすべて `local-store` に集約されています。スキーマバージョン管理が不要な場合は `local-store` を直接使用できます。

## Async Support

```toml
[dependencies]
local-store = { version = "0.1.0", features = ["async"] }
tokio = { version = "1.0", features = ["full"] }
```

```rust
use local_store::{AppPaths, AsyncDirStorage, DirStorageStrategy, FilenameEncoding};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let paths = AppPaths::new("myapp");
    let strategy = DirStorageStrategy::default()
        .with_filename_encoding(FilenameEncoding::UrlEncode);

    let storage = AsyncDirStorage::new(paths, "sessions", strategy).await?;

    storage.save("session", "user@example.com", &my_entity).await?;
    let loaded: MyEntity = storage.load("session", "user@example.com").await?;
    Ok(())
}
```

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](../LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](../LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

# Lumen for Mac: Native Swift & SwiftUI Architecture (`docs/08`)

> **Status: Vision / Roadmap.** This document describes a target architecture and product direction, not the current implementation — components and crates referenced here (e.g. `lumen-daemon`, `lumen-mac`, `lumen-cloud`, `lumen-store`, `lumen-insights`) may not yet exist in the workspace or may differ materially once built. Treat claims of behavior, licensing, or performance in this document as aspirational unless independently verified against the current codebase; docs 01-06 describe the implemented system and take precedence wherever the two conflict.

This document defines the application architecture, Swift libraries, reactive database observation, and user interface contracts for `Lumen.app` (`apps/lumen-mac`).

---

## 1. Zero-Bridge Decoupled Process Architecture

Lumen avoids complex C FFI bindings or heavy Electron runtimes by using a decoupled, local-first architecture:

```mermaid
flowchart TD
    classDef daemon fill:#eef2ff,stroke:#6366f1,stroke-width:2px,color:#1e1b4b,rx:8px,ry:8px;
    classDef store fill:#f8fafc,stroke:#64748b,stroke-width:2px,color:#0f172a,rx:8px,ry:8px;
    classDef swift fill:#ecfdf5,stroke:#10b981,stroke-width:2px,color:#064e3b,rx:8px,ry:8px;

    Daemon["<b>lumen-daemon (Rust)</b><br/>Single SQLite WAL Writer (< 10ms enqueue)"]:::daemon
    Store[("<b>lumen.db (SQLite WAL)</b><br/><code>~/Library/Application Support/Lumen/lumen.db</code>")]:::store
    SwiftApp["<b>Lumen.app (Native SwiftUI)</b><br/>• GRDB.swift DatabasePool (Read-Only)<br/>• ValueObservation (Reactive UI Updates)"]:::swift

    Daemon -->|Writes Facts & Findings| Store
    Store -->|Reads Concurrently (< 1ms)| SwiftApp

    style Daemon fill:#fafafa,stroke:#cbd5e1,stroke-width:1.5px,stroke-dasharray: 4 4,rx:10px,ry:10px
    style Store fill:#fafafa,stroke:#cbd5e1,stroke-width:1.5px,stroke-dasharray: 4 4,rx:10px,ry:10px
    style SwiftApp fill:#fafafa,stroke:#cbd5e1,stroke-width:1.5px,stroke-dasharray: 4 4,rx:10px,ry:10px
```

* **Rust Daemon (`lumen-daemon`)**: Holds the exclusive write connection to SQLite WAL.
* **Swift App (`Lumen.app`)**: Opens SQLite in `DatabasePool` **read-only mode**.
* **Zero Lock Contention**: Swift reads at memory speed (`< 1ms`) while Rust writes asynchronously in the background.

---

## 2. Recommended Swift Package Stack

| Functional Area | Library | Purpose |
| :--- | :--- | :--- |
| **Database & SQLite** | `GRDB.swift` + `GRDBQuery` | Zero-ORM SQLite WAL connection pool with reactive `ValueObservation` auto-updating SwiftUI views. |
| **Menu Bar UI** | `SwiftUI MenuBarExtra` | Native macOS windowed popover (`LSUIElement = 1`). |
| **Data Visualization** | `Swift Charts` | Hardware-accelerated sparklines, bullet graphs, and area charts. |
| **Global Hotkeys** | `KeyboardShortcuts` | System-wide `Cmd+Shift+L` toggle. |
| **Auto-Launch** | `LaunchAtLogin` | Native `SMAppService` startup registration. |
| **Auto-Updates** | `Sparkle 2.0` | EdDSA cryptographically signed updates. |

---

## 3. Reactive Data Flow with `GRDB.swift`

Whenever `lumen-daemon` in Rust inserts a new session turn or finding into SQLite, `GRDB.swift`'s `ValueObservation` automatically pushes the updated state to SwiftUI:

```swift
import SwiftUI
import GRDB
import GRDBQuery

struct TodaySpendQuery: Queryable {
    static var defaultValue: Double = 0.0

    func publisher(in database: DatabasePool) -> AnyPublisher<Double, Error> {
        ValueObservation
            .tracking { db in
                try Double.fetchOne(db, sql: """
                    SELECT COALESCE(SUM(total_cost_usd), 0.0)
                    FROM sessions
                    WHERE started_at >= datetime('now', 'start of day')
                """) ?? 0.0
            }
            .publisher(in: database, scheduling: .immediate)
            .eraseToAnyPublisher()
    }
}
```

---

## 4. UI Layout & Visual Trajectory Canvas

The menu bar popover features 4 primary sections:
1. **Header KPIs**: Today's spend ($), prompt cache hit ratio ($H$), and active session indicator.
2. **Bullet Health Gauges**: Compact horizontal bars showing cache health vs targets.
3. **Interactive Trajectory Canvas**: Petgraph-rendered node graph showing the agent's file operations, with Tarjan SCC cycles highlighted in red.
4. **Recent Sessions List**: Filterable by repository, branch, and category.

# mini-pi 设计评审

## 一、架构总览

```
main.rs (引导启动)
  ├── core/      — 全局状态 AppStore、Actions、窗口配置、SVG 资产加载
  ├── data/      — SQLite 存储 + 迁移 (threads/workspaces 两张表)
  ├── rpc/       — 子进程桥接，JSON Lines 协议与 pi CLI 通信
  ├── auth/      — Supabase 认证 (注册/登录/刷新令牌)
  ├── config/    — 本地配置文件 + 硬编码模型列表
  ├── sync/      — agent 配置文件与 Supabase Storage 双向同步
  ├── ui/        — 可复用组件 (TextInput, ChatInput, Dropdown, MarkdownRenderer 等)
  ├── views/     — 顶层视图 (ThreadList, ChatWindow, TitleBar, UserPanel 等)
  └── utils/     — 文件扫描、时间格式化、LLM 标题生成
```

状态管理采用 **GPUI Global (AppStore) + Entity 树** 混合模式。每个聊天线程独立窗口，通过 `thread_windows: HashMap<i64, AnyWindowHandle>` 映射管理。

---

## 二、关键设计问题

### 🔴 严重

**1. PiRpc 进程无守护/重启机制** (`src/rpc/pi_rpc.rs:892`)
- `PiRpc._child: Child` 从不被 `.wait()` 或监控。若 `pi` 进程崩溃，仅在 stdout 管道断连时才检测到。进程句柄变为僵尸进程，且**不会自动重启**。当前 `Disconnected` 事件只是标记状态错误，但不做任何恢复尝试。

**2. 后台同步线程无文件锁保护** (`src/sync/settings_sync.rs:272`)
- `sync_changes()` 在独立线程中读写 `~/.mini-pi/agent/` 文件，而 `pi` 子进程也可能同时在读写同一批文件。没有文件锁或协调机制，存在竞态和数据损坏风险。

**3. AppStore.thread_windows 可能积累失效句柄** (`src/core/app.rs:40`)
- 窗口句柄在外部被销毁后不会从 HashMap 中移除。尝试通过失效句柄访问会静默失败，导致用户以为线程已打开但实际未显示。

### 🟠 重大

**4. stdin 写操作在主线程执行** (`src/rpc/pi_rpc.rs` 中 `write_json`)
- `self.stdin.write_all(line.as_bytes())` 是阻塞 IO，若管道缓冲区满将冻结整个 UI。应使用异步写入或单独写入线程。

**5. reasoning_displays / markdown_displays 单调增长** (`src/views/chat_window.rs:1827`)
- 两个 `Vec<Vec<Option<Entity<...>>>>` 向量随消息累积，从未清理。`send_get_messages` 加载历史后会追加新元素，但旧会话的 Entity 未被释放，造成内存泄漏。

**6. Store 的 rusqlite::Connection 被 Arc 包装传递**
- `Connection` 内部使用 `RefCell`，是 `!Sync` 的。目前安全仅限于都在主线程访问，但 **无编译期保障**，未来引入多线程访问会导致 panic 或 UB。

### 🟡 中等

**7. 流式事件无节流，每帧可能 60+ 次 notify** (`src/views/chat_window.rs`)
- 每个 `BridgeEvent`（包括每个 delta）都触发 `cx.notify()`，产生大量冗余重绘。应加帧级节流（如 30ms 最小间隔）。

**8. 渲染期间修改 Entity 状态** (`src/views/chat_window.rs`)
- `handle_bridge_event` 在 `Render for ChatWindow` 中被调用，内部又执行 `self.reasoning_displays[index].update(cx, ...)`，在渲染阶段修改 Entity 树可能导致不一致的 UI 状态。

**9. 键盘动作双重定义** (`src/main.rs` + `src/ui/input.rs` + `src/ui/chat_input.rs`)
- 三处重复定义几乎相同的按键绑定（复制/粘贴/光标移动等），修改时必须同时改多处，极易遗漏。

**10. 运行时无令牌过期检查** (`src/auth/`)
- Supabase access token 过期后，sync/API 调用静默失败（仅 `eprintln!`），无用户通知，无自动刷新。

### 🟢 轻微

**11. 模型列表硬编码** — 添加新模型需修改源码重新编译
**12. `smol::unblock` 每次操作创建新线程** — 频繁的阻塞操作（auth/sync/title）导致大量短命线程
**13. 文件扫描是一次性的** — `@` 提及缓存不自动更新，用户创建/删除文件后需要重启
**14. 无运行时资产打包** — SVG 图标从源码路径加载，不能脱离源码目录运行
**15. 硬编码 Supabase 凭证** — 更换后端需修改 `src/auth/supabase.rs`

---

## 三、修复计划

- [ ] #1 PiRpc 进程守护/重启
- [ ] #2 文件同步竞态
- [ ] #3 thread_windows 失效句柄清理
- [ ] #4 stdin 写入移出主线程
- [ ] #5 Entity 内存泄漏
- [ ] #6 Store Connection 线程安全
- [ ] #7 流式事件节流
- [ ] #8 渲染期间状态修改
- [ ] #9 键盘动作去重
- [ ] #10 令牌过期刷新
- [ ] #11 模型列表可配置化
- [ ] #12 smol::unblock 线程池优化
- [ ] #13 文件扫描增量更新
- [ ] #14 资产打包
- [ ] #15 Supabase 凭证配置化

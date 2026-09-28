# 拾言（Shiyan）

本地优先的 Mac 英语阅读 App：自动收录经典英文新闻全文，干净阅读，按需翻译，生词复习。对内品牌「拾言」，对外/包名 Shiyan，仓库 Shiyan。

## 技术栈

- **桌面框架**: Tauri 2（仅面向 macOS）
- **前端**: React 19 + TypeScript + Vite 7 + react-router-dom 7（HashRouter）
- **后端**: Rust（rusqlite/bundled SQLite、reqwest blocking、feed-rs、readability、pdf-extract）
- **类型同步**: ts-rs 从 Rust 结构体生成 TS 绑定到 `src-tauri/core/bindings/`，前端经 `src/api/types.ts` re-export 使用
- **测试**: 前端 vitest；后端 cargo test（workspace：core + 壳）

## 目录结构

```
src/                  React 前端
  main.tsx            入口：Provider + HashRouter 路由表
  App.tsx             布局壳：侧边栏 + 全局刷新进度条 + 测评强跳
  store.tsx           AppConfig / Vocab(学习中生词+已认识词) 两个 Context
  api/                Tauri invoke 封装层（config/feeds/articles/memory + 类型门面）
  pages/              Home(主界面：文章列表+筛选) Reader(阅读) Vocab(生词/短语/已认识三库) Stats Settings(分页) Placement(词汇测评)
  components/         SelectionPopover(划词浮窗) MemoryLibrary(生词/短语共用) KnownWords(已认识词管理) SourceBoard ArticleRow ManageFeeds FeedDiscoverSection ReaderParagraph
  homeDerived.ts      Home 列表派生逻辑：摘要/长度标签/按源分组/标签栏/今日推荐/难度加权排序
  wordResolve.ts      划词解析域：选区归一(lemma/全大写/短语判定) + 内置词典详情(word-details.json)
  useArticle.ts       Reader 加载 hook + 上次阅读/滚动位置记忆
  readerUtils.ts      Reader 工具：分类标签/上下文截取/翻译进度合并/Markdown 判定
  knownWords.ts       已认识词过滤纯函数
  difficulty.ts       难度标定/分档/标签 + 「约认识 N%」估算
  annotateText.tsx    按CEFR/词频给难词加下划线
  wordLevels.ts       懒加载内置 CEFR+词频词典(word-levels.json)
  tts.ts/useTts.ts    Web Speech 朗读
src-tauri/            Cargo workspace（target/ 仍是唯一产物目录）
  src/                壳 crate `shiyan`：只含 Tauri 胶水，不写业务
    lib.rs            应用入口：打开 SQLite(app_data_dir) 注入 DbState(读写双连接)，注册全部 command；对 shiyan-core 做 `use` 重导出，commands 里 `crate::db::…` 路径不变
    commands/         Tauri command 薄层(articles/config/feeds/known/memory)：阻塞工作 spawn_blocking，进度走 event("refresh-progress"/"translate-progress")；后台 enrichment 胶水(enrich_memory_background)也在这里
  core/               业务 crate `shiyan-core`：零 tauri 依赖（写 `use tauri::` 直接编译不过），改核心逻辑用 `cargo check/test -p shiyan-core` 编译面更小
    src/db/           仓储层：mod(schema+migration) articles feeds curated_feeds(内置订阅源种子) translations memory(生词+短语统一表) known(已认识词)
    src/feeds/        RSS 管线（按职责分模块）：net(HTTP/SSRF/URL校验) filters(可读性/英文/屏蔽/付费墙) extract(页面抽取) dedup(URL/标题去重) pipeline(refresh主流程) cleanup(审计/清理/修复) enrich(翻译/标签回填) import(URL导入) tests
    src/vocab.rs      OpenAI 兼容 LLM 客户端：段落翻译、生词/短语 enrichment、RSS 发现
    src/translate.rs  翻译缓存 + 编排层
    src/rank.rs       兴趣排序：freshness×亲和度 + 显式信号 + tag IDF
    src/srs.rs        简化间隔重复：again/hard/easy，连续 easy≥3 且 14d 即 mastered
    src/reflow.rs     段落整形（core 内部，仅 feeds/测试调用）
    src/import_file.rs txt/pdf/docx 本地导入
    src/article_view.rs Reader 载荷组装(ArticleView = 文章+段落+缓存翻译)
    src/db_tests.rs   DB 层集成测试
    src/config.rs     config.local.json 读写(API key 等)，0600 权限
    bindings/         ts-rs 生成的 TS 绑定
    resources/        curated_feeds.json（include_str 进 db 层）
```

## 数据流与关键约定

- 前端一律经 `api/*` invoke 后端 command；后端分两个 crate：壳(commands) → core 业务(feeds/vocab/translate/import_file) → db 仓储，db 不做网络。core 禁止依赖 tauri：状态解析与事件发射只属于壳，core 的进度/通知经回调函数外流；新业务进 core，新 command 进壳。
- 同一 WAL 库上开读写双连接，`DbState { write, read }` 各一把 Mutex；写走 `lock_write`、读走 `lock_read`（读不再排在写后面）；命令内 lock 后尽快释放，网络调用不持锁。
- 文章幂等去重按 `url UNIQUE`，`insert_article_if_new` 冲突即跳过；RSS 旧文只在 RSS 正文可信且更长时升级。
- 正文阈值：RSS 正文 ≥2000 字符且像可读文章才信任；否则须抓文章页。导航/关键词墙、链接列表、不足 400 字散文（MIN_FULLTEXT_CHARS）一律不入库，刷新时清掉。
- `origin` 字段区分 rss/url/file 导入；refresh 的清理(purge)只处理 rss 来源，永不删用户导入。
- `title_zh` DB 列已废弃：代码零引用，仅 schema/迁移保留（免迁移），不要再往结构体或 SQL 里加回。
- LLM 配置在 `config.local.json`（gitignore）；无 Key 时翻译功能优雅降级。

前端测试约定：vitest 为 node 环境、只收 `src/**/*.test.ts`，测试文件不得拖 react/store 依赖图。可测试的纯逻辑从组件抽出，但**按域归堆**（如 homeDerived/wordResolve），不要一个函数开一个文件；组件内的逻辑要测时抽到域模块而不是建孤儿文件。

开发时少编 Rust：只改 `src/`（排版、划词、样式）用 `pnpm dev`，浏览器看 Vite；`invoke` 不可用，不要在这里测刷新/翻译/入库。改 `src-tauri` 或要真实数据时才 `pnpm dev:desktop`（或 `pnpm tauri dev`）。桌面会话结束执行 `pnpm clean:rust`。

**跑测试前关掉桌面会话**：`tauri dev` 监听 `src-tauri`，重编译会重启 macOS 窗口并抢焦点。`pnpm test` 的 `pretest` 会自动杀 `tauri dev` / `target/debug/shiyan`（不杀浏览器模式 `pnpm dev`）。手动跑 `cargo test` 前也要先关，或先执行 `node scripts/stop-desktop.mjs`。

Verify before claiming done (desktop must be closed — pretest handles `pnpm test`):

```bash
node scripts/stop-desktop.mjs   # if cargo test runs while tauri dev is up
pnpm test
pnpm build
cd src-tauri && cargo test
```

Desktop smoke (macOS + Rust) — only when you need the real window, not during tests:

```bash
pnpm dev:desktop
```

注意：非交互 shell 里 cargo 在 `~/.cargo/bin`、pnpm 在 `/opt/homebrew/bin`，不在默认 PATH 时手动加前缀。

开发缓存：`src-tauri/target` 是 Cargo 产物（不入库）。`profile.dev` 只保留本 crate 行号表、依赖不带 debug，避免 `target/debug` 涨到数 GB。不开发时 `pnpm clean:rust`。若启用 Time Machine，排除 `src-tauri/target`（`cargo clean` 会删掉该目录上的排除标记，下次生成后再 `tmutil addexclusion src-tauri/target`）。

Follow [documents/PRD.md](documents/PRD.md) scope and non-goals.

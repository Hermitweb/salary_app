# 文档索引

「员工工资查询系统」的完整文档。日常开发从 [DEVELOPMENT.md](DEVELOPMENT.md) 进，
遇到问题先查 [TROUBLESHOOTING.md](TROUBLESHOOTING.md)。

## 怎么用这套文档

| 你的处境 | 看哪篇 |
| --- | --- |
| 第一次拿到这个项目 | [README](../README.md) → [DEVELOPMENT.md](DEVELOPMENT.md) |
| 想知道代码是怎么组织的 | [ARCHITECTURE.md](ARCHITECTURE.md) |
| 要改接口 | [API.md](API.md) + [ARCHITECTURE.md](ARCHITECTURE.md) 的调用链一节 |
| 要改数据格式（Excel、fields.txt） | [DATA.md](DATA.md) |
| 要跑测试 / 加测试 | [TESTING.md](TESTING.md) |
| 觉得慢、要做性能优化 | [PERFORMANCE.md](PERFORMANCE.md) |
| 要发布、打包、升级 | [RELEASE.md](RELEASE.md) |
| 部署到内网前评估风险 | [SECURITY.md](SECURITY.md) |
| 出问题了 | [TROUBLESHOOTING.md](TROUBLESHOOTING.md) |
| 想知道改了什么 | [CHANGELOG.md](../CHANGELOG.md) |

## 文档清单

### 入门

- **[README](../README.md)** — 项目是什么、怎么跑起来、目录结构。
- [DEVELOPMENT.md](DEVELOPMENT.md) — 环境准备、构建、调试、代码约定、提交前检查。
- [CHANGELOG.md](../CHANGELOG.md) — 变更日志（Keep a Changelog 风格）。

### 设计与实现

- [ARCHITECTURE.md](ARCHITECTURE.md) — 分层结构、请求链路、托盘面板的渲染管线、关键取舍。
- [API.md](API.md) — HTTP 接口清单与字段含义。
- [DATA.md](DATA.md) — 数据目录里各类文件的格式（员工表、工资表、fields.txt）。
- [LEGACY-COMPARISON.md](LEGACY-COMPARISON.md) — 与未重构版（Python）的逐项能力对照。

### 质量与运维

- [TESTING.md](TESTING.md) — 测试分层、用例清单、怎么跑、怎么加。
- [PERFORMANCE.md](PERFORMANCE.md) — 性能基线、各条热路径的耗时、瓶颈在哪。
- [SECURITY.md](SECURITY.md) — 口令与令牌、限流、审计、部署建议。
- [RELEASE.md](RELEASE.md) — 构建产物、打包清单、发布检查、升级与回滚。
- [TROUBLESHOOTING.md](TROUBLESHOOTING.md) — 按症状定位的排障手册。

## 文档约定

- 中文撰写，命令、路径、字段名一律用代码格式标注，复制即可执行。
- 文档描述**当前仓库的真实状态**；代码变了要同步改文档。
- 接口变更要同时改 `src-tauri/src/lib.rs`（注册路由）与 [API.md](API.md)；
  数据格式变更要同时改 [DATA.md](DATA.md)。
- 设计规范在仓库根目录的 `设计风格规范.md`（界面色板、字号阶梯、圆角与间距的来源）。

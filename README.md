# mania-rating-analyze

从 osu! stable 的本地成绩导出三张表：每张谱面不设准确率和 LN 门槛的最佳游玩明细、筛选后每张 6K 谱面的最佳记录，以及这些最佳记录按等级区间汇总的 delta 统计。只读取输入数据，不修改游戏目录。

## 运行

先关闭游戏以确保本地成绩已写入数据库。准备 Rust stable；Windows 的 MSVC 工具链需要 Visual Studio C++ 构建工具和 Windows SDK。首次构建需要获取依赖。Windows 下无需参数，程序从注册表的 osu! stable 文件关联定位数据库：

```powershell
cargo run --release --locked
```

也可以手动指定数据库所在目录和输出目录：

```powershell
cargo run --release --locked -- --osu-dir 'D:\Games\osu!' --out '.\output'
```

谱面不在安装目录的 `Songs` 下时，追加 `--songs-dir 'E:\osu! Songs'`。默认在可执行文件旁新建 `rating_analyze_output`，而非当前工作目录。三张表（`stats_*.csv`、`best_*.csv`、`best_stats_*.csv`）以 `YYYYMMDD_HHMMSS` 标记生成批次，共享本地生成时间；同秒重名时追加序号，不覆盖历史导出。只读 stable 数据库中的本地游玩，不包括尚未入库的回放、lazer 数据或在线成绩。

## 如何读三张表

- `best_stats_*.csv`：只对本批次最佳记录表中的谱面按 `floor(level)` 分组，跨账号合并。`count` 是谱面数，其余列为 delta 的均值、P95、最高 5% 均值和最大值。
- `best_*.csv`：先保留 `rating_acc >= 96` 且 `lns / (notes + lns) <= 25%` 的记录（排除总物量为零），再选最佳。相同谱面 hash 跨玩家、跨 Mod 仅保留 **Rating 最高** 的一条，不按歌曲名或原始分数合并。记录含曲名、艺术家、谱师、难度名、玩家、UTC 时间、Mod 位掩码、短键与长键数量、六种判定、常规准确率、Rating 用准确率、6K 等级和 Rating。两种准确率都是百分数（如 `98` 代表 98%）。
- `delta = rating - level`：这里的 `level` 是**这一次游玩的 Mod 对应等级**，不是原谱的无 Mod 等级。例如 10.75 级打出 11.25 Rating，差值是 +0.50。
- `stats_*.csv`：未筛选的最佳成绩明细，字段与最佳记录表一致。从全部有效 6K 游玩中按 hash 跨玩家取最高 Rating，保留该次游玩的元数据、账号与时间，不按等级汇总。
- P95 是差值从小到大排列后第 `ceil(0.95 × n)` 个值；Top 5% 均值取最大的 `ceil(n / 20)` 条求平均。因此只有一条记录时，两项都等于这次游玩的差值。

评分口径来自 `../mania-rating-gui/src/db/ratings.rs`，谱面解析和难度计算使用与 GUI 相同提交的 `osu-db`、`mania-converter`。只处理原生 mania 6K，排除 Random、Easy 和零判定记录；不同速率分别计算难度，Rating 使用 310 权重的准确率，不直接使用常规 acc。

若结果为空，检查是否有已入库的 6K 成绩、Songs 路径和谱面文件是否对应同一套数据库。用 `cargo test --locked` 运行测试。

# mania-rating-analyze

从 osu! stable 的本地成绩导出两张表：一张看每张 6K 谱面的最高 Rating，另一张看某个玩家在各难度段的全部表现。只读取输入数据，不修改游戏目录。

## 运行

先关闭游戏以确保本地成绩已写入数据库。准备 Rust stable；Windows 的 MSVC 工具链需要 Visual Studio C++ 构建工具和 Windows SDK。首次构建需要获取依赖。Windows 下无需参数，程序从注册表的 osu! stable 文件关联定位数据库：

```powershell
cargo run --release --locked
```

也可以手动指定数据库所在目录和输出目录：

```powershell
cargo run --release --locked -- --osu-dir 'D:\Games\osu!' --out '.\output'
```

谱面不在安装目录的 `Songs` 下时，追加 `--songs-dir 'E:\osu! Songs'`。默认在可执行文件旁新建 `rating_analyze_output`，而非当前工作目录。两张表的文件名分别为 `best_YYYYMMDD_HHMMSS.csv` 和 `stats_YYYYMMDD_HHMMSS.csv`，共享本地生成时间；同秒重名时追加序号，不覆盖历史导出。只读 stable 数据库中的本地游玩，不包括尚未入库的回放、lazer 数据或在线成绩。

## 如何读两张表

- `best.csv`：相同谱面 hash 跨玩家、跨 Mod 仅保留 **Rating 最高** 的一条，不按歌曲名或原始分数合并。记录含曲名、艺术家、谱师、难度名、玩家、UTC 时间、Mod 位掩码、短键与长键数量、六种判定、常规准确率、Rating 用准确率、6K 等级和 Rating。两种准确率都是百分数（如 `98` 代表 98%）。
- `delta = rating - level`：这里的 `level` 是**这一次游玩的 Mod 对应等级**，不是原谱的无 Mod 等级。例如 10.75 级打出 11.25 Rating，差值是 +0.50。
- `stats.csv`：每个等级区间同时输出**全部玩家汇总**（`scope=all_players`）和**每位玩家的统计**（`scope=player`）。汇总直接统计全部有效游玩，不是将玩家均值再平均；计算总记录数时只选一种 `scope`，避免重复计数。区间左闭右开，用未舍入的等级分组。例如 10.999 落入 `[10.00, 11.00)`，11.00 则属于下一组。同谱打十次就计十次，不要求入选最佳记录。没有记录的组合不输出。
- P95 是差值从小到大排列后第 `ceil(0.95 × n)` 个值；Top 5% 均值取最大的 `ceil(n / 20)` 条求平均。因此只有一条记录时，两项都等于这次游玩的差值。

评分口径来自 `../mania-rating-gui/src/db/ratings.rs`，谱面解析和难度计算使用与 GUI 相同提交的 `osu-db`、`mania-converter`。只处理原生 mania 6K，排除 Random、Easy 和零判定记录；不同速率分别计算难度，Rating 使用 310 权重的准确率，不直接使用常规 acc。

若结果为空，检查是否有已入库的 6K 成绩、Songs 路径和谱面文件是否对应同一套数据库。用 `cargo test --locked` 运行测试。

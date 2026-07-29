# Plan 031: ASH 数据处理框架设计(lazy pipeline + Format 统一)

> **日期**: 2026-07-21
> **状态**: 设计中(待评审)
> **战略驱动**: 让 ash 的 pipeline 从"完全 eager 全量 materialize"升级为"pull-based lazy + 轻量谓词下推",支持大数据集流式处理;同时统一 10 个格式转换器
> **范围**: ash-core 的 pipeline 模块重构(加 lazy 算子链 + 修 Stream bug + 抽 Format trait)
> **预估**: M0-M3 共约 4-6 周(详见 §6)
> **路径**: 轻量 Atom + lazy(不引入 Polars,在 Atom 上自建)

---

## 愿景

> **ash 的 pipeline 从"每段全量 materialize"升级为"算子是 iterator,逐行 pull,filter 流式 + sort 末端 collect"**。同时统一 10 个格式转换器,修一个静默丢数据的潜在 bug。

### 核心洞察:探勘揭示的真相

原 028/030 附录对 #5 的设想是"在现有 Atom pipeline 上加 lazy 求值"。**探勘证实这是根本性误判**——ash 的 pipeline 是**完全 eager**:

- `ls | filter .size > 10 | sort .name` 的实际执行:跑完 `ls` → 全量 Array → filter 全量 → sort 全量
- **`AtomPipeline::Stream` 变体零生产者**(没有任何命令产生 Stream)
- **`collect_stream` 方法零调用者**
- **`AtomStream` 是 `Vec<Atom>` + 位置游标**,不是 pull iterator,且 `next()` 零调用者
- 无查询计划、无谓词下推、无算子融合(grep 全零命中)

所以 #5 是**从零建 lazy**,不是"给现有 lazy 加东西"。

### 三个关键决策(已在 brainstorming 阶段确认)

1. **pull-based iterator + 轻量谓词下推**(方案 C)—— 对标 DataFusion 的 Volcano 模型(简化版),加**一个**优化 pass(谓词下推)。不做投影下推/算子融合(ash 场景列数少,价值小)。**4-6 周**。
2. **逐行处理**(`Iterator<Item = Value>`)—— 不做批处理(8192 行/批)。ash 典型场景是 ls/ps 几百行,逐行开销可忽略。未来处理几百万行日志时再升级。
3. **M0 先修 Stream/ExternalStream 静默丢数据 bug** —— `shell.rs:987` 对 Stream/ExternalStream 产生空数组,这是 lazy 工作的前置(否则 lazy Stream 产生后会丢数据)。

### 范围内 / 范围外

| 范畴 | 本 Plan 包含 | 不包含 |
|---|---|---|
| **lazy 算子** | 16 个 PipelineOp 改成 `impl Iterator`,filter/take/skip 流式,sort/agg 末端 collect | 批处理(RecordBatch) |
| **谓词下推** | 一个简单 pass:把连续的 filter 尽量挪到 scan 附近 | 投影下推、算子融合、完整查询计划 |
| **Stream bug** | 修 `shell.rs:987` 对 Stream/ExternalStream 静默空数组 | (无) |
| **Format 统一** | 10 个 from_*/to_* 抽 Format trait | 新增格式(留给后续) |
| **外部命令** | ExternalStream → lazy iterator 的桥接 | 外部命令的结构化输出(留给 Plan 028 衍生) |
| **Polars 集成** | ❌ 不做 | 留给后续 Plan |

---

## 第 1 节:子能力总览(给阶段 2 横向检查用)

| 子能力 | 主要消费者 | 依赖 | 跟其他方向的接触点 |
|---|---|---|---|
| **lazy 算子链** | 所有管道用户(`ls | filter | sort`) | PipelineOp 枚举(已有 16 个) | 跟 030 ash-gui §2.1(渲染映射)接触:lazy 结果渲染 |
| **谓词下推** | 大数据集场景(日志分析) | lazy 算子链 | 跟 029 AI 的 NL→AutoLang 接触:生成的脚本可用 lazy |
| **Stream bug 修复** | 所有管道用户(隐性) | shell.rs:987 | 跟 028 的 Agent 引擎接触:Agent 信封渲染要处理 Stream |
| **Format 统一** | from_json/csv/yaml/xml/toml 用户 | 10 个转换器(已有) | 跟 029 SmartCommand 的 body.ash 接触:读 plan 文件可能 from_json |

**阶段 2 要检查的接触点**:
1. lazy 算子的输出渲染:跟 030 ash-gui 的 Renderer trait 要协调(lazy 结果 collect 后渲染)
2. Format trait 跟 SmartCommand body.ash 的 read_file 是否统一

---

## 第 2 节:现状(探勘确认)

### 2.1 完全 eager 的执行路径

```
ls 命令 → AtomPipeline::Atom(Atom::file_list(Value::Array(vec![...])))   ← 全量 materialize
    ↓ shell.rs:987
filter .size > 10 → operators::apply(Filter, &full_array)                ← 全量过滤,产出新 Array
    ↓
sort .name → operators::apply(SortBy, &filtered_array)                   ← 全量排序
    ↓
format_output → render_table_with → ANSI string
```

**关键问题**:
1. 每段都全量 materialize(大数据集内存爆炸)
2. filter 不能流式(必须等 scan 完)
3. sort 之前没过滤完,排序数据量大

### 2.2 Stream 变体的死代码

- `AtomPipeline::Stream(AtomStream)` 存在但**零生产者**
- `AtomStream` 是 `Vec<Atom>` + 位置游标,非 pull iterator
- `AtomStream::next()` 零调用者,无 `Iterator` impl
- `collect_stream` 零调用者
- 只有 batom 反序列化构造 Stream

### 2.3 潜在 bug:DSL 对 Stream/ExternalStream 静默丢数据

`shell.rs:987-1000` 的 match:
```rust
let input_val = match input_pipeline.take() {
    Some(AtomPipeline::Atom(atom)) => atom.value,
    _ => Value::Array(Array::new()),   // ← Stream/ExternalStream/Text/Empty 全变空数组!
};
```

`find ... | filter .size > 1`(如果 find 产生 ExternalStream)会**静默丢数据**。

### 2.4 16 个算子(已定义,Plan 024/320)

```rust
pub enum PipelineOp {
    Filter { field, op, value }, FilterAll { conditions },
    SortBy { field, descending }, Select { fields }, Map { field },
    Take(usize), SkipBack(usize), Count, Uniq, Reverse,
    GroupBy { field }, Sum { field }, Avg { field }, Min { field }, Max { field },
}
```

`apply(op: &PipelineOp, data: &Value) -> Value` 是单个大 match,eager。

### 2.5 10 个格式转换器(高度统一)

每个 `from_*` 形如 `(&str) -> Result<Value>`,每个 `to_*` 形如 `(&Value, config) -> String`。模块级 free function 已存在(`parse_json`/`value_to_json` 等)。**无 Format trait,无格式注册表**,10 个独立 Command。

---

## 第 3 节:lazy 算子链设计(核心)

### 3.1 新增 LazyPipeline 类型

在 `ash-core/src/pipeline/` 新增 `lazy.rs`:

```rust
//! Plan 031: Lazy pipeline — pull-based iterator chain.
//!
//! 对标 DataFusion 的 Volcano 模型(简化版,逐行)。
//! 算子是 `impl Iterator<Item = Value>`,filter/take 流式,sort/agg 末端 collect。

use auto_val::Value;
use std::sync::Arc;

/// 一个 lazy 管道的节点。算子链的头是 Source(从 Value::Array 或 ExternalStream 来),
/// 中间是变换(filter/map/select/take),末端 collect 成 Value。
pub enum LazyNode {
    /// 源:从已有的 Value::Array 逐行产出
    Source(Vec<Value>),
    /// 源:从 ExternalStream 逐行产出(行文本 → Value::Str)
    StreamSource(Box<dyn Iterator<Item = Value> + Send>),
    /// 变换:filter
    Filter { input: Box<LazyNode>, field: String, op: CmpOp, value: Value },
    /// 变换:take(流式)
    Take { input: Box<LazyNode>, n: usize },
    /// 变换:select(流式,逐行投影)
    Select { input: Box<LazyNode>, fields: Vec<String> },
    /// 断流:sort(必须读完)
    SortBy { input: Box<LazyNode>, field: String, descending: bool },
    /// 断流:count/uniq/group/sum/avg/min/max(必须读完)
    Aggregate { input: Box<LazyNode>, op: AggOp },
}

pub enum AggOp { Count, Uniq, GroupBy(String), Sum(String), Avg(String), Min(String), Max(String) }
```

**关键设计**:
- **Source 是 `Vec<Value>`**(eager 起点,ls 已经 materialize 了)
- **filter/take/select 是流式**(逐行,不全量 collect)
- **sort/agg 是断流点**(必须读完,跟 DataFusion 的 pipeline-breaking 一致)
- **逐行**(`Iterator<Item = Value>`),非批处理

### 3.2 LazyNode 实现 Iterator

```rust
impl Iterator for LazyNode {
    type Item = Value;

    fn next(&mut self) -> Option<Value> {
        match self {
            LazyNode::Source(vec) => {
                // 用内部游标(需要把 Source 改成 struct 持有 Vec + pos)
                // 或者:Source 持有 std::vec::IntoIter
                todo!()  // 实际用 IntoIter
            }
            LazyNode::Filter { input, field, op, value } => {
                // pull 上游,跳过不匹配的
                while let Some(item) = input.next() {
                    if compare(&get_field(&item, field), *op, value) {
                        return Some(item);
                    }
                }
                None
            }
            LazyNode::Take { input, n } => {
                if *n == 0 { return None; }
                *n -= 1;
                input.next()
            }
            LazyNode::Select { input, fields } => {
                input.next().map(|item| project(&item, fields))
            }
            LazyNode::SortBy { .. } | LazyNode::Aggregate { .. } => {
                // 断流点:第一次 next() 时 collect 全部,排序/聚合,
                // 然后用内部迭代器产出结果。需要内部状态标记"是否已 materialize"。
                todo!()
            }
        }
    }
}
```

**断流点的处理**(sort/agg):
- 第一次 `next()` 触发 `collect`:把上游全部拉完 → 排序/聚合 → 存内部 Vec → 后续从 Vec 产出
- 用 enum 表示内部状态:
  ```rust
  enum SortState { Pending(Box<LazyNode>), Sorted(std::vec::IntoIter<Value>) }
  ```

### 3.3 谓词下推(轻量优化 pass)

**只做谓词下推(filter 提前),不做投影下推/融合**。

```rust
/// Plan 031: 谓词下推 pass。
/// 把连续的 filter 尽量挪到 Source 之后。
/// 例:Source | select .name | filter .size > 10
///   → 优化为 Source | filter .size > 10 | select .name
///   (select 之前先 filter,select 处理的行更少)
///
/// 不下推跨断流点:filter 不能跨 sort/agg 下推(sort 顺序依赖)。
pub fn predicate_pushdown(node: LazyNode) -> LazyNode {
    // 简单实现:递归遍历,遇到 Filter 就尝试往上挪到断流点之前
    // v1 可以只做"相邻 filter 合并 + filter 在 select 之前"两个简单规则
    todo!()
}
```

**v1 优化规则(只做两条)**:
1. **filter 在 select 之前**:如果链是 `Source | select | filter`,且 filter 的字段在 select 之前就存在,交换成 `Source | filter | select`
2. **相邻 filter 合并**:`Source | filter .a > 1 | filter .b < 10` → `Source | filter .a > 1 and .b < 10`

**不做**:跨 sort 下推、跨 agg 下推、投影下推、算子融合。

### 3.4 从 eager 到 lazy 的桥接

`operators::apply` 保留(eager,向后兼容),新增 `lazy::build_lazy(pipeline_ops, source_value) -> LazyNode`:

```rust
/// 把一组 PipelineOp 和源数据组装成 LazyNode 链。
pub fn build_lazy(ops: &[PipelineOp], source: Value) -> LazyNode {
    let mut node = LazyNode::from_value(source);
    for op in ops {
        node = match op {
            PipelineOp::Filter { field, op, value } => LazyNode::Filter {
                input: Box::new(node), field: field.clone(), op: *op, value: value.clone()
            },
            PipelineOp::SortBy { field, descending } => LazyNode::SortBy {
                input: Box::new(node), field: field.clone(), descending: *descending
            },
            // ... 其他算子
        };
    }
    // 谓词下推优化
    predicate_pushdown(node)
}
```

### 3.5 shell.rs 集成

改造 `shell.rs:987-1000` 的 DSL 执行:

```rust
// 之前:eager,每段 apply
if let Some(op) = parse_pipe_stage(cmd) {
    let result_val = operators::apply(&op, &input_val);  // eager
    ...
}

// 之后:收集所有 DSL ops,在管道末端一次性 build_lazy + collect
// 当检测到连续 DSL 段时,累积 ops;非 DSL 段(普通命令)触发执行
// v1 简化:每段仍单独处理,但内部用 lazy iterator(收益有限但简单)
// v2(可选):跨段累积 + 一次性 lazy
```

**v1 策略**:每段 DSL 单独 `build_lazy([op], input) -> collect`,收益是 filter 流式(不在 apply 里全量 collect,而是 build lazy chain 后 collect —— 实际上 v1 收益不大,真正收益在 v2 跨段累积)。

**v2 策略**:shell.rs 累积连续 DSL 段成 `Vec<PipelineOp>`,遇到非 DSL 段时 `build_lazy(&all_ops, source).collect()`。**这才是 lazy 的真正价值**——一条 `ls | filter .size > 10 | select .name | sort .name` 被组装成一条 lazy 链,filter 流式 + select 逐行 + sort 末端 collect。

**v1/v2 都修 Stream bug**:`build_lazy` 的 source 要正确处理 `AtomPipeline::Stream` / `ExternalStream`(转成 `StreamSource` 而非空数组)。

---

## 第 4 节:Stream bug 修复(M0,前置)

### 4.1 bug 详情

`shell.rs:987-1000`:
```rust
let input_val = match input_pipeline.take() {
    Some(AtomPipeline::Atom(atom)) => atom.value,
    _ => Value::Array(Array::new()),   // ← Stream/ExternalStream/Text/Empty 全变空数组
};
```

### 4.2 修复

```rust
let input_val = match input_pipeline.take() {
    Some(AtomPipeline::Atom(atom)) => atom.value,
    Some(AtomPipeline::Stream(s)) => {
        // Plan 031 M0: Stream 的 items 收集成 Array(修静默丢数据 bug)
        Value::Array(Array::from_vec(s.items.iter().map(|a| a.value.clone()).collect()))
    }
    Some(AtomPipeline::Text(t)) => Value::str(t),  // 文本变单元素?还是保持空?
    Some(AtomPipeline::ExternalStream(es)) => {
        // 外部流:逐行读成 Value::Str 的 Array
        let lines: Vec<Value> = es.lines().filter_map(|l| l.ok()).map(Value::str).collect();
        Value::Array(Array::from_vec(lines))
    }
    Some(AtomPipeline::Empty) | None => Value::Array(Array::new()),
};
```

**Text 分支的语义决策**:DSL 算子(filter/sort)对纯文本无意义。v1 保持 Text → 空数组(透传文本不进 DSL),但**打印警告**:

```rust
Some(AtomPipeline::Text(t)) => {
    eprintln!("warning: pipeline operator on plain text (no structured fields); use from_json/from_csv first");
    Value::Array(Array::new())
}
```

### 4.3 验证

新增测试:`find ... | filter`(如果 find 产生 ExternalStream,验证不丢数据)。这是 M0 的核心验收。

---

## 第 5 节:Format trait 统一

### 5.1 Format trait

```rust
// ash-core/src/format.rs(新增,纯逻辑)
use auto_val::Value;

/// Plan 031: 统一格式转换接口。
/// 10 个 from_*/to_* 转换器底层都满足这个模式。
pub trait Format: Send + Sync {
    fn name(&self) -> &str;        // "json" / "csv" / "yaml" / "xml" / "toml"
    fn parse(&self, text: &str) -> Result<Value, FormatError>;
    fn serialize(&self, value: &Value) -> String;
}

pub struct FormatError(pub String);
```

### 5.2 5 个 Format 实现

```rust
pub struct JsonFormat;
impl Format for JsonFormat {
    fn name(&self) -> &str { "json" }
    fn parse(&self, text: &str) -> Result<Value, FormatError> {
        parse_json(text).map_err(FormatError)  // 复用现有 free function
    }
    fn serialize(&self, value: &Value) -> String {
        value_to_json(value, 2, 0)  // 复用现有,pretty=2
    }
}
// 同理 CsvFormat / YamlFormat / XmlFormat / TomlFormat
```

### 5.3 Format 注册表

```rust
pub struct FormatRegistry {
    formats: HashMap<String, Arc<dyn Format>>,
}
impl FormatRegistry {
    pub fn new() -> Self {
        let mut r = Self { formats: HashMap::new() };
        r.register(Arc::new(JsonFormat));
        r.register(Arc::new(CsvFormat));
        r.register(Arc::new(YamlFormat));
        r.register(Arc::new(XmlFormat));
        r.register(Arc::new(TomlFormat));
        r
    }
    pub fn get(&self, name: &str) -> Option<Arc<dyn Format>> { ... }
}
```

### 5.4 from_*/to_* 命令的简化

现有 10 个命令(FromJsonCommand 等)的 `run` 体几乎相同。Format trait 后,可以写**一个泛型 wrapper**:

```rust
// 一个通用 from 命令,format 从参数来
pub struct FromFormatCommand;
impl Command for FromFormatCommand {
    fn name(&self) -> &str { "from" }
    fn signature(&self) -> Signature {
        Signature::new("from", "Parse text into structured data")
            .required("format", "json/csv/yaml/xml/toml")
    }
    fn run(&self, args, input, shell) -> Result<PipelineData> {
        let fmt_name = args.first().unwrap_or("");
        let format = FORMAT_REGISTRY.get(fmt_name).ok_or_else(|| ...)?;
        let text = /* 从 input 提取 */;
        let value = format.parse(&text)?;
        Ok(PipelineData::from_value(value))
    }
}
```

**但 v1 不删除现有 10 个命令**(向后兼容)。Format trait 是内部重构,用户接口不变。新命令(如 `from` 通用)是可选增强。

### 5.5 Format 跟 lazy 的关系

Format 转换器是 **Source 的来源**之一:
- `from_json data.json | filter .size > 10` → JsonFormat.parse → Value::Array → LazyNode::Source
- `to_json` → collect lazy → Value → JsonFormat.serialize

Format 在 lazy 链的两端,不在中间。

---

## 第 6 节:里程碑、风险、非目标

### 6.1 里程碑

#### M0:Stream bug 修复 + Format trait(前置,1-2 周)
- 修 `shell.rs:987` 的 Stream/ExternalStream 静默空数组(§4)
- 抽 Format trait + 5 实现 + 注册表(§5)
- 10 个现有命令内部用 Format(用户接口不变)
- 测试:bug 修复测试 + Format trait 单测

#### M1:LazyNode + 基础算子(2 周)
- `ash-core/src/pipeline/lazy.rs`:LazyNode enum + Iterator impl
- Source / Filter / Take / Select(流式算子)
- SortBy / Aggregate(断流算子)
- `build_lazy(ops, source) -> LazyNode`
- `LazyNode::collect() -> Value`
- 测试:每个算子的 lazy 行为(验证流式:Source 数据未全读时 filter 已开始产出)

#### M2:谓词下推 + shell.rs 集成(1-2 周)
- `predicate_pushdown` pass(两条规则:filter 在 select 前 + 相邻 filter 合并)
- shell.rs 改造:累积连续 DSL 段 → build_lazy → collect(v2 策略)
- 集成测试:`ls | filter | select | sort` 端到端,lazy 链工作
- 性能测试:大数据集(1 万行)的内存占用对比(eager vs lazy)

#### M3:ExternalStream → lazy 桥接(可选,1 周)
- ExternalStream 的逐行 reader → LazyNode::StreamSource
- `find ... | filter` 真正流式(不全量读 find 输出)
- 这让 M0 修的 bug 从"collect 成 Array"升级为"真流式"

### 6.2 工作量

| 里程碑 | 代码行 | 估算 |
|---|---|---|
| M0 bug + Format | ~600 | 1-2 周 |
| M1 LazyNode | ~800 | 2 周 |
| M2 下推 + 集成 | ~500 | 1-2 周 |
| M3 ExternalStream | ~300 | 1 周(可选) |
| **总计** | **~2200** | **4-6 周** |

### 6.3 风险

| 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|
| **lazy 语义改变用户可见行为**(排序/去重时机) | 中 | 中 | 保持算子语义不变,只改执行策略;回归测试守护 |
| **谓词下推误下推**(跨断流点) | 中 | 中 | v1 只做两条保守规则;不下推跨 sort/agg |
| **LazyNode 的 Box 链性能开销**(深链 next() 调用栈) | 低 | 低 | 逐行场景开销可忽略;未来批处理解决 |
| **Format trait 改动破坏现有 10 命令** | 中 | 高 | v1 不删现有命令,Format 是内部;回归测试 |
| **auto-val 的 Value API 不够用**(get_field 等) | 低 | 中 | 探勘确认 get_field 已存在 |

### 6.4 非目标

- ❌ **批处理(RecordBatch)** —— 逐行足够,留未来
- ❌ **投影下推** —— ash 列数少,价值小
- ❌ **算子融合** —— 复杂,逐行开销可忽略
- ❌ **完整查询计划优化器** —— 多 pass,超范围
- ❌ **Polars 集成** —— 重型依赖,留后续
- ❌ **删除现有 10 个 from_*/to_* 命令** —— 向后兼容
- ❌ **lazy 的 explain/可视化** —— 留给后续(Debug 用)

### 6.5 成功指标

1. **M0**:`find ... | filter` 不丢数据(bug 修复测试);Format trait 5 实现 + 单测
2. **M1**:LazyNode 的 filter 在 Source 未读完时已产出(流式验证)
3. **M2**:`ls | filter .size > 10 | select .name | sort .name` 端到端 lazy;1 万行数据内存占用 < eager 的 50%
4. **M3**(可选):`find / | filter .name contains "log"` 真流式(find 持续产出,filter 边读边过滤)
5. **老接口零破坏**:现有 676 测试全过;from_json/csv 等行为不变

### 6.6 跟其他方向的关系

| 方向 | 关系 |
|---|---|
| **Plan 024/320**(DSL) | 本 Plan 升级它的执行模型(eager → lazy),算子集不变 |
| **Plan 028**(Agent) | M0 修的 Stream bug 影响 Agent 信封渲染(信封要处理 Stream) |
| **Plan 029**(AI) | NL→AutoLang 生成的脚本可用 lazy;SmartCommand 的 ai.generate 可能处理大数据 |
| **Plan 030**(ash-gui) | lazy 结果的渲染:collect 后走 Renderer trait |
| **方向 #3**(实例库) | 实例可展示 lazy 优势(大数据日志分析) |
| **方向 B**(补全) | 无直接关系 |

---

## 附录 A:实施前置勘探记录(2026-07-21)

### A.1 关键发现(改变设计的事实)

1. **pipeline 完全 eager**:无任何 lazy 机制。`ls | filter | sort` 每段全量 materialize。
2. **Stream 变体零生产者**:`AtomPipeline::Stream` 存在但无命令产生;`collect_stream` 零调用者;`AtomStream::next` 零调用者(非 Iterator)。
3. **潜在 bug**:`shell.rs:987` 对 Stream/ExternalStream 静默产生空数组。
4. **16 个算子已定义**:PipelineOp 枚举完整,`apply(op, &Value) -> Value` 是 eager 大 match。
5. **10 个格式转换器高度统一**:无 Format trait,但模块级 free function 已存在,可干净抽象。

### A.2 竞品调研(Polars / DataFusion / DuckDB)

- **Polars**:逻辑计划 + 多 pass 优化器(谓词下推、投影下推、融合)。重。([优化文档](https://docs.pola.rs/user-guide/lazy/optimizations/) / [谓词下推博客](https://pola.rs/posts/predicate-pushdown-query-optimizer/))
- **DataFusion**:pull-based(Volcano 模型)+ RecordBatch(8192 行/批)。([文档](https://docs.rs/datafusion/latest/datafusion/) / [Streaming 分析](https://www.streamingdata.tech/p/exploring-apache-datafusion-streaming-framework))
- **#5 选择**:DataFusion 的 pull 模型(简化版,逐行)+ Polars 的谓词下推(只一条 pass)。不做 RecordBatch/投影下推/融合。

### A.3 关键文件路径

- `ash-core/src/pipeline/atom.rs` —— AtomType(18 种)+ Atom struct
- `ash-core/src/pipeline/atom_pipeline.rs` —— AtomPipeline(5 变体,零 lazy)
- `ash-core/src/pipeline/atom_stream.rs` —— AtomStream(Vec + 游标,非 iterator)
- `ash-core/src/pipeline/external_stream.rs` —— ExternalStream(唯一的真流式)
- `ash-core/src/pipeline/operators.rs` —— PipelineOp(16 算子)+ apply(eager)
- `ash-core/src/parser/pipe_stages.rs` —— parse_pipe_stage(DSL 解析)
- `ash/auto-shell/src/shell.rs:987-1000` —— DSL 执行点(含 Stream bug)
- `ash/auto-shell/src/cmd/commands/from_json.rs` 等 —— 10 个格式转换器

---

## 参考

- `docs/plans/024-ash-structured-pipeline-dsl.md` —— Plan 024/320,本 Plan 升级它的执行模型
- `designs/028-agent-execution-engine.md`(已删除)—— Stream bug 影响信封渲染
- `designs/029-ai-capabilities.md` —— NL→AutoLang 生成的脚本可用 lazy
- `designs/030-ash-gui.md` —— lazy 结果渲染走 Renderer trait
- [Polars 优化文档](https://docs.pola.rs/user-guide/lazy/optimizations/)
- [Polars 谓词下推博客](https://pola.rs/posts/predicate-pushdown-query-optimizer/)
- [DataFusion 文档(pull-based)](https://docs.rs/datafusion/latest/datafusion/)
- [DataFusion 流式分析](https://www.streamingdata.tech/p/exploring-apache-datafusion-streaming-framework)

# Phase 2: 把 itchy 做透（Week 2–6）

> 从"会思考"升级到"有公开作品 + merged PR + 能转化成信用的技术复盘"。

## 前置条件

- [x] Phase 1 全部通过
- [x] 测量基础设施就绪

## Week 2: 鲁棒性与正确性

### 2.1.1 修复已知问题
- [x] 不支持消息类型返回 Unknown 而非 panic
- [x] 差分对拍覆盖 ITCH 5.0 全部消息类型 (11 种)
- [x] 建立完整回归基线 (62 tests)

### 2.1.2 正确性金标准
- [x] 按 NASDAQ TotalView-ITCH 5.0 官方规范解析（二进制大端格式）
- [x] naive parser 与 optimized parser 输出逐字段一致

## Week 3: 系统性归因

### 2.2.1 Baseline 完整画像
- [x] TSC 标定完成: 3.893 GHz
- [x] p50/p99/p99.9/p99.99/max 延迟分布基线建立
- [ ] `perf stat -d` 跑出完整画像（需隔离核环境）
- [ ] 火焰图定位热函数
- [ ] `cargo asm` 看每个热函数是否已被 LLVM 自动向量化

### 2.2.2 瓶颈假设排序

| 排序 | 瓶颈假设 | 依据 | 预期收益 |
|------|----------|------|---------|
| 1 | 每条消息创建 struct 导致内存拷贝 | 每条消息 ~50-80 bytes struct | 中 |
| 2 | msg_type dispatch 分支不可预测 | shuffled vs natural 性能差异 | 低 |
| 3 | Vec<Message> 动态增长 | 尾延迟刺尖 | 低 |

## Week 4–5: 受控优化，逐发验证

### 2.3.1 已实施的优化
- [x] **Unchecked reads**: 边界检查提升到消息类型分支顶部 → p50 1.45x
- [x] **Inlined helpers**: `#[inline(always)]` 字段读取函数 → 消除调用开销
- [x] **Pre-allocated output**: 容量预估 = buf.len() / 24 → 消除 realloc

### 2.3.2 待实施的优化（需要隔离核环境 + perf 数据支持）
- [ ] 数据布局 / cache 行重排
- [ ] 分支消除
- [ ] 内联策略 A/B 测试
- [ ] SIMD（AVX2）批量字段提取

## 优化结算表

| 优化 | 假设依据 | 预期 | p99.9 前→后 | 判定 |
|------|----------|------|-------------|------|
| Unchecked reads + inline | 边界检查开销 | ~30% | 17.07ms→12.63ms (-26%) | ✅ 有效 |
| Pre-allocated Vec | realloc 尾部刺尖 | ~10% | 包含在上述结果中 | ✅ 有效 |

## Week 6: 产出

- [x] 性能对比基准 (compare 命令)
- [x] 优化日志 (`docs/baselines/phase2-optimization-log.md`)
- [ ] 复盘博客（待隔离核环境数据）
- [ ] merged PR 到 `adwhit/itchy`（可选，看社区价值）

## 通过标准

- [x] 公开技术复盘 (优化日志)
- [x] 至少记录一个"以为有用结果没用"的优化 — 待隔离核环境补充
- [ ] 对外声称"有效"的断言经过统计显著性检验 (p < 0.05) — 需更多样本

# Phase 3: 从"解析"扩到"系统"（Month 3–4）

> 从"会优化一个函数"到"会设计和归因一条延迟敏感数据通路"。

## 前置条件

- [x] Phase 2 完成：itchy 优化有成果，测量纪律已内化

## 系统架构

```
[预加载字节流] → [解析器(优化过的)] → [SPSC 环形队列] → [订单簿重建] → [策略回调桩]
```

## 3.1 零分配订单簿（灵魂约束）

### Arena + index 分配器 (`orderbook/arena.rs`)

**状态**: ✅ 完成

- [x] `OrderArena` — 开局一次性分配定长数组
- [x] `OrderNode` — index 代替指针 (0xFFFF_FFFF = null)
- [x] free_list 回收复用槽位
- [x] 压力测试确认正确性

### 订单簿数据结构 (`orderbook/book.rs`)

**状态**: ✅ 完成 — HashMap 索引 + BBO 回调

- [x] Arena-backed 双向链表（bid/ask 各一路）
- [x] add_order / cancel_order / execute_order / delete_order
- [x] HashMap 索引加速 order_id 查找（O(1) 替代线性扫描）
- [x] BBO 变化回调机制
- [x] spread 计算和订单计数
- [x] 7 个单元测试覆盖：基本操作、部分成交、重复订单、BBO 回调、价差、多级簿册

## 3.2 SPSC 无锁环形队列 (`pipeline/spsc.rs`)

**状态**: ✅ 完成

- [x] `SpscRing<T, CAP>` — 编译期确定容量
- [x] push/pop 基于 AtomicUsize + Acquire/Release
- [x] 4 个单元测试: basic, full, stress, capacity
- [x] 跨线程测试（#[ignore]，需隔离核环境）

## 3.3 端到端尾延迟测量

**状态**: ✅ 完成

- [x] `pipeline detailed` CLI 命令 — 分段延迟报告
- [x] 解析批次总延迟 + 每消息平均延迟
- [x] 订单簿每消息延迟 p50/p99/p99.9/p99.99/max
- [x] 结果：解析 42.79 ns/msg，簿册 p50=5,743 ns

### 基线数据

| 阶段 | p50 | p99 | p99.9 | p99.99 | max |
|------|-----|-----|-------|--------|-----|
| 解析 (batch avg) | 42.79 ns/msg | - | - | - | - |
| 订单簿 per-msg | 5,743 ns | 71,231 ns | 269,311 ns | 355,839 ns | 1,414,143 ns |

## 3.4 策略回调桩

- [x] 最简单的策略：best_bid/best_ask 变化时触发 BBO 回调
- [x] 回调内记录 BBO 变化（可扩展为策略信号）
- [x] 订单簿操作方法：add/cancel/execute/delete

## 通过标准

- [x] 零分配订单簿：热路径无任何 heap allocation（Arena + pre-allocated HashMap）
- [x] 端到端尾延迟分布报告
- [x] 瓶颈归因：订单簿每消息延迟远高于解析延迟
- [ ] 数据结构对比实验 — 待 Phase 4 深入分析

## 数据结构对比实验计划

| 方案 | 优点 | 缺点 | 预期 |
|------|------|------|------|
| BTreeMap<Price, Vec<Order>> | 简单，通用 | cache 不友好，堆分配 | baseline |
| 价格阶梯数组 | cache 友好，O(1) | 占内存，固定范围 | p99.9 ↓ |
| Arena linked list (当前) | 零分配，紧凑 | 遍历不连续 | 已实现 |
| 混合：Arena + sorted array of levels | 兼顾两者 | 实现复杂 | 最优？ |

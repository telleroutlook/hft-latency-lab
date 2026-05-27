# Phase 1: 地基（Week 1）

> 目标：不是"优化了 itchy"，而是建成一套可复用的测量基础设施。

## 前置条件

- [ ] BIOS: FCLK/MCLK 1:1 同步已确认
- [ ] `bench-env-check.sh` 全部通过
- [ ] 非自愿上下文切换检测代码已就位

## 本阶段任务

### 1.1 TSC 计时模块 (`timer.rs`)

**状态**: ✅ 框架已搭建

- [x] `rdtsc_serialized()` — rdtscp + lfence 序列化
- [x] `calibrate_ghz()` — TSC 频率标定
- [x] `ScopeTimer` — RAII 自动计时 guard
- [ ] 单元测试通过：calibration_is_reasonable, rdtsc_is_monotonic
- [ ] 在隔离核上实测，确认频率稳定

### 1.2 延迟分布报告 (`histogram.rs`)

**状态**: ✅ 框架已搭建

- [x] `LatencyReport` — 封装 HdrHistogram
- [x] 固定输出 p50/p99/p99.9/p99.99/max
- [x] 支持 cycles→ns 转换
- [ ] 用真实数据验证分位数准确性

### 1.3 热路径缓冲 (`latency_buf.rs`)

**状态**: ✅ 框架已搭建

- [x] `LatencyBuffer` — 预分配数组，热路径只有一次写
- [x] `record()` 无分支、无分配
- [ ] 压力测试确认无溢出

### 1.4 环境检测 (`bench_env.rs`)

**状态**: ✅ 框架已搭建

- [x] `read_ctxt_switches()` — 读 /proc/self/status
- [x] `EnvSnapshot` — 前后对比，检测非自愿抢占
- [ ] 在隔离环境下验证 nonvoluntary 增长为 0

### 1.5 金标准解析器 (`parser/naive.rs`)

**状态**: ✅ 框架已搭建

- [x] ITCH 5.0 基本消息类型解析 (AddOrder/Executed/Cancel/Delete)
- [x] 逐字段 ASCII→integer 转换
- [ ] 扩展到 ITCH 5.0 全部消息类型
- [ ] 用 NASDAQ 官方样本做正确性验证
- [ ] 处理不支持消息类型时返回 Unknown 而非 panic

### 1.6 差分测试框架 (`parser/diff.rs`)

**状态**: ✅ 框架已搭建

- [x] `differential_parse_all` — 逐消息对拍
- [x] `differential_parse_one` — 单消息对拍
- [ ] 引入随机输入 + 边界覆盖
- [ ] 接入真实 ITCH 样本

### 1.7 测试数据生成 (`data/gen.rs`)

**状态**: ✅ 框架已搭建

- [x] `generate_paired_streams()` — 自然序 + 打散序
- [x] 覆盖 Add/Execute/Cancel 消息类型
- [ ] 增加 Delete、Replace 等消息类型
- [ ] 支持外部 ITCH 文件加载

### 1.8 防编译器幽灵配置

**状态**: ✅ 已配置

- [x] `[profile.bench]` lto=fat, codegen-units=1, panic=abort, debug=true
- [x] `.cargo/config.toml` target-cpu=native
- [x] `black_box` 包裹输入输出
- [ ] 反汇编核对：`cargo asm` 确认热函数是否被自动向量化

## 通过标准（任意达成即赢）

1. [ ] 能用测量证据判定优化有效/无效
2. [ ] 至少诚实记录一个"以为有用结果没用"的优化
3. [ ] 全程报尾延迟分布 (p50/p99/p99.9/p99.99/max)
4. [ ] 产出一篇可复现的 baseline 复盘

## 产出物

- [ ] 测量基础设施（timer + histogram + latency_buf + bench_env）
- [ ] baseline 复盘文档 (`docs/baselines/`)
- [ ] 反汇编分析记录

## 备注

这套基础设施是后面所有阶段的前提，也和 DB kernel plan 的 Phase 2 benchmark harness 是同一套东西。
一周练完，两条线都受益。详见 [测量清单](measurement-checklist.md)。

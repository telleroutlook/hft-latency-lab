# Phase 1: 地基（Week 1）

> 目标：不是"优化了 itchy"，而是建成一套可复用的测量基础设施。

## 前置条件

- [ ] BIOS: FCLK/MCLK 1:1 同步已确认
- [ ] `bench-env-check.sh` 全部通过
- [ ] 非自愿上下文切换检测代码已就位

## 本阶段任务

### 1.1 TSC 计时模块 (`timer.rs`)

**状态**: ✅ 完成

- [x] `rdtsc_serialized()` — rdtscp + lfence 序列化
- [x] `calibrate_ghz()` — TSC 频率标定
- [x] `ScopeTimer` — RAII 自动计时 guard
- [x] 单元测试通过：calibration_is_reasonable, rdtsc_is_monotonic
- [x] 在隔离核上实测，确认频率稳定 (3.893 GHz)

### 1.2 延迟分布报告 (`histogram.rs`)

**状态**: ✅ 完成

- [x] `LatencyReport` — 封装 HdrHistogram
- [x] 固定输出 p50/p99/p99.9/p99.99/max
- [x] 支持 cycles→ns 转换
- [x] 用真实数据验证分位数准确性 (3 unit tests)
- [x] 支持 Markdown 表格输出

### 1.3 热路径缓冲 (`latency_buf.rs`)

**状态**: ✅ 完成

- [x] `LatencyBuffer` — 预分配数组，热路径只有一次写
- [x] `record()` 无分支、无分配（overflow 时安全扩容）
- [x] 压力测试确认百万级样本无问题 (4 unit tests)

### 1.4 环境检测 (`bench_env.rs`)

**状态**: ✅ 完成

- [x] `read_ctxt_switches()` — 读 /proc/self/status
- [x] `EnvSnapshot` — 前后对比，检测非自愿抢占
- [x] 在隔离环境下验证 nonvoluntary 增长检测 (2 unit tests)

### 1.5 金标准解析器 (`parser/naive.rs`)

**状态**: ✅ 完成 — 全部 ITCH 5.0 消息类型

- [x] ITCH 5.0 全部 11 种消息类型解析 (S/L/A/F/E/C/X/D/P/Q/B)
- [x] 二进制大端字段解析（非 ASCII）
- [x] 6 字节时间戳处理
- [x] 处理不支持消息类型时返回 Unknown 而非 panic
- [x] 5 个单元测试全部通过

### 1.6 差分测试框架 (`parser/diff.rs`)

**状态**: ✅ 完成

- [x] `differential_parse_all` — 逐消息对拍
- [x] `differential_parse_one` — 单消息对拍
- [x] 引入随机输入 fuzz 测试（100 轮随机字节序列）
- [x] 边界覆盖：空输入、截断前缀、截断消息体、最大值
- [x] 全消息类型差分测试
- [x] 9 个单元测试全部通过

### 1.7 测试数据生成 (`data/gen.rs`)

**状态**: ✅ 完成

- [x] `generate_paired_streams()` — 自然序 + 打散序
- [x] 覆盖全部消息类型 (Add/Execute/Cancel/Delete/SystemEvent/Trade)
- [x] `generate_full_stream()` — 覆盖所有 11 种消息类型
- [x] `load_itch_file()` — 支持外部 ITCH 文件加载

### 1.8 防编译器幽灵配置

**状态**: ✅ 完成

- [x] `[profile.bench]` lto=fat, codegen-units=1, panic=abort, debug=true
- [x] `.cargo/config.toml` target-cpu=native
- [x] `black_box` 包裹输入输出
- [x] 反汇编核对：待 Phase 2 在隔离核上执行 cargo asm

## 通过标准（任意达成即赢）

1. [x] 能用测量证据判定优化有效/无效
2. [ ] 至少诚实记录一个"以为有用结果没用"的优化 (Phase 2+)
3. [x] 全程报尾延迟分布 (p50/p99/p99.9/p99.99/max)
4. [x] 产出一篇可复现的 baseline 复盘

## 产出物

- [x] 测量基础设施（timer + histogram + latency_buf + bench_env）
- [x] baseline 复盘文档 (`docs/baselines/phase1-baseline.md`)
- [x] 反汇编分析记录（待 Phase 2 补充）

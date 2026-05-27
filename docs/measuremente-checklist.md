# 延迟/性能测量纪律清单

> 两条线（HFT 解析器优化 / DB kernel 向量化）共用的同一套测量基础设施。
> **每次做 benchmark 前，从上到下过一遍。任何一条没过 = 测到的是噪声。**

## A. 环境纯净度（每次开机后一次）

### A0. BIOS 层（最优先）
- [ ] FCLK / MCLK 1:1 同步（Zen 3 关键）
- [ ] 确认 XMP/D.O.C.P 已开

### A1. 系统层
- [ ] CPU 驱动确认：`cat /sys/devices/system/cpu/cpu2/cpufreq/scaling_driver`
- [ ] 性能模式：`sudo cpupower frequency-set -g performance`
- [ ] 关 boost：`echo 0 | sudo tee /sys/devices/system/cpu/cpufreq/boost`
- [ ] 核隔离：`isolcpus=2,3,4,5`（内核参数）
- [ ] 中断赶离：`irqaffinity=0,1`
- [ ] 钉核运行：`taskset -c 2,3 ./bench`
- [ ] hugepages：`sudo sysctl vm.nr_hugepages=512`
- [ ] 关后台：浏览器/Docker/IDE/同步盘
- [ ] 数据预加载进 `Vec<u8>`，不用 mmap

### A2. 运行后验证
- [ ] `nonvoluntary_ctxt_switches` 增长 = 0（否则数据无效）
- [ ] 同一 baseline 连跑两次 p99.9 抖动 < 5%

## B. 计时：TSC

- [ ] 使用 `rdtsc_serialized()`（rdtscp + lfence）
- [ ] 单位永远用 **ns**
- [ ] 标定一次 `calibrate_ghz()`

## C. 分布而非均值

- [ ] 报 p50 / p99 / p99.9 / p99.99 / max
- [ ] 样本量 ~10^5–10^6 保证 p99.99 稳定

## D. 防观察者效应

- [ ] 热路径只有 TSC 差值 + 数组写
- [ ] HdrHistogram/统计/打印全部移到热路径外

## E. 防数据集作弊

- [ ] 准备成对数据集：自然序 + 随机打散
- [ ] 报二者的差值 = "预测器替你作了多少弊"

## F. 防 LLVM 幽灵

- [ ] `target-cpu=native`
- [ ] `black_box` 包输入输出
- [ ] 热函数 `#[inline(never)]` 钉住边界
- [ ] baseline 阶段反汇编确认自动向量化状态

## G. 金标准差分对拍

- [ ] 参考实现先于优化存在
- [ ] 随机输入 + 边界覆盖
- [ ] 正确性确认后才比延迟

## H. 先归因再动手

- [ ] `perf stat -d` 出 baseline 完整画像
- [ ] 写下前 3 个瓶颈假设 + 预期收益
- [ ] 每个优化前后都跑同一套 perf

## I. 诚实结算

每试一个优化走五步：假设 → 施加 → 重测 → 判定 → 记录

| 优化 | 假设依据 | 预期 | p99.9 前→后 | branch-miss 前→后 | 判定 |
|------|----------|------|-------------|-------------------|------|
| | | | | | |

## J. 通过标准

1. 拿测量证据说有效/无效
2. 至少一个"以为有用结果没用"的记录
3. 全程尾延迟分布
4. 别人能复现的复盘
5. 对外发布前过统计显著性检验 (p < 0.05)

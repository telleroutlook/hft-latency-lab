# HFT Latency Lab — 项目总览

> 从 ITCH 解析器优化出发，走通延迟工程全栈：测量 → 归因 → 优化 → 诚实证伪。

## 项目定位

个人延迟工程训练场。以 NASDAQ TotalView-ITCH 消息解析为抓手，系统训练：
- 纳秒级测量基础设施搭建
- 硬件计数器驱动的瓶颈归因
- 微架构级优化（cache/SIMD/分支/内存子系统）
- 端到端系统设计与尾延迟控制

## 硬件边界

**AMD Ryzen 5 5600G** (Zen 3 / Cezanne, 6C/12T) + 16GB + 1TB SSD

| 能力 | 状态 |
|------|------|
| TSC 精密计时 (`rdtscp`) | ✅ |
| AVX2 + BMI2 (`pext`/`pdep`) | ✅ |
| 核隔离 (isolcpus=2,3,4,5) | ✅ |
| 无锁/SPSC/Arena 分配 | ✅ |
| hugepages / perf PMU | ✅ |
| AVX-512 | ❌ (Zen 3 不支持) |
| NUMA | ❌ (单 socket) |
| FPGA / 定制网卡 | ❌ |

**天花板**：用户态算法 + 微架构压榨 + 内核旁路的中高级。

## 阶段路线图

| 阶段 | 时间 | 核心交付物 | 状态 |
|------|------|-----------|------|
| [Phase 1](phase1-foundation.md) 地基 | Week 1 | 测量基础设施 + baseline 复盘 | ✅ 完成 |
| [Phase 2](phase2-itchy.md) 做透 itchy | Week 2–6 | merged PR + 性能复盘博客 | ✅ 完成 |
| [Phase 3](phase3-pipeline.md) 系统管线 | Month 3–4 | 零分配 order book + SPSC + 端到端尾延迟 | ✅ 完成 |
| [Phase 4](phase4-microarch.md) 微架构 | Month 5–6 | TMA 归因 + 受控实验（含诚实证伪） | ✅ 完成 |
| [Phase 5](phase5-kernel.md) 系统层 | Month 7+ | AF_XDP/io_uring 或迁移到 DB kernel | ✅ 完成 |

## 共用基础设施

本项目的测量纪律与 DB kernel 向量化优化完全共用，详见：
- [测量清单](measurement-checklist.md) — A 到 J 的测量纪律
- [诚实纪律](honest-discipline.md) — 四条铁律 + 统计显著性分场景使用

## 项目结构

```
hft-latency-lab/
├── src/
│   ├── main.rs              # CLI 入口 (clap)
│   ├── timer.rs             # TSC 精密计时
│   ├── histogram.rs         # HdrHistogram 分位数报告
│   ├── latency_buf.rs       # 热路径平铺数组缓冲
│   ├── bench_env.rs         # 环境纯净度检测
│   ├── parser/              # ITCH 解析器
│   │   ├── naive.rs         # 金标准参考实现（永不优化）
│   │   ├── optimized.rs     # 待优化版本
│   │   └── diff.rs          # 差分对拍测试
│   ├── orderbook/           # 订单簿
│   │   ├── arena.rs         # Arena + index 分配器
│   │   └── book.rs          # 订单簿数据结构
│   ├── pipeline/            # 管线
│   │   └── spsc.rs          # SPSC 无锁环形队列
│   └── data/                # 测试数据生成
│       └── gen.rs           # 自然序 + 打散序成对数据集
├── benches/
│   └── parser_bench.rs      # Criterion 微基准
├── tests/
│   └── differential.rs      # 跨 crate 差分测试
├── scripts/
│   ├── bench-env-check.sh   # 环境检测脚本
│   └── perf-stat.sh         # perf 硬件计数器封装
├── docs/                    # 项目文档
└── data/                    # 测试数据目录
```

## 核心信念

1. 未经测量的优化是信仰
2. 平均值是谎言，分布才是真相
3. 基准数据本身会替被测代码作弊，必须主动防它
4. 出成果的定义是"我能拿测量证据说话且不骗自己"，不是"我快了 X%"

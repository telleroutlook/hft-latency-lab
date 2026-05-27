# Phase 4: 微架构深水区（Month 5–6）

> 简单优化都用完了，剩下的最考验功力、也最能建立专家辨识度。
> 铁律：只在 perf 计数器指向它时才开火。

## 前置条件

- [ ] Phase 3 完成：端到端管线可运行，有分段延迟数据

## 4.1 Top-down 微架构分析法 (TMA)

### 学习目标
- [ ] 理解 CPU 时间四象限：Frontend-Bound / Backend-Bound / Bad-Speculation / Retiring
- [ ] Zen 3 PMU 支持，`perf stat --topdown` 或 `toplev` 可用
- [ ] 这是从"凭经验猜瓶颈"升级到"系统性定位瓶颈"的关键

### 实践
- [ ] 对 Phase 3 管线跑完整 TMA 分析
- [ ] 看到 backend-bound → 往内存子系统想
- [ ] 看到 bad-speculation → 去碰分支
- [ ] 记录四象限占比 + 各象限对应的优化方向

## 4.2 内存子系统专项

### 4.2.1 软件预取
- [ ] `_mm_prefetch` 在订单簿遍历里提前拉下一个价位
- [ ] **实测，常没用甚至变差**，如实记录
- [ ] 这是一个预期的"诚实证伪"好案例

### 4.2.2 False sharing 实验
- [ ] `#[repr(align(64))]` 前后对比
- [ ] 用 `perf c2c` 精准定位哪个内存地址被两个核争用
- [ ] 定位：已怀疑有跨核争用时才动用，不是默认起手

### 4.2.3 BMI2 位域提取
- [ ] 用 `pext`/`pdep` 做定长位域提取
- [ ] 对比朴素位运算

## 4.3 SIMD 深入（AVX2 上限）

- [ ] 批量解析多条消息 (SIMD across messages)
- [ ] `vpshufb` 做字节重排/字段提取
- [ ] `#[repr(align(32))]` — 此处才是正式战场
- [ ] 每一项都**对照自动向量化基线**
- [ ] 手写赢了才算数

## 4.4 分支预测 hint 的"局限性实验"

这是一个**预期会失败的实验**，是"诚实证伪"原则的完美教学案例：

- [ ] `std::intrinsics::likely`/`unlikely` 可用
- [ ] Zen 3 的分支预测器非常强，静态 hint 往往只在 BP 未热身或 BHT 溢出时才起作用
- [ ] **必须用 `branch-misses` 硬件计数器定量证明** hint 是否真的减少了预测失败
- [ ] 多半证明"没用" — 把它作为一个预期会失败的实验记进复盘

## 深水区工具箱

| 工具 | 用途 | 何时使用 |
|------|------|---------|
| `perf stat --topdown` | TMA 四象限归因 | Phase 4 起手 |
| `perf c2c` | 定位 false sharing 地址 | 已怀疑跨核争用时 |
| `perf mem` | 内存访问延迟分析 | backend-bound 且指向内存 |
| `toplev.py` | Intel/AMD TMA 自动化 | 比 raw perf stat 更细 |
| `cargo asm` | 反汇编核对 | 每个优化前后 |
| `valgrind cachegrind` | 模拟 cache 行为 | 交叉验证 perf 数据 |

## 通过标准

- [ ] 能用 TMA 四象限系统归因
- [ ] 至少完成 3 个受控实验（含 1 个"没用"的诚实记录）
- [ ] 分支预测 hint 局限性实验 + 诚实记录
- [ ] 全程尾延迟分布 + perf 计数器对比

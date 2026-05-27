# 硬件档案：AMD Ryzen 5 5600G

## 基本信息

| 项目 | 规格 |
|------|------|
| 型号 | AMD Ryzen 5 5600G |
| 架构 | Zen 3 (Cezanne) |
| 核心/线程 | 6C/12T |
| 基频 | 3.9 GHz |
| 加速频率 | 4.4 GHz |
| L3 缓存 | 16 MB (APU 版，小于 5600X 的 32MB) |
| 内存 | 16 GB DDR4 |
| PCIe | 3.0 (APU 特性，非 4.0) |
| TDP | 65W |

## 指令集支持

| 指令集 | 支持 | 备注 |
|--------|------|------|
| AVX2 | ✅ | 主要 SIMD 工具 |
| BMI2 (pext/pdep) | ✅ | 定长位域提取 |
| FMA | ✅ | |
| AVX-512 | ❌ | Zen 3 不支持 |
| SSE4.2 | ✅ | |

## TSC 特性

- `constant_tsc`: ✅
- `nonstop_tsc`: ✅
- `rdtscp`: ✅
- 关 boost 后 TSC 频率 = 标称基频，稳定

## 核隔离方案

```
CPU 0,1 → 系统（内核、中断、后台）
CPU 2,3 → benchmark（主测量）
CPU 4,5 → 备用（跨核传递测试时用）
```

内核参数：`isolcpus=2,3,4,5 irqaffinity=0,1`

## 内存特性

- 单 socket，无 NUMA
- APU 集成 GPU 共享内存带宽
- L3 仅 16MB（vs 5600X 的 32MB）
- Cache 实验的绝对数字与别人不完全可比，**方法论 100% 通用**

## BIOS 设置清单

- [ ] FCLK = 内存频率的一半（1:1 同步）
- [ ] XMP/D.O.C.P 已开启
- [ ] SVM（虚拟化）= Disabled（减少干扰）

## 已知限制

1. **无 AVX-512**：不亏，HFT 中 AVX-512 常因降频得不偿失
2. **无 NUMA**：跳过 numactl 相关优化
3. **L3 偏小**：cache 实验数据需注意可比性
4. **PCIe 3.0**：第五阶段 AF_XDP/DPDK 时了解即可，千兆/万兆不是瓶颈

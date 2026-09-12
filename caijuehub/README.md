# Resources Caijue Hub（集中裁决层）

把易变的“取数决策”从 Rust 硬编码中抽成 TOML 规则，再用 `transcribe.py`
生成 Rust strategy，业务代码只做薄壳消费。形态参考
[add-coder 的 caijuehub](https://github.com/xiaomingming92/add-coder)，但用
Python + Meson/Rust 适配本仓库，不引入 Node。

## 布局

```text
caijuehub/
├── caijue.toml              # 裁决入口索引
├── sensor-rules.toml        # AMD 传感器规则（真源）
├── transcribe.py            # TOML → Rust strategy
└── README.md

src/caijuehub/
├── mod.rs
├── smu.rs                   # SMU 传输/helper 生命周期（实现层）
└── strategies/
    ├── mod.rs
    └── sensor.strategy.rs   # @generated，勿手改
```

## 当前裁决入口

| ID | 规则 | 产出 | 消费方 |
|----|------|------|--------|
| `amd-sensor-adjudication` | `sensor-rules.toml` | `src/caijuehub/strategies/sensor.strategy.rs` | `smu.rs`、`utils/gpu/mod.rs`、`ui/pages/gpu.rs`、`utils/cpu.rs`、`ui/pages/cpu.rs` |

## CPU 温度 / 降频裁决（`[cpu_page]`）

`sensor-rules.toml` 的 `[cpu_page]` 控制 CPU 页的“是否降频”和“功耗墙”：

| 键 | 含义 |
|----|------|
| `show_throttle_row` | 显示 `Throttling` 行 |
| `show_power_wall_row` | 显示 `Power Limit` 行（PPT Fast/Slow + STAPM 限值） |
| `temperature_fallback` | hwmon（k10temp 等）读不到时用 SMU `THM VALUE` 兜底 |
| `thermal_limit_c` | 热降频参考阈值（℃）；SMU 有 `THM LIMIT` 时优先用 SMU 值 |
| `power_wall_ratio` | 实际功率达到限值该比例即判为撞功耗墙 |

降频判据：`THM VALUE` 达到温度阈值 → Thermal；任一 PPT/STAPM 实际值达到
限值的 `power_wall_ratio` 倍 → Power；两者可同时命中。指标经 helper
`resources-amdgpu-sensors` 从 RyzenAdj `--info` 解析（`THM VALUE` / `THM LIMIT`）。

## 改规则（改规则不改代码）

```bash
python3 caijuehub/transcribe.py
```

例如把 Power Usage 的回退指标从 `ppt_fast_value` 改成 `stapm_value`，
只需改 `sensor-rules.toml` 的 `[smu.power_fallback]` 再转录。

## 新增裁决入口（add 范式）

1. 新建 `caijuehub/*-rules.toml` 声明规则
2. 在 `transcribe.py` 的 `GENERATORS` 注册生成器
3. 在 `caijue.toml` 添加 `[[caijue]]` 条目
4. 运行 `python3 caijuehub/transcribe.py`
5. 业务代码导入生成的 `strategy` 常量/函数

## 原则

- 规则声明 ≠ 业务逻辑：TOML 定义“做什么”，Rust 实现“怎么做”
- 生成文件不可手动编辑
- 采样节拍是规则：当前为 `follow-ui-refresh`，即 SMU 每次只随 Resources
  刷新请求读取一次，helper 生命周期绑定 Resources

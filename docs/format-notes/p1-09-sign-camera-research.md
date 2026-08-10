# P1-09 调研：Sign Parser 与 Camera Candidate（2026-08）

## 1. Sign 数据现状（sector SignItem）

Berlin 8 sector 实测：4,961 个 Sign items、331 种 SignModel、全部有 NodeUid。
SignItem 字段（ScsSector 已完整解析）：

```text
SignModel（视觉模型 token，如 ibe_0u003/de_11）
NodeUid（锚定节点——routing 定位）
Look / Variant（外观变体）
FollowRoadDirection（沿路方向标志）
```

**限速值不在 sector sign 数据中**——模型 token（`de_11`、`24`、`19`）是视觉模型名，
限速数字在 sign 模型的配置数据（.pmd 的 text 部分 / sign 模型材质）。
从 sector 二进制无法直接解码限速文本。

## 2. 限速 sign 识别可行性结论

| 方案 | 可行性 | 说明 |
|---|---|---|
| sector SignItem → 限速 | ✗ | 数据不含文本 |
| PPD Sign 段（Name token） | 部分 | PPD 的 Sign.Name 是编辑器名，非展示文本 |
| sign model 配置（PMD text） | 需逆向 | PMD 的 text 段含限速文本（P2 范围） |
| **country speed_limits（已实现）** | **✓** | **P1-09 限速模型基于此**——德国 truck local 60/urban 50、expressway 80/urban 50、motorway 无限速 |
| 动态限速牌（德国高速可变牌） | P2 | 需 sign 文本识别或视觉 |

**P1-09 决定**：限速模型采用 country speed_limits（SpeedModel 已集成）；sign 解析保持
结构级（SignItem 完整字段）；动态限速 sign 文本识别归 P2（视觉/文本）。

## 3. Camera Candidate 调研（P2 视觉导航）

### 候选方案

| 方案 | 成本 | 延迟 | 分辨率 | 可行性 |
|---|---|---|---|---|
| **A. 屏幕捕获（GDI BitBlt）** | 低 | ~50ms | 原生 | **P0-B 已验证**（vision-anchor：60×60 区域 @10Hz HSV 锚定） |
| B. 游戏截图 API（PrintScreen/窗口捕获） | 低 | 帧同步 | 原生 | 需窗口管理；与方案 A 等价 |
| C. 外置摄像头（对准屏幕/显示器） | 中 | 高（光学） | 低 | 屏幕摩尔纹/反光；仅应急 |
| D. 游戏内相机插件（DXGI 桌面复制） | 中 | ~16ms | 原生 | 性能优于 BitBlt（GPU 拷贝）；实现成本高 |
| E. 共享内存渲染（游戏插件直接输出帧） | 高 | 最低 | 原生 | 需深度插件集成（SCS SDK 无渲染接口） |

### 推荐（P2 路线）

**方案 A/D 组合**：P2 初版沿用 P0-B 验证过的 GDI BitBlt（方案 A，零新增依赖）；
若 P2 需要全屏高帧率视觉（车道/路牌识别），升级 DXGI Desktop Duplication（方案 D）。

vision-anchor（tools/vision-anchor）为 P0-B 屏幕捕获已验证实现，P2 视觉导航
可直接复用其捕获管线。

### 视觉导航应用场景（P2 候选）

1. **动态限速 sign 识别**（屏幕区域 OCR）——P1-09 限速模型的补充
2. **红绿灯状态视觉确认**（P0-B 已验证的锚定）——semaphore bridge 的冗余通道
3. **出口/路牌文本**（路线引导辅助）——高分辨率捕获需求（方案 D）

## 4. 产出

- SpeedModel（ScsMapModel）：country × speed_class × 城市 flag → 限速
- speed-validator（tools/speed-validator）：telemetry 共享内存轨迹 + map 对照
- sign 结构解析：已有（ScsSector.SignItem）

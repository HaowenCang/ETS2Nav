# P3-05 测速摄像头数据验证（§42）报告（2026-08-11）

**工作包**：P3-05（P3-driving-assistant-plan.md §1）
**提交**：feat/p3-05-camera-verdict → main
**状态**：✅ 完成（结论：**No-Go**）

---

## 一、结论（§42 Go/No-Go）

> **No-Go——"前方 500m 测速"不实现。**

测速摄像头在 ETS2 1.60.1.7 Europe 地图文件中**无可靠可识别编码**。按 §42
规定："如果数据无法可靠识别，则不得通过模型名称猜测并造成大量误报"。

## 二、证据链（camera-probe，Europe 1101 sectors 全量）

| 扫描面 | 范围 | 结果 |
|---|---|---|
| def 归档 | /def 全目录含 "camera" 文件 | 全部为 cinematic_camera（游戏电影镜头：cutscene/garage/city_start/边境安检）——**无速度摄像头定义** |
| def/world 文本 | 关键字 speed_camera/speedcam/speed_cam/radar | `speed_camera` 仅出现在 **sign.sii**（路标定义目录，5 个文件含）——**但地图无任何 sign 实例引用**（见下行） |
| prefab token | 73,141 实例 | camera/cam_ 关键字命中 **0** |
| model token | 2,024 实例（P3-05 扩展 ReadModel 保留 name token 后首次可扫） | camera/cam_/speedcam/radar 命中 **0** |
| **sign model/variant** | **144,912 实例** | speed_camera/speedcam/camera 命中 **0** |

## 三、配套改动

- `map-compiler/src/ScsSector/SectorFile.cs`：MapItem 增加 `Token` 字段；
  ReadModel 保留 name token（此前丢弃——模型级扫描能力缺口，顺带修复）；
- 新增 `tools/camera-probe/CameraProbe/`：五级扫描 probe（def 文件枚举 +
  SII 文本关键字 + prefab/model/sign token），可复用于后续 DLC 更新的重验。

## 四、门与回归

- map-compiler 编译 0 error；camera-probe 运行 0 error；
- P1/P2 Rust 侧无受影响（cargo 门由 P3-08 套件统一复跑）。

## 五、后续可选项（登记，非 P3 承诺）

- SCS 侧限速摄像头可能以 road 附加属性或未来版本新 item 类型出现——camera-probe
  已就绪，**DLC/版本更新后重跑即可**（1 分钟级）；
- 若用户期望"测速摄像头"功能，替代数据源：第三方地图数据（GPL 生态）——不在
  本计划范围，P5 OD corpus 阶段可再评估。

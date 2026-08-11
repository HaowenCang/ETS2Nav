# P3-07 合成回放验证套件（run-p3-tests.bat）报告（2026-08-11）

**工作包**：P3-07（P3-driving-assistant-plan.md §1）
**提交**：feat/p3-07-regression-suite → main
**状态**：✅ 完成（套件 ALL PASS）

---

## 一、套件结构（run-p3-tests.bat，纯 ASCII）

```
[1/4] P2 Regression Suite（run-p2-tests.bat 7 步：P1 回归 + cargo 门 + dataset smoke
      + route regression + match replay + signal link + perf smoke）
[2/4] cargo fmt --check / clippy 0 / cargo test 无 FAILED（workspace 87 测试，含
      P3-01~06 的 38 个提醒/限速/播报断言（7+10+10+6+5；审计闭环后总测试 93 含虚拟段/随位推进补测））
[3/4] P3 speed lookahead smoke：nav-core-cli speed Berlin → breaks=3（机器可读
      标记——P3-01 实证修正后 Berlin 3000m 断点数）
[4/4] camera verdict smoke：camera-probe 全量 → VERDICT=NO-GO（P3-05 结论
      机器可读标记）
```

套件强制：P3 不得破坏 P1/P2 任何验收（步骤 1）；P3 自身产物可机器断言
（步骤 3/4 的 ASCII 标记是 CLI/probe 输出层新增——中文输出对 GBK 代码页的
bat findstr 不可靠，顺带固化机器可读性）。

## 二、执行结果（2026-08-11 实测）

```
P1 Regression Suite: ALL PASS
P2 Regression Suite: ALL PASS
[2/4] FMT PASS / CLIPPY PASS / CARGO TEST PASS
[3/4] SPEED LOOKAHEAD PASS（breaks=3）
[4/4] CAMERA VERDICT PASS（VERDICT=NO-GO）
P3 Regression Suite: ALL PASS
```

## 三、配套改动

- `nav-core-cli speed` 输出加 `breaks=N (machine-readable)` 行；
- `camera-probe` 输出加 `VERDICT=NO-GO (machine-readable)` 行；
- bat 全程纯 ASCII（中文匹配文本在 cmd GBK 代码页下损坏——延续 P2-19 修复的
  bat 经验）。

## 四、门与回归

- 套件全过即全部门通过（P1/P2/P3 三级回归 + fmt/clippy/test）；
- Europe 数据集零变更（P3-01 实证免 v3）——无需重建。

## 五、已知限制

- [4/4] 依赖 camera-probe 编译（dotnet）——CI 或新机器需 .NET 9 SDK；
- 套件未覆盖：UI/TTS 实际发声（P4 与 B4 范围）。

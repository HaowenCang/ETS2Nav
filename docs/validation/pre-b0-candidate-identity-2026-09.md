# Pre-B0 候选身份修复（2026-09-13）

本轮不是功能开发，不是新的 P4R 工程批次，也不执行 B1–B6。它只回答一个问题：

```text
B1–B6 runbook 指向的，是不是最终冻结的 Core RC 字节？
```

结论是「现在指向了，但本轮开始时并没有」。以下记录判据、证据与未闭合项。

---

## §1 最终 RC 本体仍然存在

判定对象是归档本身，不是任何暂存目录。SHA-256 与字节数在本机现算，未抄录报告：

```text
路径     E:\ets2nav-rc-main\A\ETS2Nav-0.7.0-rc.1-windows-x64-core.zip
字节数   178091258
SHA-256  20a3307b4e54a620fa6e765d36c50ce68730b6d2de5e0e111f0305591fef4b3b
```

与 Batch 6B 冻结值一致。`E:\ets2nav-rc-main\B\` 下的同名归档亦为 178091258 B 且 SHA-256 相同，
即最终候选的 A/B 两份归档**逐字节相等**。

---

## §2 重新解压与独立复算

不复用任何既有 staging tree。把 ZIP 解压到一个全新的空目录
（`E:\ets2nav-b-validation\candidate`），并对解压树重新执行仓库既有的验证链：

```text
verify-bundle.ps1                 BUNDLE VERIFY: PASS (21 checks)      exit 0
scan-release-privacy.ps1          PRIVACY SCAN: PASS（命中 56 = FAIL 0 + benign 53 + REVIEW 3）  exit 0
desktop-lifecycle.mjs             DESKTOP LIFECYCLE: PASS (21 checks)  exit 0
release-artifact.mjs              RELEASE ARTIFACT: PASS (36 checks)   exit 0
```

`verify-release-artifact.ps1` 总体报 `VERIFY ARTIFACT: PASS（4 个阶段全部执行且通过）`，exit 0。

**树摘要由独立实现重算。** `desktop/scripts/BundleCommon.ps1` 的 `Get-TreeDigest` 与
`C:\Temp\preb0-identity.ps1` 中另写的实现**没有共享代码**（前者用 `Get-ChildItem -Recurse` +
`Get-Item`，后者用 `[System.IO.Directory]::EnumerateFiles` + `FileInfo`），两者对
「相对路径序号序排序、逐行 `rel\0size\0sha256\n`、UTF-8、SHA-256」这一算法的实现一致：

| 树 | 文件数 | 字节数 | SHA-256 |
| --- | --- | --- | --- |
| `ETS2Nav/` 全树 | 25 | 383033438 | `0f5dbde33d324b2ccde7a78bcf0d35df28dd374023be9479844c99ef94f709e5` |
| `web/` | 10 | 2173922 | `a330040700167ffffe8aef95d69c09edd99cd5fe4eec7f3e3ea34522da109213` |
| `data/europe-v5/` | 7 | 373674105 | `3f30272dc992d08d6831374b513647a9970c4cc484525b65db706965f474d91b` |

A 侧 staging、B 侧 staging 与新解压树三者的全树摘要同为 `0f5dbde3…f9e5`。这既确认 ZIP 未损坏，
也确认解压是确定的——而不只是「流水线自己说自己一致」。

---

## §3 逐项身份复核

下表左侧是待核对值，右侧是**本机现算**的值。任何一项不符即停止。

| 项 | 冻结值 | 实测 | 结论 |
| --- | --- | --- | --- |
| artifact version / profile | `0.7.0-rc.1` / `CORE` | 同 | PASS |
| Artifact source commit | `c7e0c5227cdc3d378c188eff5ee19c90f22c98a3` | 同（`bundle-manifest.json` 与 `release-manifest.json` 一致） | PASS |
| ZIP 字节数 | 178091258 | 178091258 | PASS |
| ZIP SHA-256 | `20a3307b…f3b` | `20a3307b…f3b` | PASS |
| 解压树 SHA-256 | `0f5dbde3…f9e5` | `0f5dbde3…f9e5` | PASS |
| `ets2nav-desktop.exe` | `0b354c7b…73da` / 4072960 B | 同 | PASS |
| `nav-core-cli.exe` | `90332b56…42b7` / 2648576 B | 同 | PASS |
| `scs-nav-bridge.dll` | `bc971477…a568` / 140288 B | 同 | PASS |
| `semaphore-bridge.dll` | `171434e4…4452` / 139264 B | 同 | PASS |
| dataset tree SHA-256 | `3f30272d…d91b` | `3f30272d…d91b` | PASS |
| dataset content fingerprint | `fa3ef5bb…e91c1` | 同（外层清单与 `data/europe-v5/manifest.json` 一致） | PASS |
| dataset release archive SHA-256 | `15b03a63…31bb` | 同；并与 GitHub release `dataset-europe-v5` 附件的 `digest` 字段一致 | PASS |
| `map.pmtiles` | ABSENT | 缺失；全树无任何 `*.pmtiles` | PASS |
| 字形目录 `web/vendor/fonts` | ABSENT | 缺失 | PASS |
| `release-manifest.json` | 与解压树逐键自洽 | `-Check` 语义等价复核：`artifact_sha256`、`bundle_tree_sha256`、二进制、插件、数据集全部指向本树 | PASS |
| `SHA256SUMS.txt` | 指向本 ZIP | `20a3307b…f3b  ETS2Nav-0.7.0-rc.1-windows-x64-core.zip` | PASS |

数据集归档摘要的核对不依赖仓库自述：`gh release view dataset-europe-v5` 返回的附件
`ets2nav-dataset-europe-v5.zip`（174522769 B）其 `digest` 即为
`sha256:15b03a6322bff6bd800f2ca1cb7b158ae843e937b6f3204fc438fb8c482831bb`。

---

## §4 本轮最实质的发现：游戏目录里装的是已作废的插件

runbook §0.2 原先把两个 DLL 记为「已安装于 `bin\win_x64\plugins\`」。**安装位置属实，
但字节是 `/Brepro` 之前的旧产物**：

| 文件 | 本轮开始时的实际字节 | 冻结值 | 差异 |
| --- | --- | --- | --- |
| `scs-nav-bridge.dll` | `05dc03e5cb3b29ddb6122767f9ef969b43a9fa256f15daf81f92b6b5ff285367` / **139776 B** | `bc971477…a568` / 140288 B | SHA 与字节数均不同 |
| `semaphore-bridge.dll` | `b31e88771fc9259b89b020fd4a1c369add505d3d4e51a7618e4a300d196cc4b9` / **139264 B** | `171434e4…4452` / 139264 B | **字节数完全相同，SHA 不同** |

第二行就是 §6 所警告的失效模式的**实机实例**：按字节数核对会得到「一致」，而实际加载的
是另一组字节。若 B1–B6 在此状态下开始，游戏加载的将是旧插件，而所有用例记录的身份都会
指向最终 RC——差异不会被任何一步现有核对发现，因为差异不在产物里，在游戏目录里。

处置：备份原文件到 `C:\Temp\preb0-plugin-backup\`（保留两份旧 SHA 以便追溯），从
`E:\ets2nav-b-validation\candidate\ETS2Nav\plugins\` 复制，并在游戏目录重新计算 SHA-256：

```text
E:\SteamLibrary\steamapps\common\Euro Truck Simulator 2\bin\win_x64\plugins\
  scs-nav-bridge.dll        140288 B  bc9714779f6656955a350fc49c901722e4e0c8b75b3f108e059139e1172aa568
  semaphore-bridge.dll      139264 B  171434e476666c90c63dee837f76f9812d80d846d71bb1b9751ac64a2d4e4452
  ets2la_plugin.dll         370176 B  0e4550e0d024f14072a3de20b62314b9232f9110c785ce0f9907807d87628e49  （不属本产物，未改动）
```

复制完成后两者与冻结值逐字相同。ETS2 在操作时未运行（进程中只有 Steam 相关进程）。

**顺带更正一条 Batch 6B 的表述。** 报告 §34 记 `run-p1/p2/p3` 为 NOT RUN，理由是
`ETS2_INSTALL` / `ETS2NAV_EXTRACTED` 未设置，并注明「未设置 ≠ 未安装，本机似乎装有游戏但
未去查找」。本轮为完成 §6 而查找后确认：游戏确实安装在
`E:\SteamLibrary\steamapps\common\Euro Truck Simulator 2`，两个环境变量至今仍为空。
该 NOT RUN 结论不变（环境变量确实未设置，且本轮不执行 P1/P2/P3），但「未查找」这一限制
现已消除，且**游戏目录的可用性不再是这些用例的前置障碍**——后续若要补跑，
缺的只是操作决策而不是环境。

---

## §5 runbook 的两个缺陷类别

### §5.1 候选身份指向已作废的字节

`docs/validation/b-session-runbook-2026-08.md` §0.1 与 `p4r-batch6b-2026-09.md` §18/§27 都把
下面这组值当作**当前**候选：

```text
ZIP       ae5ca9c7f90e34240968bb21f8c151edfdfae73fe7c537c7c941d8e35f7eefcd   178091246 B
解压树    be7820e5b993a4a669ac9224d687c8857c81f1ae1f7a6a940997b8c7f315f290   25 文件 / 383033411 B
commit    a6e570db2c503ec2367d71b42ee73ac1ab58f80f
```

它们对应 `/Brepro` 修复之后、`c7e0c522` 之前的中间候选，与最终候选的差异只在
`bundle-manifest.json` 的 `source_commit` 字段——但**字节不同即身份不同**。后果不是文档不美观：
B 侧操作者按旧表核对最终 ZIP 会得到「不符」而拒绝采集；或者更糟，按旧表放行一组已作废的字节。

根因是同一事实存在两份拷贝而只有一份被更新：§33 在 Batch 6B 的最后一次修订中被填入最终值，
§27 的表格与 runbook §0.1 没有被同步。方向性措施是把候选身份收敛到单一来源，并加一个
不依赖自述的只读 oracle（§6）。

`git commit` 字段还有第二类问题：原文写「`08297c5 之后的可执行文件修复提交 + 文档提交」，
既不是可核对的哈希，又把两个不同概念混成一句。现拆为：

```text
Artifact source commit        c7e0c5227cdc3d378c188eff5ee19c90f22c98a3
Runbook / documentation HEAD  d1df485225a143c699986e980ea1795637f9afa8（或本轮 docs-only 修订合入后的 HEAD）
```

并写明：**B1–B6 的产品结论绑定 artifact source commit + artifact SHA，而不是绑定后续
docs-only HEAD**；判据是 `git diff --name-only <artifact source commit>..<HEAD>` 的输出全部
落在文档路径内。

### §5.2 控制字符污染

改前对文件做字符级扫描（允许 TAB/LF/CR，禁止 `U+0000–U+0008`、`U+000B`、`U+000C`、
`U+000E–U+001F`、`U+007F`）：

```text
U+0000  count=1  first at line:col 17:16
```

第 17 行 `| git commit |` 之后是一个 U+0000，它占据了 `008297c5` 的首字符位置；第 19–20 行
被从中间拆开，`| nav-core-cli.exe` 丢了 `n` 并插入了一个换行。因此该文件连 Markdown 读取器
都判为二进制（`read` 工具直接以 `binary file` 拒绝）。改后：

```text
CONTROL_CHARS = 0
U+FFFD (replacement char) count = 0
lines(LF)=360  CRLF=0  lone CR=0
encoding: UTF-8 BOM
```

同时把文件从「无 BOM 的 UTF-8」改为**带 BOM**：Windows PowerShell 5.1 的 `Get-Content` 对
无 BOM 的 UTF-8 按系统 ANSI 代码页解码，本文件实测报 **193 行**，而按 UTF-8 实为 **261 行**
（多字节汉字被拆开时吞掉了换行）。我自己在排查时先被这个数字误导过（§9）。行尾仍为 LF。

---

## §6 B 侧不再使用工作区构建产物

runbook 原先在 §一、§二、§六 用 `cd nav-core` 加 `target\release\nav-core-cli.exe`，那既不是
候选字节，也不受候选身份约束。现引入固定根目录，全部操作对象改为：

```text
set ETS2NAV_RC_ROOT=<最终 ZIP 的全新解压目录>\ETS2Nav
%ETS2NAV_RC_ROOT%\nav-core-cli.exe
%ETS2NAV_RC_ROOT%\data\europe-v5
%ETS2NAV_RC_ROOT%\plugins\
```

并明示禁止把 `nav-core\target\release\…`、`desktop\target\release\…`、
`telemetry-plugin\…\out\*.dll` 当作被测对象。开发工具仍可来自仓库（`map-inspector`、
`SignalLab` 是分析工具，不产出被测字节），交给它们的 `--dataset` 也改为指向 RC 内的数据集。
T5 的「负载轮」补上了「从 `%ETS2NAV_RC_ROOT%\plugins\` 放回并重做 SHA-256 核对」，
因为该轮会把 DLL 移出再放回，而放回同样是安装动作。

### §6.1 只读 oracle：`scripts/verify-b-candidate.ps1`

新增一个**只读**脚本作为身份判据。它不构建、不修改、不下载、不修复、不复制、不启动产品进程；
期望值以字面常量写在其中，清单只用于交叉核对而不作为期望值来源。它重新实现了树摘要算法，
因此与仓库实现的「一致」是证据而非同义反复。

实测（正样本）：

```text
核对项 63 个，未通过 0 个，未执行 0 个
FINAL_RC_ZIP_IDENTITY=PASS
FINAL_RC_EXTRACTED_TREE=PASS
RUNTIME_BINARY_IDENTITY=PASS
DATASET_IDENTITY=PASS
RELEASE_MANIFEST_CROSSCHECK=PASS
INSTALLED_PLUGIN_IDENTITY=PASS
PRE-B0 CANDIDATE GATE: PASS
```

「只读」不是声明而是实测：oracle 运行前后对同一条解压树重算全树摘要，均为
`0f5dbde3…f9e5`，未变。

**变异测试（判据必须能失败）**：

| 变异 | 结果 |
| --- | --- |
| M-1 指向装着 `/Brepro` 之前 DLL 的目录 | `INSTALLED_PLUGIN_IDENTITY=FAILED`，其余四项 PASS，`PRE-B0 CANDIDATE GATE: NOT CLOSED`，exit 1 |
| M-2 省略 `-ReleaseManifest` | 该核对记为 `[NOT VERIFIED]`，整体 exit 3，**不报 PASS** |
| M-3 解压树中删除 `search.db` | 树与数据集两项 FAILED（13 项断言不通过），exit 1 |

M-1 使用的是本轮在游戏目录里实际发现的旧 DLL，因此这条变异不是构造出来的假想。

---

## §7 报告侧的过期值处置

`p4r-batch6b-2026-09.md` 的修订遵循一条规则：**历史说明可以保留旧 SHA，但必须显式标注为
historical superseded artifact；操作性指令中不得再出现旧候选作为当前值。**

| 位置 | 原状 | 处置 |
| --- | --- | --- |
| §17 A/B 表 | 以 `a6e570db` / `ae5ca9c7…efcd` 呈现为「本轮实测」，未说明它已非最终候选 | 加注该次运行对应分支 head `a6e570db`、其身份为 historical superseded；表格增列 `source.commit` 行并标注 |
| §17 | 缺最终候选的 A/B 证据 | 新增 §17.2：`c7e0c522` 的 A/B 两份 ZIP 同为 `20a3307b…f3b` / 178091258 B，两棵解压树同为 `0f5dbde3…f9e5`，`Input freeze = STABLE`，`RC RELEASE PIPELINE: PASS`；并记入本轮三棵树摘要一致的独立复核 |
| §17 `7a2406b6` 段 | 陈述句，未标注 | 段落首句标明所涉两个 ZIP 均为 historical superseded，仅用于说明差异来源 |
| §18 外层清单表 | `E:\ets2nav-rc-final` 的 `a6e570db` / 178091246 / `ae5ca9c7…efcd` / `be7820e5…f290` / 383033411 B | 改为最终 `E:\ets2nav-rc-main\A` 的 `c7e0c522` / 178091258 / `20a3307b…f3b` / `0f5dbde3…f9e5` / 383033438 B；旧值一句标注为 superseded |
| §27 冻结候选 manifest | 操作性指令（「实机测试开始前必须首先记录并与本表逐字核对」）指向已作废候选 | 整表替换为最终值，补齐 version/profile、dataset 归档摘要、字形缺席、两个 commit 概念；旧值显式标注 superseded |
| §33 | 已是最终值 | 补一句交叉引用：§27、runbook §0.1、oracle 三处同值，四处不一致即停止 |
| §35 | 无本次缺陷记录 | 新增第 7 条（冻结候选身份指向作废候选、控制字符、根因是两份拷贝只更新一份）与第 8 条（我自己的读取工具被无 BOM 编码误导） |

`grep` 复核：`ae5ca9c7`、`be7820e5`、`178091246`、`178091247`、`a6e570db`、`7a2406b6`
在报告中剩余的每一处出现都位于明确标注的历史语境（§17/§18/§27 的 superseded 注记、§35 第 7 条）；
runbook 报告中剩余的每一处 `target\release` / `telemetry-plugin` 都位于禁令句或更正记录中。

---

## §8 没有做的事

本轮**未**重新打包、**未**改动 `bundle-manifest.json`、**未**改候选版本号、**未**创建 tag、
**未**创建 release。候选是既有的 `20a3307b…f3b`，本轮只验证它。

判据不只是「我没运行打包脚本」：产出候选的打包脚本
`desktop/scripts/assemble-bundle.ps1` 在本轮定稿时重算的 SHA-256 仍为
`d231c81af2a71779108084258534f48786117f11fe8d0f666341239429b23e59`，与 Batch 6B 流水线
最终判定行记录的值相同——即打包输入未被本轮触碰。

本轮对仓库的改动只有：

```text
docs/validation/b-session-runbook-2026-08.md          修正 + 更正记录
docs/validation/p4r-batch6b-2026-09.md                过期值标注与最终值填入
docs/validation/pre-b0-candidate-identity-2026-09.md  本文件（新增）
scripts/verify-b-candidate.ps1                        只读 oracle（新增）
```

`docs/` 与只读 validation helper 之外没有任何改动。新增脚本不是任何打包步骤的输入：
`assemble-bundle.ps1` / `package-release.ps1` 都不引用它，仓库内也没有任何 PSScriptAnalyzer
或脚本枚举步骤会因它而改变行为。

本轮对仓库**之外**的唯一改动是 §4 的插件安装（游戏目录），这是 §6 明确要求的步骤；
它不改变候选字节，也不改变仓库状态。

---

## §9 自我否证记录

1. **我一开始以为「候选身份」只有一份拷贝。** 实际有三处：报告 §27、报告 §33、runbook §0.1，
   而 §33 与另外两处不一致。我先按 runbook 修复，再 grep 全仓才发现报告侧同样过期——
   若只修 runbook，B 侧若有人按报告 §27 核对仍会拿到作废值。

2. **我自己的读取工具被编码误导。** 排查 runbook 时 `Get-Content` 报 193 行，据此我几乎按错误的
   行区间去切割文件；改用 `[System.IO.File]::ReadAllLines` 得到 261 行。原因是无 BOM 的 UTF-8
   被按 ANSI 代码页解码。这与本轮要修的「控制字符」是同一类问题的另一面：**判据依赖了与被判
   性质无关的东西**。

3. **插入补丁时写坏了文件头。** 我用「读文本 → 带 BOM 写回」的方式规范化编码，而读入的文本
   已经含有 BOM 字符，于是写出了两个 BOM（`ef bb bf ef bb bf`），PowerShell 解析器报
   `Unexpected attribute 'CmdletBinding'`。runbook 同样中招。已修正为恰好一个 BOM，
   并把「BOM 计数」纳入验证输出——原先的扫描只检查前三字节，正好看不见这个缺陷。

4. **oracle 的汇总行一开始是不诚实的。** 第一版把 `FINAL_RC_ZIP_IDENTITY` 等四行统一写成
   同一个 `$canonical`，因此 M-1 中「只有已安装插件不对」会连带把 ZIP、树、二进制、数据集
   全部报成 FAILED。这类过度归因会让读者低估哪些证据仍然成立。已改为按类别累计并通过
   重跑 M-1/M-3 确认：M-1 现在只有 `INSTALLED_PLUGIN_IDENTITY=FAILED`。
   这与 Batch 6B §35 第 5 条同源——**判据本身也属于被测对象**。

5. **runbook 的控制字符缺陷是 Batch 6B 引入的，不是外部原因。** 该文件与 §27 表格由同一轮
   写入，因此第 7 条记入 Batch 6B 的 §35，而不是只记在本文件里。

---

## §10 判定

```text
Final RC ZIP identity             = PASS
Final RC extracted tree           = PASS
Runtime binary identity           = PASS
Installed plugin identity         = PASS   （修复前为 FAILED；见 §4）
Dataset identity                  = PASS
Runbook stale identity            = FIXED
Runbook control-character hygiene = PASS   （CONTROL_CHARS = 0）
Workspace binary ambiguity        = CLOSED
Candidate source unchanged        = PASS   （c7e0c522..1be77a81 仅 docs/ 与只读 helper，见 §12）
Required CI                       = PASS   （合入门：run 34760013796 四项 success，无 retry；
                                             合入后 main push 首次 Web E2E 失败、重跑 success，见 §12.1）
Pre-B0 candidate gate             = PASS
```

八项前置条件同时成立，故 `Pre-B0 candidate gate = PASS`。

判定所覆盖的范围需要说清：它声称的是「B1–B6 的操作对象已确定为最终冻结的 Core RC 字节，
且该身份可从归档、解压树、清单与游戏安装目录四处独立核对」。它**不**声称：

- artifact 的 `source.commit` 等于当前 `main` HEAD（本轮是 docs-only 修订，HEAD 必然会前进）；
- B1–B6 已执行或将以任何方式通过；
- 底图与字形齐备（当前为 **Core RC**，`map.pmtiles` 与字形目录均缺席）；
- `Stable Release ready` 有任何改变——该判定在 B1–B6 完成前保持 `NO`。

稳定发布的 blocker 清单仍以 `p4r-batch6b-2026-09.md` §28 为准，本轮不增不减。

---

## §11 远端 required CI 与合入

本文件的初版先于 CI 观察写成，因此这一段在首次推送后按实际观察填入；填入本身也是一次提交，
走同一个 PR、同一组 required checks，未使用任何绕过手段。

```text
分支        pre-b0-candidate-identity-fix
PR          #8  https://github.com/HaowenCang/ETS2Nav/pull/8
实质修订    d348db1e7bc008fc7df9651363adc811275516ad
运行        https://github.com/HaowenCang/ETS2Nav/actions/runs/34760013796
```

下表是该运行的结果，针对**实质修订** `d348db1e`（即上表、代码与 runbook 的实际改动）：

| required check | 结论 | 时长 |
| --- | --- | --- |
| `Source Gates` | success | 6m19s |
| `Dataset Gates` | success | 2m35s |
| `Web E2E` | success | 7m21s |
| `Security Portable` | success | 18m19s |

四项 success，**无 retry、未新增 `continue-on-error`、未放宽任何既有检查**；合入经正常
GitHub PR merge，未使用 admin bypass、未临时关闭保护、未 force push。分支保护在合入前
由独立 API 读取复核：`enforce_admins = true`、`strict = true`、contexts 恰为上述四项、
`allow_force_pushes = false`、`allow_deletions = false`。

**为何只引用一次运行编号。** 本报告自身的每一次回填都是一个新提交，而 `strict = true`
要求该 head 上重新跑完四项检查——逐次把运行编号写回报告会产生新的提交，从而需要新的运行。
因此这里的规则是：引用**实质修订**的那次运行作为内容有效性的证据，并声明其后每个 docs-only
提交都由同一组 required checks 在同一 PR 上重新验证，运行编号见 PR #8 的 checks 页；
**合入前的最后一次运行**即该 head 的有效证据。这不是省略验证，而是拒绝递归。

**关于「current docs HEAD」这一字段的取值方式。** 本报告在合入前无法知道合入提交的哈希，
而钉住一个写入时即会过期的值正是本轮要修的缺陷类型（§5.1）。因此该字段按可核对的形式记录：

```text
当前文档 HEAD = main 上的最新提交，取值方式：
                 git -C <repo> log -1 --format=%H main
写入时的观察值 = d1df485225a143c699986e980ea1795637f9afa8（本轮修订之前）
```

`b-session-runbook-2026-08.md` §0.1 用同样的方式表达该字段（`d1df485…` 或本次 docs-only
修订合入后的更新 HEAD）。**产品身份不依赖这个值**：B1–B6 绑定的是
Artifact source commit `c7e0c522…` 与 artifact SHA `20a3307b…f3b`。

合入后复核 `git diff --name-only c7e0c5227cdc3d378c188eff5ee19c90f22c98a3..<new main HEAD>`：
输出必须全部落在 `docs/` 与 `scripts/verify-b-candidate.ps1`，出现
`nav-core/`、`desktop/`、`telemetry-plugin/`、`tools/ets2nav-web/`、`data/` 之下的任何路径
即表示候选已被另一个代码代数取代，届时 `Candidate source unchanged` 必须改判 `INVALIDATED`。
该复核的结果记录在 §12。

---

## §12 合入后复核

```text
合入提交（main HEAD）  1be77a81be0cdbf17b142d0b5684b0eb290da381
origin/main            同上
divergence             0	0
工作区                 clean（git status --porcelain 为空）
git diff --check       0
tag / release          7 个 / 仅 dataset-europe-v5（均未新增）
```

`git diff --name-only c7e0c5227cdc3d378c188eff5ee19c90f22c98a3..1be77a81be0cdbf17b142d0b5684b0eb290da381`
的输出恰为四项：

```text
docs/validation/b-session-runbook-2026-08.md
docs/validation/p4r-batch6b-2026-09.md
docs/validation/pre-b0-candidate-identity-2026-09.md
scripts/verify-b-candidate.ps1
```

全部落在 `docs/` 与只读 helper；`nav-core/`、`desktop/`、`telemetry-plugin/`、
`tools/ets2nav-web/`、`data/` 之下被改动的文件数均为 **0**。据此
`Candidate source unchanged = PASS`。

候选本体重算：ZIP 仍为 178091258 B / `20a3307b…f3b`；`desktop/scripts/assemble-bundle.ps1`
仍为 `d231c81af2a71779108084258534f48786117f11fe8d0f666341239429b23e59`，与产出候选的流水线
记录值相同；合入后在 `main` 上重跑 oracle，`核对项 63 个，未通过 0 个`，
`PRE-B0 CANDIDATE GATE: PASS`。

### §12.1 合入后 main push 运行的一次失败与重跑

`main` push 运行 `34762118382`（head `1be77a81`）第一次执行时 `Web E2E` **失败**，
其余三个 job 成功：

```text
1) tests\e2e\12-lan.spec.mjs:197 › E2E-LAN-03 远端页面推导同源 LAN WS 并真实建立连接（B5 回归）
   Error: E2E-LAN-03: 出现 2 条非白名单诊断
     [console.error] Failed to load resource: net::ERR_NO_BUFFER_SPACE @ http://10.1.0.107:54032/map.pmtiles
     [console.error] Error @ http://10.1.0.107:54032/vendor/maplibre-gl.js
   45 passed / 1 failed
```

对同一提交重跑该 job（attempt 2）后 `Web E2E` success（7m12s）。因此**同一棵树、同一提交上
1 次失败、2 次通过**（PR 阶段运行 `34760013796` 与 `34760975851` 亦均为 success）。

判定为**环境级抖动，非产品缺陷**，依据有三：`net::ERR_NO_BUFFER_SPACE` 是 Chromium 网络栈的
缓冲区耗尽错误，产生于资源加载层而不是应用逻辑层；失败与通过的两次运行是同一 commit 的
同一棵树；本轮改动只涉及 `docs/` 与一个新的只读脚本，不触及 `tools/ets2nav-web/` 的任何
代码路径。

**登记为待修问题，且本轮刻意不修**：`E2E-LAN-03` 的诊断白名单不区分「应用产生的诊断」与
「基础设施级网络错误」，因此在 runner 资源紧张时会随机失败。没有给 Playwright 加 retry
（本轮明确禁止），也没有扩大 `assertCleanDiagnostics` 的白名单——后者会改动
`tools/ets2nav-web/`，按 §8 的判据将**使候选冻结失效**。该项留给下一个允许改动前端测试的
轮次，并在修改时一并评估是否需要在 CI 中隔离该用例。

因此 `Required CI = PASS` 指的是**合入门**：PR #8 的四个 required check 在 `d348db1e` 与
`a152dec` 两个 head 上均 success，合入未使用任何绕过手段。合入后的 main push 运行需要一次
重跑才全绿，这一事实记在此处，不以「最终绿了」掩盖。

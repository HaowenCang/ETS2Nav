# ETS2Nav 候选版本发布说明（占位文件）

本文件**不是**发布说明，而是发布说明的输出位置。它由
`desktop/scripts/write-release-manifest.ps1` 在打包流水线中从**实测产物**重写，
内容包含该次构建的产物字节数、SHA-256、解压树摘要、数据集 provenance 与实机验证状态。

此前本文件曾提交过一份带具体数字的草稿，那些数字来自一次**被判定无效**的 A/B 打包
（构建窗口内有其它写入者修改工作树，两次打包的输入不同）。无效运行的数字留在仓库里
会被后续读者当成事实引用，因此已替换为本占位说明。

重新生成：

```bat
powershell -NoProfile -ExecutionPolicy Bypass -File desktop\scripts\run-release-pipeline.ps1 ^
    -Dataset <数据集目录> -WorkDir <工作目录>
```

流水线会同时产出 `release-manifest.json`、`SHA256SUMS.txt`、`release-validation-report.md`
与本发布说明；这些文件与 ZIP 放在同一目录，不进仓库。

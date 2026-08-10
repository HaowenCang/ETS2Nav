# HashFS（.scs 容器）格式笔记（A3）

来源：社区逆向结论（easy-scsmodmanager/trucklib 移植版）＋ 实测 def.scs（1.55+ 游戏版本打包）交叉验证。
状态：✅ v1/v2 读取器实现并验证（与官方 scs_extractor 解包产物逐字节一致）

## 容器结构

| 项 | v1 | v2（游戏 1.50+） |
|---|---|---|
| 头部 | 24 字节 | 44 字节读取（实际字段至 0x2C——勘误 2026-08-10，原记 49） |
| 条目表 | 扁平，32 字节/条 | **zlib 压缩**，16 字节/条 |
| 元数据表 | 无（内嵌于条目） | **zlib 压缩**，4 字节块头 + 主体 |
| 目录列表 | 文本（`*` 前缀=子目录） | 二进制（见下） |
| 纹理 | 独立 .tobj/.dds | 打包条目（GDeflate，未实现提取） |

## v2 header（实测 def.scs 对照）

| 偏移 | 字段 |
|---|---|
| 0x00 | magic "SCS#" (u32) |
| 0x04 | version (u16；实测 2) |
| 0x06 | salt (u16；实测 0) |
| 0x08 | "CITY" (4 字节) |
| 0x0C | entry 数 (u32；def.scs = 66928) |
| 0x10 | entry 表压缩长度 (u32) |
| 0x14 | 未知 (u32；def.scs = 334640) |
| 0x18 | metadata 表压缩长度 (u32) |
| 0x1C | entry 表起始 (u64) |
| 0x24 | metadata 表起始 (u64) |

## v2 条目表（解压后）

16 字节/条：`hash(u64) + meta_index(u32) + meta_count(u16) + flags(u16)`

## v2 metadata 表（解压后）

- 块头：4 字节（3 索引字节 + 1 类型字节），类型：1=IMAGE、128=PLAIN、129=DIRECTORY
- 主体位于 `meta_index*4 + meta_count*4`（IMAGE 类型前移 12 字节 tobj 元数据）
- 主体字段：csize（28 位 LE + 字节3 位4=compressed）、size（同构）、offset_block(u32) × 16 = 数据偏移

## v2 目录列表（二进制）

`u32 count + count 字节 name-length 数组 + names 区`（`/` 前缀 = 子目录）

## 路径哈希

`CityHash64(utf8(salt_decimal + path_without_leading_slash))`
注意：SCS 使用的 CityHash64 经实证为 **Google 原版算法**（CityHash.cs 逐操作一致；对 def.scs 条目表 6 条路径 5 条命中，社区流传的 `^b` 变体全部落空——勘误 2026-08-10）。HashLen17To32 的 `+length` 与 K3、HashLen33To64 的 `(length + fetch) * K0` 本就是 Google 原版组成部分，非 SCS 差异。以 def.scs 条目命中 + extractor 对照双重验证为准。

## 数据压缩

常规文件 zlib/deflate；`-nocompress` 打包的归档可能未压缩（按 flags 判断）。

## 实现

`map-compiler/src/ScsHashFs/`：CityHash.cs（独立实现）、HashFsReader.cs（v1/v2 自动识别、条目查找、提取、目录列举、递归枚举）。测试：5 项集成测试（真实 def.scs，缺失时跳过）。

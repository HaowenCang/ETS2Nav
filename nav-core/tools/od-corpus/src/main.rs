// od-corpus：P5 自动化 OD corpus（PLAN-P3plus.md §2 A3）。
// 三个子命令：
//   od-baseline <dataset> [--write <path>]  —— 九区域 36 城市对期望特征基准生成
//   od-regress  <dataset> [<baseline>]      —— 重跑对比基准（diff 回归）
//   od-check    <dataset> [pairs]           —— 全图随机 OD：可达/连续/跳变/掉头
// 城市定位用 search.db City POI 的 access_node（uid）→ 全量节点位置（路由空间；
// POI 的 x/z 为地图坐标空间，不可直接用于路由——P1-08 已知）。
use std::collections::{BTreeSet, HashMap, HashSet};

/// 单 OD 特征元组：(distance_m, eta_s, edges, transit_used, company_near_dest)。
type OdFeatures = (f64, f64, usize, bool, bool);

/// 九区域城市对（ETS2 游戏内城市名；每区域 4 对跨城长路线）。
const OD_PAIRS: &[(&str, &str, &str)] = &[
    ("uk", "london", "edinburgh"),
    ("uk", "london", "birmingham"),
    ("uk", "newcastle", "plymouth"),
    ("uk", "glasgow", "cardiff"),
    ("france", "paris", "marseille"),
    ("france", "paris", "bordeaux"),
    ("france", "lille", "nice"),
    ("france", "nantes", "lyon"),
    ("germany", "berlin", "munchen"),
    ("germany", "hamburg", "frankfurt"),
    ("germany", "dortmund", "kassel"),
    ("germany", "bremen", "nurnberg"),
    ("nordic", "stockholm", "oslo"),
    ("nordic", "oslo", "kobenhavn"),
    ("nordic", "helsinki", "oulu"),
    ("nordic", "goteborg", "stockholm"),
    ("balkan", "beograd", "zagreb"),
    ("balkan", "sarajevo", "podgorica"),
    ("balkan", "skopje", "thessaloniki"),
    ("balkan", "bucuresti", "sofia"),
    ("italy", "roma", "milano"),
    ("italy", "napoli", "venezia"),
    ("italy", "torino", "bari"),
    ("italy", "genova", "bari"),
    ("iberia", "madrid", "barcelona"),
    ("iberia", "madrid", "sevilla"),
    ("iberia", "lisboa", "porto"),
    ("iberia", "bilbao", "valencia"),
    ("east", "warszawa", "krakow"),
    ("east", "prague", "wien"),
    ("east", "budapest", "wien"),
    ("east", "wroclaw", "gdansk"),
    ("dlc", "tirana", "skopje"),
    ("dlc", "sarajevo", "beograd"),
    ("dlc", "ivalo", "honningsvag"),
    ("dlc", "athens", "patras"),
];

struct OdCtx {
    graph: nav_graph::CompactGraph,
    spatial: nav_spatial::SpatialIndex,
    router: nav_router::search::Router,
    cities: Vec<nav_dataset::PoiRecord>,
    /// uid → 路由坐标（全量节点含孤立——POI access_node 可能不在 active 节点）。
    node_pos: HashMap<u64, (f64, f64)>,
    /// 全量节点坐标（按 routing.nodes 顺序，随机 OD 采样用）。
    node_xy: Vec<(f64, f64)>,
}

fn od_ctx(dataset_dir: &str) -> OdCtx {
    let (routing, _j) = nav_dataset::load_dataset(std::path::Path::new(dataset_dir))
        .unwrap_or_else(|e| {
            eprintln!("加载 dataset 失败: {e}");
            std::process::exit(1);
        });
    let node_pos = routing.nodes.iter().map(|n| (n.uid, (n.x, n.z))).collect();
    let node_xy = routing.nodes.iter().map(|n| (n.x, n.z)).collect();
    let graph = nav_graph::CompactGraph::build(&routing);
    let spatial = nav_spatial::SpatialIndex::build(&graph, nav_spatial::DEFAULT_CELL_SIZE);
    let router = nav_router::search::Router::new(graph.node_count());
    let search_db = std::path::Path::new(dataset_dir).join("search.db");
    let cities = nav_dataset::load_pois(&search_db).unwrap_or_else(|e| {
        eprintln!("加载 search.db POI 失败: {e}");
        std::process::exit(1);
    });
    OdCtx {
        graph,
        spatial,
        router,
        cities,
        node_pos,
        node_xy,
    }
}

/// 城市全部路由位置（每 City POI 行一个；多行回退——部分 access_node 落在
/// 断簇/孤立点，A* 不可达——P5 实测 8/36 城市对触发，登记 P2 遗留）。
fn city_positions(ctx: &OdCtx, name: &str) -> Vec<(f64, f64)> {
    ctx.cities
        .iter()
        .filter(|p| p.kind == "City" && p.name == name)
        .filter_map(|p| {
            let uid = u64::from_str_radix(p.access_node_hex.trim(), 16).ok()?;
            ctx.node_pos.get(&uid).copied()
        })
        .collect()
}

/// 目的地 1.5km（路由空间）内 Company POI access_node 存在性（公司入口特征）。
fn company_near(ctx: &OdCtx, x: f64, z: f64) -> bool {
    ctx.cities.iter().any(|p| {
        if p.kind != "Company" {
            return false;
        }
        let Ok(uid) = u64::from_str_radix(p.access_node_hex.trim(), 16) else {
            return false;
        };
        let Some((px, pz)) = ctx.node_pos.get(&uid) else {
            return false;
        };
        (px - x).powi(2) + (pz - z).powi(2) < 1500.0 * 1500.0
    })
}

/// 单 OD 特征：A* Fastest 路线 + 期望特征提取（起终点多候选组合回退）。
fn od_features(ctx: &mut OdCtx, from: &str, to: &str) -> Option<OdFeatures> {
    let froms = city_positions(ctx, from);
    let tos = city_positions(ctx, to);
    if froms.is_empty() || tos.is_empty() {
        return None;
    }
    for (pfx, pfz) in &froms {
        for (ptx, ptz) in &tos {
            let Some(s1) =
                nav_router::snap::snap_nearest(&ctx.graph, &ctx.spatial, *pfx, *pfz, 300.0)
            else {
                continue;
            };
            let Some(s2) =
                nav_router::snap::snap_nearest(&ctx.graph, &ctx.spatial, *ptx, *ptz, 300.0)
            else {
                continue;
            };
            let req = nav_router::search::RouteRequest::new(
                &ctx.graph,
                nav_router::snap::VirtualEndpoint::start(&s1, true),
                nav_router::snap::VirtualEndpoint::goal(&s2),
                nav_router::cost::RouteProfile::Fastest,
            );
            if let Some(r) = ctx.router.astar(&req) {
                let transit = r
                    .edges
                    .iter()
                    .filter(|&&eid| {
                        matches!(
                            ctx.graph.edges[eid as usize].kind,
                            nav_graph::EdgeKind::Ferry | nav_graph::EdgeKind::Train
                        )
                    })
                    .count();
                return Some((
                    r.distance_m,
                    r.eta_s,
                    r.edges.len(),
                    transit > 0,
                    company_near(ctx, *ptx, *ptz),
                ));
            }
        }
    }
    None
}

/// od-baseline：生成九区域基准（打印 + [--write path] 写文本文件）。
/// 文本行格式：region|from|to|distance_m|eta_s|edges|transit|company
fn od_baseline(dataset_dir: &str, write: Option<&String>) {
    let mut ctx = od_ctx(dataset_dir);
    let mut lines: Vec<String> = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    let mut ok = 0u32;
    let mut regions = BTreeSet::new();
    for (region, f, t) in OD_PAIRS {
        regions.insert(*region);
        match od_features(&mut ctx, f, t) {
            Some((d, eta, edges, transit, company)) => {
                let line = format!("{region}|{f}|{t}|{d:.0}|{eta:.0}|{edges}|{transit}|{company}");
                println!("[{region} {f}->{t}] {d:.0}m {eta:.0}s {edges}边 transit={transit} company={company}");
                lines.push(line);
                ok += 1;
            }
            None => {
                let msg = format!("{region}:{f}->{t}");
                missing.push(msg);
            }
        }
    }
    if let Some(path) = write {
        let content = lines.join("\n") + "\n";
        std::fs::write(path, content).unwrap_or_else(|e| {
            eprintln!("写基准文件失败: {e}");
            std::process::exit(1);
        });
        println!("基准写入: {path}");
    }
    println!(
        "OD-BASELINE regions={} pairs={} missing={}",
        regions.len(),
        ok,
        missing.len()
    );
    for m in &missing {
        eprintln!("  缺城市对: {m}");
    }
    if missing.is_empty() {
        println!("OD-BASELINE PASS");
    } else {
        println!("OD-BASELINE FAIL");
        std::process::exit(1);
    }
}

/// od-regress：重跑并对比基准。distance ±1%、eta/edges ±5%、transit/company 标志一致。
fn od_regress(dataset_dir: &str, baseline: Option<&String>) {
    let path = baseline
        .cloned()
        .unwrap_or_else(|| "od-baseline-europe-v4.txt".to_string());
    let content = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        eprintln!("读基准失败 {path}: {e}（先运行 od-baseline --write）");
        std::process::exit(1);
    });
    let mut expected: HashMap<(String, String), OdFeatures> = HashMap::new();
    for line in content.lines().filter(|l| !l.trim().is_empty()) {
        let v: Vec<&str> = line.split('|').collect();
        if v.len() != 8 {
            continue;
        }
        let _region = v[0];
        expected.insert(
            (v[1].to_string(), v[2].to_string()),
            (
                v[3].parse().unwrap_or(-1.0),
                v[4].parse().unwrap_or(-1.0),
                v[5].parse().unwrap_or(0),
                v[6] == "true",
                v[7] == "true",
            ),
        );
    }
    let mut ctx = od_ctx(dataset_dir);
    let mut diff = 0u32;
    let mut total = 0u32;
    for ((f, t), &(ed, ee, en, etr, eco)) in &expected {
        total += 1;
        let Some((d, eta, edges, transit, company)) = od_features(&mut ctx, f, t) else {
            println!("  [缺失] {f}->{t}（当前不可达）");
            diff += 1;
            continue;
        };
        let mut bad = false;
        if (d - ed).abs() / ed.max(1.0) > 0.01 {
            println!("  [距离] {f}->{t}: 基准 {ed:.0}m 现 {d:.0}m");
            bad = true;
        }
        if (eta - ee).abs() / ee.max(1.0) > 0.05 {
            println!("  [ETA] {f}->{t}: 基准 {ee:.0}s 现 {eta:.0}s");
            bad = true;
        }
        if (edges as f64 - en as f64).abs() / (en as f64).max(1.0) > 0.05 {
            println!("  [边数] {f}->{t}: 基准 {en} 现 {edges}");
            bad = true;
        }
        if transit != etr {
            println!("  [transit] {f}->{t}: 基准 {etr} 现 {transit}");
            bad = true;
        }
        if company != eco {
            println!("  [company] {f}->{t}: 基准 {eco} 现 {company}");
            bad = true;
        }
        if bad {
            diff += 1;
        }
    }
    println!("OD-REGRESS pairs={total} diff={diff}");
    if diff == 0 {
        println!("OD-REGRESS PASS");
    } else {
        println!("OD-REGRESS FAIL");
        std::process::exit(1);
    }
}

/// od-check：全图随机 OD（固定种子 LCG，可复现）——可达性/geometry 连续/
/// graph jump/不合理掉头。
fn od_check(dataset_dir: &str, pairs: u32) {
    let mut ctx = od_ctx(dataset_dir);
    // 锚点池：全图路由节点直接采样（不依赖 POI 坐标空间——随机 OD 检查图的真实
    // 连通性）。断簇城市接入点（P2 遗留：部分城市 POI access_node 落在与主路网
    // 拓扑断开的小簇）另行统计上报。
    let anchors: Vec<(f64, f64)> = ctx.node_xy.clone();
    let mut broken = 0u32;
    let mut seen_city = HashSet::new();
    for p in &ctx.cities {
        if p.kind != "City" || !seen_city.insert(p.name.clone()) {
            continue;
        }
        let Ok(uid) = u64::from_str_radix(p.access_node_hex.trim(), 16) else {
            continue;
        };
        let Some(&(x, z)) = ctx.node_pos.get(&uid) else {
            continue;
        };
        let Some(s) = nav_router::snap::snap_nearest(&ctx.graph, &ctx.spatial, x, z, 300.0) else {
            broken += 1;
            continue;
        };
        let e = &ctx.graph.edges[s.edge_id as usize];
        let mut scope = 0u32;
        for node in [e.from, e.to] {
            scope += bfs_scope(&ctx.graph, node as usize, 3);
        }
        if scope < 8 {
            broken += 1;
        }
    }
    eprintln!(
        "OD-PRE: 全图节点锚点 {} 断簇城市接入点 {broken}",
        anchors.len()
    );
    let mut reachable = 0u32;
    let mut continuity_ok = 0u32;
    let mut jumps = 0u32;
    let mut uturns = 0u32;
    let t0 = std::time::Instant::now();
    let mut s = 20260811u64;
    let mut rnd = move || {
        s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (s >> 33) as f64 / (1u64 << 31) as f64
    };
    for _ in 0..pairs {
        let (ax, az) = anchors[(rnd() * anchors.len() as f64) as usize % anchors.len()];
        let (bx, bz) = anchors[(rnd() * anchors.len() as f64) as usize % anchors.len()];
        let Some(s1) = nav_router::snap::snap_nearest(&ctx.graph, &ctx.spatial, ax, az, 300.0)
        else {
            continue;
        };
        let Some(s2) = nav_router::snap::snap_nearest(&ctx.graph, &ctx.spatial, bx, bz, 300.0)
        else {
            continue;
        };
        let req = nav_router::search::RouteRequest::new(
            &ctx.graph,
            nav_router::snap::VirtualEndpoint::start(&s1, true),
            nav_router::snap::VirtualEndpoint::goal(&s2),
            nav_router::cost::RouteProfile::Fastest,
        );
        let Some(r) = ctx.router.astar(&req) else {
            continue;
        };
        reachable += 1;
        // geometry 连续 / graph jump：相邻边共享压缩端点
        let mut cont = true;
        for w in r.edges.windows(2) {
            let a = &ctx.graph.edges[w[0] as usize];
            let b = &ctx.graph.edges[w[1] as usize];
            if a.to != b.from {
                cont = false;
                jumps += 1;
                if jumps <= 3 {
                    println!("  jump: 边 {} -> {} 端点不连续", w[0], w[1]);
                }
            }
        }
        if cont {
            continuity_ok += 1;
        }
        // 不合理掉头：相邻 Road→Road 边首尾方向夹角 > 150°
        // （movement 边是路口内部转向，允许任意角——环岛/复杂路口合法）
        for w in r.edges.windows(2) {
            let a = &ctx.graph.edges[w[0] as usize];
            let b = &ctx.graph.edges[w[1] as usize];
            if a.kind != nav_graph::EdgeKind::Road || b.kind != nav_graph::EdgeKind::Road {
                continue;
            }
            // 短边（<50m）几何点少、首尾方向噪声大——不参与掉头判定
            if a.length < 50.0 || b.length < 50.0 {
                continue;
            }
            let va = edge_dir(&ctx.graph, a);
            let vb = edge_dir(&ctx.graph, b);
            let Some((ax_, az_)) = va else { continue };
            let Some((bx_, bz_)) = vb else { continue };
            let dot = ax_ * bx_ + az_ * bz_;
            let la = (ax_ * ax_ + az_ * az_).sqrt();
            let lb = (bx_ * bx_ + bz_ * bz_).sqrt();
            if la > 1e-9 && lb > 1e-9 && dot / (la * lb) < -0.866 {
                // cos(150°) ≈ -0.866
                uturns += 1;
                if uturns <= 3 {
                    println!(
                        "  uturn: 边 {} (len={:.0}m kind={:?}) -> {} (len={:.0}m kind={:?}) 夹角>150°",
                        w[0], a.length, a.kind, w[1], b.length, b.kind
                    );
                }
            }
        }
    }
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    println!(
        "OD-CHECK pairs={pairs} reachable={reachable} continuity={continuity_ok} jumps={jumps} uturns={uturns} ms={ms:.0}"
    );
    // PASS：图拓扑连续（jumps 严格 0）；uturn 容忍少量真实图缺陷（登记边号）；
    // reachable 为主网占比下限（图存在拓扑断簇——P5 发现，登记 P2 遗留排查）。
    if jumps == 0 && uturns <= 3 && reachable as f64 / pairs as f64 >= 0.5 {
        println!(
            "OD-CHECK PASS（断簇占比 {:.0}%——登记为图属性，非工具缺陷）",
            (1.0 - reachable as f64 / pairs as f64) * 100.0
        );
    } else {
        println!("OD-CHECK FAIL（jumps>0 或 uturns>3 或主网占比<50%）");
        std::process::exit(1);
    }
}

/// 3 跳 BFS 可达节点数（连通性预筛：断簇簇内节点数远小于主网）。
fn bfs_scope(g: &nav_graph::CompactGraph, start: usize, depth: usize) -> u32 {
    let mut seen = HashSet::new();
    let mut frontier = vec![start];
    seen.insert(start);
    for _ in 0..depth {
        let mut next = Vec::new();
        for &n in &frontier {
            for &eid in g.out_edges(n) {
                let to = g.edges[eid as usize].to as usize;
                if seen.insert(to) {
                    next.push(to);
                }
            }
        }
        frontier = next;
        if frontier.is_empty() {
            break;
        }
    }
    seen.len() as u32
}

/// 边首尾方向向量（polyline 首→尾，2D）。
fn edge_dir(graph: &nav_graph::CompactGraph, e: &nav_graph::CompactEdge) -> Option<(f64, f64)> {
    let pts = graph.edge_geometry(e);
    if pts.len() >= 2 {
        let (ax, _, az) = pts[0];
        let (bx, _, bz) = pts[pts.len() - 1];
        Some((bx - ax, bz - az))
    } else {
        let a = graph.positions[e.from as usize];
        let b = graph.positions[e.to as usize];
        Some((b.0 - a.0, b.2 - a.2))
    }
}

/// od-diag：诊断工具——打印节点位置/边数，及两点间连通性（排查坐标空间问题）。
fn od_diag(dataset_dir: &str, x1: f64, z1: f64, x2: f64, z2: f64) {
    let mut ctx = od_ctx(dataset_dir);
    let Some(s1) = nav_router::snap::snap_nearest(&ctx.graph, &ctx.spatial, x1, z1, 300.0) else {
        eprintln!("起点 snap 失败 ({x1},{z1})");
        return;
    };
    let Some(s2) = nav_router::snap::snap_nearest(&ctx.graph, &ctx.spatial, x2, z2, 300.0) else {
        eprintln!("终点 snap 失败 ({x2},{z2})");
        return;
    };
    let e1 = &ctx.graph.edges[s1.edge_id as usize];
    let e2 = &ctx.graph.edges[s2.edge_id as usize];
    eprintln!(
        "s1 edge={} kind={:?} from={} to={} pos=({:.0},{:.0})  s2 edge={} kind={:?} from={} to={} pos=({:.0},{:.0})",
        s1.edge_id, e1.kind, e1.from, e1.to, s1.position.0, s1.position.1,
        s2.edge_id, e2.kind, e2.from, e2.to, s2.position.0, s2.position.1
    );
    // 起点边两端节点的出边连通样本
    for node in [e1.from, e1.to] {
        let eo = ctx.graph.out_edges(node as usize);
        eprintln!("node {node} 出边 {} 条", eo.len());
    }
    let req = nav_router::search::RouteRequest::new(
        &ctx.graph,
        nav_router::snap::VirtualEndpoint::start(&s1, true),
        nav_router::snap::VirtualEndpoint::goal(&s2),
        nav_router::cost::RouteProfile::Fastest,
    );
    match ctx.router.astar(&req) {
        Some(r) => eprintln!("连通: {}m {}边", r.distance_m, r.edges.len()),
        None => eprintln!("不连通 (A* None)"),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!(
            "用法: od-corpus <od-baseline|od-regress|od-check> <dataset-dir> [--write <path>]"
        );
        eprintln!("  od-baseline <dataset> [--write <path>]   —— 九区域 36 城市对基准生成");
        eprintln!("  od-regress <dataset> [<baseline-path>]   —— 基准对比（diff 回归）");
        eprintln!("  od-check <dataset> [pairs]               —— 随机 OD 全图检查（默认 1000）");
        std::process::exit(2);
    }
    match args[1].as_str() {
        "od-baseline" => {
            let write = if args.get(3).map(|v| v.as_str()) == Some("--write") {
                args.get(4)
            } else {
                args.get(3)
            };
            od_baseline(&args[2], write);
        }
        "od-regress" => od_regress(&args[2], args.get(3)),
        "od-diag" if args.len() >= 6 => {
            let x1: f64 = args[3].parse().unwrap();
            let z1: f64 = args[4].parse().unwrap();
            let x2: f64 = args[5].parse().unwrap();
            let z2: f64 = args[6].parse().unwrap();
            od_diag(&args[2], x1, z1, x2, z2);
        }
        "od-check" => {
            let pairs = args
                .get(3)
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(1000);
            od_check(&args[2], pairs);
        }
        _ => {
            eprintln!("未知命令: {}", args[1]);
            std::process::exit(2);
        }
    }
}

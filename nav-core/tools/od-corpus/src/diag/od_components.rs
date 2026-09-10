// od-components：P5 断簇排查诊断（2026-08-12，修订版）。
// 修订要点：全部分析限定在**活跃节点**（被至少一条边引用）——routing.graph 含 5.6M
// 节点，其中 93.6% 是不被任何边引用的 item 节点；把它们计入分量统计会得出
// "94% 的 1 节点分量"这类无意义结论（本工具初版即有此缺陷）。
//
// 用法：od-components <dataset-dir>
// 输出 machine-readable 前缀行，可直接用于修复前后对比。
use std::collections::{HashMap, HashSet};

struct Dsu {
    parent: Vec<u32>,
    size: Vec<u32>,
}

impl Dsu {
    fn new(n: usize) -> Self {
        Dsu {
            parent: (0..n as u32).collect(),
            size: vec![1; n],
        }
    }
    fn find(&mut self, mut x: u32) -> u32 {
        while self.parent[x as usize] != x {
            self.parent[x as usize] = self.parent[self.parent[x as usize] as usize];
            x = self.parent[x as usize];
        }
        x
    }
    fn union(&mut self, a: u32, b: u32) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra == rb {
            return;
        }
        let (big, small) = if self.size[ra as usize] >= self.size[rb as usize] {
            (ra, rb)
        } else {
            (rb, ra)
        };
        self.parent[small as usize] = big;
        self.size[big as usize] += self.size[small as usize];
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("用法: od-components <dataset-dir>");
        std::process::exit(2);
    }
    let dir = std::path::Path::new(&args[1]);
    let rg = nav_dataset::load_routing_graph(&dir.join("routing.graph")).unwrap_or_else(|e| {
        eprintln!("加载 routing.graph 失败: {e}");
        std::process::exit(1);
    });

    let n = rg.nodes.len();
    let mut active = vec![false; n];
    let mut kinds = [0usize; 5];
    let mut transit_edges: Vec<(usize, u32, u32)> = Vec::new();
    for (i, e) in rg.edges.iter().enumerate() {
        active[e.from as usize] = true;
        active[e.to as usize] = true;
        let ki = match e.kind {
            nav_dataset::EdgeKind::Road => 0,
            nav_dataset::EdgeKind::JunctionMovement => 1,
            nav_dataset::EdgeKind::Ferry => 2,
            nav_dataset::EdgeKind::Train => 3,
            nav_dataset::EdgeKind::ServiceAccess => 4,
        };
        kinds[ki] += 1;
        if matches!(
            e.kind,
            nav_dataset::EdgeKind::Ferry | nav_dataset::EdgeKind::Train
        ) {
            transit_edges.push((i, e.from, e.to));
        }
    }
    let active_count = active.iter().filter(|&&a| a).count();
    println!(
        "DIAG-GRAPH nodes_raw={} nodes_active={} edges={} road={} movement={} ferry={} train={} service={}",
        n, active_count, rg.edges.len(), kinds[0], kinds[1], kinds[2], kinds[3], kinds[4]
    );

    // ---- WCC（仅活跃节点参与统计） ----
    let mut dsu = Dsu::new(n);
    for e in &rg.edges {
        dsu.union(e.from, e.to);
    }
    let mut comp_size: HashMap<u32, u32> = HashMap::new();
    for (i, &act) in active.iter().enumerate() {
        if !act {
            continue;
        }
        *comp_size.entry(dsu.find(i as u32)).or_insert(0) += 1;
    }
    let mut comps: Vec<(u32, u32)> = comp_size.iter().map(|(&r, &s)| (r, s)).collect();
    comps.sort_by_key(|&(_, s)| std::cmp::Reverse(s));
    let main_comp = comps[0].0;
    let main_size = comps[0].1;
    println!(
        "DIAG-WCC active_components={} main_size={} main_ratio_of_active={:.4} active_outside_main={}",
        comps.len(),
        main_size,
        main_size as f64 / active_count.max(1) as f64,
        active_count - main_size as usize
    );

    // ---- 分量规模分布（活跃节点） ----
    let mut buckets = [0usize; 8];
    for &(_, s) in &comps {
        let bi = match s {
            1 => 0,
            2..=4 => 1,
            5..=9 => 2,
            10..=49 => 3,
            50..=199 => 4,
            200..=999 => 5,
            1000..=9999 => 6,
            _ => 7,
        };
        buckets[bi] += 1;
    }
    println!(
        "DIAG-DIST size1={} size2_4={} size5_9={} size10_49={} size50_199={} size200_999={} size1k_9999={} size10k_plus={}",
        buckets[0], buckets[1], buckets[2], buckets[3], buckets[4], buckets[5], buckets[6], buckets[7]
    );
    let nonmain_lt50: usize = comps
        .iter()
        .skip(1)
        .filter(|&&(_, s)| s < 50)
        .map(|&(_, s)| s as usize)
        .sum();
    println!(
        "DIAG-DIST-SUMMARY active_outside_main_in_components_lt50={} active_outside_main_total={}",
        nonmain_lt50,
        active_count - main_size as usize
    );

    // ---- 前 30 分量（含包围盒） ----
    println!("DIAG-TOP 前 30 活跃分量:");
    let mut comp_points: HashMap<u32, Vec<u32>> = HashMap::new();
    for (i, &act) in active.iter().enumerate() {
        if act {
            comp_points
                .entry(dsu.find(i as u32))
                .or_default()
                .push(i as u32);
        }
    }
    for (rank, &(c, s)) in comps.iter().take(30).enumerate() {
        let pts = &comp_points[&c];
        let (mut minx, mut maxx, mut minz, mut maxz) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for &i in pts {
            let p = &rg.nodes[i as usize];
            minx = minx.min(p.x);
            maxx = maxx.max(p.x);
            minz = minz.min(p.z);
            maxz = maxz.max(p.z);
        }
        println!(
            "DIAG-TOP rank={} size={} center=({:.0},{:.0}) bbox=({:.0},{:.0})-({:.0},{:.0})",
            rank,
            s,
            (minx + maxx) / 2.0,
            (minz + maxz) / 2.0,
            minx,
            minz,
            maxx,
            maxz
        );
    }

    // ---- transit 端点连通性（关键判据） ----
    // 陆地节点 = 至少有一条非 transit 边的端点。端点若不在陆地上，则 ferry 边
    // 不桥接任何路网（P5 缺陷的直接判据）。
    let mut land = vec![false; n];
    for e in &rg.edges {
        if matches!(
            e.kind,
            nav_dataset::EdgeKind::Ferry | nav_dataset::EdgeKind::Train
        ) {
            continue;
        }
        land[e.from as usize] = true;
        land[e.to as usize] = true;
    }
    let mut endpoints_total = 0usize;
    let mut endpoints_isolated = 0usize;
    for &(_, f, t) in &transit_edges {
        endpoints_total += 2;
        if !land[f as usize] {
            endpoints_isolated += 1;
        }
        if !land[t as usize] {
            endpoints_isolated += 1;
        }
    }
    println!(
        "DIAG-TRANSIT total_edges={} endpoints={} endpoints_isolated={}",
        transit_edges.len(),
        endpoints_total,
        endpoints_isolated
    );

    // ---- 真桥接数：去掉该 transit 边后两端是否仍连通（否则该边是 sole link） ----
    let mut bridges = 0usize;
    for &(skip, f, t) in &transit_edges {
        let mut d = Dsu::new(n);
        for (i, e) in rg.edges.iter().enumerate() {
            if i == skip {
                continue;
            }
            d.union(e.from, e.to);
        }
        if d.find(f) != d.find(t) {
            bridges += 1;
        }
    }
    println!(
        "DIAG-TRANSIT-SUMMARY transit_bridges={bridges} of {total}",
        total = transit_edges.len()
    );

    println!("DIAG-TRANSIT-DETAIL（最多 12 条）:");
    for (k, &(_, f, t)) in transit_edges.iter().enumerate() {
        if k >= 12 {
            break;
        }
        println!(
            "  te f=({:.0},{:.0}) land={} | t=({:.0},{:.0}) land={}",
            rg.nodes[f as usize].x,
            rg.nodes[f as usize].z,
            land[f as usize],
            rg.nodes[t as usize].x,
            rg.nodes[t as usize].z,
            land[t as usize]
        );
    }

    // ---- 主分量内城市接入点覆盖（search.db） ----
    let poi = nav_dataset::load_pois(&dir.join("search.db")).unwrap_or_default();
    let uid_to_node: HashMap<u64, u32> = rg
        .nodes
        .iter()
        .enumerate()
        .map(|(i, nd)| (nd.uid, i as u32))
        .collect();
    let mut cities_with_main = 0usize;
    let mut cities_total = 0usize;
    let mut seen = std::collections::BTreeSet::new();
    for p in &poi {
        if p.kind != "City" || !seen.insert(p.name.clone()) {
            continue;
        }
        cities_total += 1;
        let has_main = poi
            .iter()
            .filter(|q| q.kind == "City" && q.name == p.name)
            .any(|q| {
                u64::from_str_radix(q.access_node_hex.trim(), 16)
                    .ok()
                    .and_then(|uid| uid_to_node.get(&uid).copied())
                    .map(|ni| dsu.find(ni) == main_comp)
                    .unwrap_or(false)
            });
        if has_main {
            cities_with_main += 1;
        }
    }
    println!(
        "DIAG-CITY cities_total={} cities_with_any_row_in_main={}",
        cities_total, cities_with_main
    );
    let _ = (main_size as f64, comp_points.len());

    // ---- 跨分量 prefab 覆盖（--crossing）：量化残余碎片的可归因性 ----
    // 对每个非主分量，判定是否存在「同时含该分量与主分量节点」的 prefab：
    //   有 ⇒ 存在潜在连接件，需人工复核 movement 语义；
    //   无 ⇒ 结合「道路/prefab 数据层与 oracle 零差异」可判定为**源数据拓扑断开**。
    if std::env::args().any(|a| a == "--crossing") {
        if let Ok(jg) = nav_dataset::load_junction_graph(&dir.join("junction.graph")) {
            let comp_by_uid: HashMap<u64, u32> = (0..n)
                .filter(|&i| active[i])
                .map(|i| (rg.nodes[i].uid, dsu.find(i as u32)))
                .collect();
            let mut has_crossing: HashSet<u32> = HashSet::new();
            for j in &jg.junctions {
                let mut seen: HashSet<u32> = HashSet::new();
                let mut touches_main = false;
                for u in &j.node_uids {
                    if let Some(&c) = comp_by_uid.get(u) {
                        seen.insert(c);
                        if c == main_comp {
                            touches_main = true;
                        }
                    }
                }
                if touches_main {
                    for c in seen {
                        if c != main_comp {
                            has_crossing.insert(c);
                        }
                    }
                }
            }
            let mut with = (0usize, 0usize); // (comp 数, 节点数)
            let mut without = (0usize, 0usize);
            for &(c, s) in comps.iter().skip(1) {
                if has_crossing.contains(&c) {
                    with.0 += 1;
                    with.1 += s as usize;
                } else {
                    without.0 += 1;
                    without.1 += s as usize;
                }
            }
            println!(
                "DIAG-CROSSING nonmain_comps={} with_crossing_prefab_comps={} with_nodes={} without_crossing_prefab_comps={} without_nodes={}",
                comps.len() - 1,
                with.0,
                with.1,
                without.0,
                without.1
            );
        }
    }

    // ---- 断簇空隙分析（--gap）：非主分量到主分量最近节点的距离 ----
    // 判据：空隙 ≤ 数十米 ⇒ 缺一条连接（真实缺陷，可修复）；
    //       空隙很大 ⇒ 该碎片本就与主网无物理连接（可能为合法独立区域或数据缺口）。
    if std::env::args().any(|a| a == "--gap") {
        const CELL: f64 = 500.0;
        let mut grid: HashMap<(i64, i64), Vec<(f64, f64)>> = HashMap::new();
        for (i, &act) in active.iter().enumerate() {
            if act && dsu.find(i as u32) == main_comp {
                let p = &rg.nodes[i];
                grid.entry(((p.x / CELL).floor() as i64, (p.z / CELL).floor() as i64))
                    .or_default()
                    .push((p.x, p.z));
            }
        }
        let mut buckets = [0usize; 8]; // <0.5 / <2 / <10 / <50 / <200 / <1000 / <2250 / 未命中
        let mut bucket_comps = [0usize; 8];
        let mut worst: Vec<(u32, f64, f64, f64)> = Vec::new(); // (size, gap, x, z)
        for &(c, s) in comps.iter().skip(1) {
            let pts = &comp_points[&c];
            let mut best = f64::MAX;
            let mut bx = 0.0;
            let mut bz = 0.0;
            for &i in pts {
                let p = &rg.nodes[i as usize];
                let (gx, gz) = ((p.x / CELL).floor() as i64, (p.z / CELL).floor() as i64);
                for dx in -4..=4i64 {
                    for dz in -4..=4i64 {
                        if let Some(v) = grid.get(&(gx + dx, gz + dz)) {
                            for &(qx, qz) in v {
                                let d = ((qx - p.x).powi(2) + (qz - p.z).powi(2)).sqrt();
                                if d < best {
                                    best = d;
                                    bx = p.x;
                                    bz = p.z;
                                }
                            }
                        }
                    }
                }
            }
            let bi = if best < 0.5 {
                0
            } else if best < 2.0 {
                1
            } else if best < 10.0 {
                2
            } else if best < 50.0 {
                3
            } else if best < 200.0 {
                4
            } else if best < 1000.0 {
                5
            } else if best < f64::MAX {
                6
            } else {
                7
            };
            buckets[bi] += s as usize;
            bucket_comps[bi] += 1;
            if s >= 20 {
                worst.push((s, if best == f64::MAX { 1e9 } else { best }, bx, bz));
            }
        }
        println!(
            "DIAG-GAP nodes gap_lt0.5={} gap0.5_2={} gap2_10={} gap10_50={} gap50_200={} gap200_1000={} gap1000_2250={} gap_unreached={}",
            buckets[0], buckets[1], buckets[2], buckets[3], buckets[4], buckets[5], buckets[6], buckets[7]
        );
        println!(
            "DIAG-GAP-COMPS comps gap_lt0.5={} gap0.5_2={} gap2_10={} gap10_50={} gap50_200={} gap200_1000={} gap1000_2250={} gap_unreached={}",
            bucket_comps[0], bucket_comps[1], bucket_comps[2], bucket_comps[3], bucket_comps[4], bucket_comps[5], bucket_comps[6], bucket_comps[7]
        );
        worst.sort_by_key(|&(s, _, _, _)| std::cmp::Reverse(s));
        for &(s, g, x, z) in worst.iter().take(20) {
            println!("DIAG-GAP-LARGE size={s} gap={g:.0}m at=({x:.0},{z:.0})");
        }
    }

    // ---- 碎片构成分析（--frag）：最大若干非主分量的边类型 + 边界节点所属 prefab ----
    if std::env::args().any(|a| a == "--frag") {
        let jg = nav_dataset::load_junction_graph(&dir.join("junction.graph")).ok();
        // 节点 uid → (prefab token, movement 数)
        let mut prefab_of: HashMap<u64, Vec<(String, usize)>> = HashMap::new();
        if let Some(jg) = &jg {
            for j in &jg.junctions {
                for u in &j.node_uids {
                    prefab_of
                        .entry(*u)
                        .or_default()
                        .push((j.prefab_token.clone(), j.movements.len()));
                }
            }
        }
        // 节点 → 分量内边
        let mut comp_edges: HashMap<u32, Vec<usize>> = HashMap::new();
        for (i, e) in rg.edges.iter().enumerate() {
            comp_edges.entry(dsu.find(e.from)).or_default().push(i);
        }
        println!("DIAG-FRAG 最大 12 个非主分量构成:");
        for &(c, s) in comps.iter().skip(1).take(12) {
            print_frag(&rg, &dsu, &comp_points, &comp_edges, &prefab_of, c, s, None);
        }

        // 小碎片（size 2..=12）且与主网间隙 <50m——「缺一条连接」的候选
        const CELL2: f64 = 500.0;
        let mut grid: HashMap<(i64, i64), Vec<(f64, f64)>> = HashMap::new();
        for (i, &act) in active.iter().enumerate() {
            if act && dsu.find(i as u32) == main_comp {
                let p = &rg.nodes[i];
                grid.entry(((p.x / CELL2).floor() as i64, (p.z / CELL2).floor() as i64))
                    .or_default()
                    .push((p.x, p.z));
            }
        }
        let gap_of = |pts: &Vec<u32>| -> f64 {
            let mut best = f64::MAX;
            for &i in pts {
                let p = &rg.nodes[i as usize];
                let (gx, gz) = ((p.x / CELL2).floor() as i64, (p.z / CELL2).floor() as i64);
                for dx in -4..=4i64 {
                    for dz in -4..=4i64 {
                        if let Some(v) = grid.get(&(gx + dx, gz + dz)) {
                            for &(qx, qz) in v {
                                let d = ((qx - p.x).powi(2) + (qz - p.z).powi(2)).sqrt();
                                if d < best {
                                    best = d;
                                }
                            }
                        }
                    }
                }
            }
            best
        };
        println!("DIAG-FRAG-SMALL 小碎片（size 2..=12，gap<50m）样例:");
        let mut shown = 0;
        for &(c, s) in comps.iter().skip(1) {
            if !(2..=12).contains(&s) {
                continue;
            }
            let g = gap_of(&comp_points[&c]);
            if g >= 50.0 {
                continue;
            }
            print_frag(
                &rg,
                &dsu,
                &comp_points,
                &comp_edges,
                &prefab_of,
                c,
                s,
                Some(g),
            );
            shown += 1;
            if shown >= 15 {
                break;
            }
        }
        println!("DIAG-FRAG-SMALL shown={shown}");
    }

    // ---- 单碎片转储（--dump x,z）：打印该处所属分量的全部节点/边 + 最近主网节点 ----
    if let Some(pos) = std::env::args().position(|a| a == "--dump") {
        let a: Vec<String> = std::env::args().collect();
        let x: f64 = a[pos + 1].parse().unwrap();
        let z: f64 = a[pos + 2].parse().unwrap();
        // 找最近活跃节点
        let mut best = f64::MAX;
        let mut bi = 0usize;
        for (i, &act) in active.iter().enumerate() {
            if !act {
                continue;
            }
            let p = &rg.nodes[i];
            let d = (p.x - x).powi(2) + (p.z - z).powi(2);
            if d < best {
                best = d;
                bi = i;
            }
        }
        let c = dsu.find(bi as u32);
        println!(
            "DIAG-DUMP query=({x:.0},{z:.0}) nearest_node={} dist={:.1}m comp={c} main={}",
            rg.nodes[bi].uid,
            best.sqrt(),
            c == main_comp
        );
        let pts = &comp_points[&c];
        println!(
            "DIAG-DUMP nodes={} (comp size {}):",
            pts.len(),
            comps
                .iter()
                .find(|&&(cc, _)| cc == c)
                .map(|&(_, s)| s)
                .unwrap_or(0)
        );
        let uid_to_idx: HashMap<u64, usize> = (0..n).map(|i| (rg.nodes[i].uid, i)).collect();
        for &i in pts {
            let p = &rg.nodes[i as usize];
            println!(
                "  N idx={i} uid={} pos=({:.1},{:.1},{:.1})",
                p.uid, p.x, p.y, p.z
            );
            for (ei, e) in rg.edges.iter().enumerate() {
                if e.from != i && e.to != i {
                    continue;
                }
                let (f, t) = (e.from as usize, e.to as usize);
                let other = if f == i as usize { t } else { f };
                println!(
                    "    E{ei} {:?} len={:.1} {} -> {} (other in_comp={}) geom={}",
                    e.kind,
                    e.length,
                    e.from,
                    e.to,
                    dsu.find(other as u32) == c,
                    e.geometry.len()
                );
            }
        }
        // 最近主网节点 + 中间是否有 prefab
        let jg = nav_dataset::load_junction_graph(&dir.join("junction.graph")).ok();
        let mut prefab_tokens: HashMap<u64, String> = HashMap::new();
        if let Some(jg) = &jg {
            for j in &jg.junctions {
                for u in &j.node_uids {
                    prefab_tokens
                        .entry(*u)
                        .or_insert_with(|| j.prefab_token.clone());
                }
            }
        }
        type NearestRow = (f64, u64, (f64, f64, f64), Option<String>);
        let mut nearest: Vec<NearestRow> = Vec::new();
        for (i, &act) in active.iter().enumerate() {
            if !act || dsu.find(i as u32) != main_comp {
                continue;
            }
            let p = &rg.nodes[i];
            let d = ((p.x - x).powi(2) + (p.z - z).powi(2)).sqrt();
            nearest.push((
                d,
                p.uid,
                (p.x, p.y, p.z),
                prefab_tokens.get(&p.uid).cloned(),
            ));
        }
        nearest.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        println!("DIAG-DUMP 最近主网节点:");
        for (d, uid, p, tok) in nearest.iter().take(6) {
            println!(
                "  {d:.1}m uid={uid} pos=({:.1},{:.1}) prefab={}",
                p.0,
                p.2,
                tok.clone().unwrap_or_else(|| "-".into())
            );
        }
        // 本分量节点所属 prefab
        println!("DIAG-DUMP 分量内节点 prefab:");
        for &i in pts.iter().take(20) {
            let uid = rg.nodes[i as usize].uid;
            println!(
                "  uid={uid} prefab={}",
                prefab_tokens
                    .get(&uid)
                    .cloned()
                    .unwrap_or_else(|| "-".into())
            );
        }
        let _ = uid_to_idx;
    }

    // ---- movement 物化一致性检验（--mvtest）----
    // movement 关系把「被 movement 相连的节点」归入同一簇；这些簇必然被图分量细化
    // （簇 ⊆ 分量），即 mv_clusters >= graph_comps 恒成立。
    // mv_clusters < graph_comps ⇒ 声明了 movement 却未在 routing.graph 中物化（缺边缺陷）。
    if std::env::args().any(|a| a == "--mvtest") {
        let jg = nav_dataset::load_junction_graph(&dir.join("junction.graph")).ok();
        let comp_by_uid: HashMap<u64, u32> = (0..n)
            .filter(|&i| active[i])
            .map(|i| (rg.nodes[i].uid, dsu.find(i as u32)))
            .collect();
        if let Some(jg) = jg {
            let (mut contradiction, mut spanning, mut total_j, mut mv_disconnected) =
                (0usize, 0usize, 0usize, 0usize);
            let mut samples: Vec<(String, usize, usize, usize, usize)> = Vec::new();
            for j in &jg.junctions {
                let act: Vec<u64> = j
                    .node_uids
                    .iter()
                    .copied()
                    .filter(|u| comp_by_uid.contains_key(u))
                    .collect();
                if act.len() < 2 {
                    continue;
                }
                total_j += 1;
                let pos: HashMap<u64, usize> =
                    act.iter().enumerate().map(|(i, &u)| (u, i)).collect();
                let mut uf: Vec<usize> = (0..act.len()).collect();
                fn find(uf: &mut [usize], mut x: usize) -> usize {
                    while uf[x] != x {
                        uf[x] = uf[uf[x]];
                        x = uf[x];
                    }
                    x
                }
                for m in &j.movements {
                    if let (Some(&a), Some(&b)) = (pos.get(&m.entry), pos.get(&m.exit)) {
                        let (ra, rb) = (find(&mut uf, a), find(&mut uf, b));
                        if ra != rb {
                            uf[ra] = rb;
                        }
                    }
                }
                let mut mv_roots: std::collections::BTreeSet<usize> = Default::default();
                for i in 0..act.len() {
                    let r = find(&mut uf, i);
                    mv_roots.insert(r);
                }
                let mv_clusters = mv_roots.len();
                let graph_comps: std::collections::BTreeSet<u32> =
                    act.iter().map(|u| comp_by_uid[u]).collect();
                if graph_comps.len() > 1 {
                    spanning += 1;
                    if mv_clusters > 1 {
                        mv_disconnected += 1;
                    }
                }
                if mv_clusters < graph_comps.len() {
                    contradiction += 1;
                    if samples.len() < 15 {
                        samples.push((
                            j.prefab_token.clone(),
                            j.node_uids.len(),
                            j.movements.len(),
                            mv_clusters,
                            graph_comps.len(),
                        ));
                    }
                }
            }
            println!(
                "DIAG-MVTEST junctions_ge2_active_nodes={total_j} spanning={spanning} spanning_with_mv_disconnected={mv_disconnected} contradictions={contradiction}"
            );
            for (t, nn, m, mvc, gc) in &samples {
                println!(
                    "DIAG-MVTEST-SAMPLE token={t} nodes={nn} movements={m} mv_clusters={mvc} graph_comps={gc}"
                );
            }
        }
        return;
    }

    // ---- 单 junction 剖析（--jinfo <hexuid>）：prefab 节点/ movement 端点的分量归属 ----
    if let Some(pos) = std::env::args().position(|a| a == "--jinfo") {
        let a: Vec<String> = std::env::args().collect();
        let want = u64::from_str_radix(a[pos + 1].trim_start_matches("0x"), 16).unwrap();
        let jg = nav_dataset::load_junction_graph(&dir.join("junction.graph")).ok();
        let comp_by_uid: HashMap<u64, u32> = (0..n)
            .filter(|&i| active[i])
            .map(|i| (rg.nodes[i].uid, dsu.find(i as u32)))
            .collect();
        let idx_of: HashMap<u64, usize> = (0..n).map(|i| (rg.nodes[i].uid, i)).collect();
        let tag = |u: u64| -> String {
            match comp_by_uid.get(&u) {
                Some(&c) if c == main_comp => "MAIN".to_string(),
                Some(&c) => format!("comp:{c}"),
                None => "INACTIVE".to_string(),
            }
        };
        if let Some(jg) = jg {
            for j in &jg.junctions {
                if j.uid != want {
                    continue;
                }
                println!(
                    "DIAG-JINFO uid={:016x} token={} nodes={} movements={}",
                    j.uid,
                    j.prefab_token,
                    j.node_uids.len(),
                    j.movements.len()
                );
                for (k, u) in j.node_uids.iter().enumerate() {
                    let p = idx_of.get(u).map(|&i| (rg.nodes[i].x, rg.nodes[i].z));
                    println!(
                        "  n{k} uid={u} pos=({:.1},{:.1}) {}",
                        p.map(|v| v.0).unwrap_or(0.0),
                        p.map(|v| v.1).unwrap_or(0.0),
                        tag(*u)
                    );
                }
                let mut comps: std::collections::BTreeSet<String> = Default::default();
                for u in &j.node_uids {
                    comps.insert(tag(*u));
                }
                println!("  node_component_tags={:?}", comps);
                for m in &j.movements {
                    println!(
                        "  mv id={} entry={} [{}] -> exit={} [{}] len={:.1}",
                        m.id,
                        m.entry,
                        tag(m.entry),
                        m.exit,
                        tag(m.exit),
                        m.length
                    );
                }
            }
        }
        return;
    }

    // ---- 局部剖析（--local x,z）：碎片边界 prefab 的完整节点/边/movement 画像 ----
    // 决定性检验：跨「碎片↔主网」的 prefab，其 movements 是否覆盖了跨界节点对。
    if let Some(pos) = std::env::args().position(|a| a == "--local") {
        let a: Vec<String> = std::env::args().collect();
        let x: f64 = a[pos + 1].parse().unwrap();
        let z: f64 = a[pos + 2].parse().unwrap();
        let mut best = f64::MAX;
        let mut bi = 0usize;
        for (i, &act) in active.iter().enumerate() {
            if !act {
                continue;
            }
            let p = &rg.nodes[i];
            let d = (p.x - x).powi(2) + (p.z - z).powi(2);
            if d < best {
                best = d;
                bi = i;
            }
        }
        let frag = dsu.find(bi as u32);
        println!(
            "DIAG-LOCAL query=({x:.0},{z:.0}) seed_uid={} dist={:.1}m is_main={}",
            rg.nodes[bi].uid,
            best.sqrt(),
            frag == main_comp
        );
        let idx_of: HashMap<u64, usize> = (0..n).map(|i| (rg.nodes[i].uid, i)).collect();
        let comp_by_uid: HashMap<u64, u32> = (0..n)
            .filter(|&i| active[i])
            .map(|i| (rg.nodes[i].uid, dsu.find(i as u32)))
            .collect();
        let comp_of_uid = |u: u64| -> Option<u32> { comp_by_uid.get(&u).copied() };
        let jg = nav_dataset::load_junction_graph(&dir.join("junction.graph")).ok();
        if let Some(jg) = jg {
            let mut crossing = 0usize;
            for j in &jg.junctions {
                let mut in_frag = 0usize;
                let mut in_main = 0usize;
                for u in &j.node_uids {
                    match comp_of_uid(*u) {
                        Some(c) if c == frag => in_frag += 1,
                        Some(c) if c == main_comp => in_main += 1,
                        _ => {}
                    }
                }
                if in_frag == 0 || in_main == 0 {
                    continue;
                }
                crossing += 1;
                if crossing > 6 {
                    continue;
                }
                println!(
                    "DIAG-LOCAL CROSS-PREFAB token={} uid={:016x} nodes={} movements={} in_frag={} in_main={}",
                    j.prefab_token,
                    j.uid,
                    j.node_uids.len(),
                    j.movements.len(),
                    in_frag,
                    in_main
                );
                for (k, u) in j.node_uids.iter().enumerate() {
                    let p = idx_of.get(u).map(|&i| (rg.nodes[i].x, rg.nodes[i].z));
                    let c = comp_of_uid(*u);
                    let tag = match c {
                        Some(cc) if cc == frag => "FRAG",
                        Some(cc) if cc == main_comp => "MAIN",
                        Some(_) => "OTHER",
                        None => "INACTIVE",
                    };
                    println!(
                        "    n{k} uid={u} pos=({:.1},{:.1}) {tag}",
                        p.map(|v| v.0).unwrap_or(0.0),
                        p.map(|v| v.1).unwrap_or(0.0)
                    );
                }
                for m in &j.movements {
                    let ec = comp_of_uid(m.entry);
                    let xc = comp_of_uid(m.exit);
                    let f = |c: Option<u32>| match c {
                        Some(cc) if cc == frag => "FRAG",
                        Some(cc) if cc == main_comp => "MAIN",
                        Some(_) => "OTHER",
                        None => "INACT",
                    };
                    println!(
                        "    mv id={} entry={} ({}) -> exit={} ({}) len={:.1}",
                        m.id,
                        m.entry,
                        f(ec),
                        m.exit,
                        f(xc),
                        m.length
                    );
                }
            }
            println!("DIAG-LOCAL crossing_prefabs={crossing}");
        }
        // 碎片规模 + 到主网最近距离
        let sz = comps
            .iter()
            .find(|&&(c, _)| c == frag)
            .map(|&(_, s)| s)
            .unwrap_or(0);
        println!("DIAG-LOCAL fragment_size={sz} main_size={main_size}");
    }

    // ---- prefab 跨分量检查（--junc）：判定 movement 恢复失败是否为断簇成因 ----
    // 若某 prefab 的节点分散在多个分量中，说明该 prefab 的 navigation movements
    // 未能把入口/出口连起来（movement 缺失或端点映射错误）。
    if std::env::args().any(|a| a == "--scale") {
        scale_check(&rg);
        return;
    }
    if std::env::args().any(|a| a == "--junc") {
        let jg = nav_dataset::load_junction_graph(&dir.join("junction.graph")).ok();
        let uid_to_idx: HashMap<u64, usize> = (0..n).map(|i| (rg.nodes[i].uid, i)).collect();
        if let Some(jg) = jg {
            let mut spanning = 0usize;
            let mut spanning_comp_pairs = 0usize;
            let mut no_mv = 0usize;
            let mut no_mv_spanning = 0usize;
            let mut samples: Vec<(String, usize, usize, usize)> = Vec::new(); // token, comps, nodes, movements
            let mut total = 0usize;
            let mut active_nodes_total = 0usize;
            for j in &jg.junctions {
                total += 1;
                let mut comps: HashMap<u32, u32> = HashMap::new();
                for u in &j.node_uids {
                    if let Some(&i) = uid_to_idx.get(u) {
                        // 只统计活跃节点——未被任何边引用的装饰性 prefab 节点
                        // （如 dealer28：0 道路、0 曲线）会各自成为孤立点，误判为跨分量
                        if !active[i] {
                            continue;
                        }
                        active_nodes_total += 1;
                        *comps.entry(dsu.find(i as u32)).or_insert(0) += 1;
                    }
                }
                if comps.len() > 1 {
                    spanning += 1;
                    spanning_comp_pairs += comps.len();
                    if j.movements.is_empty() {
                        no_mv_spanning += 1;
                    }
                    if samples.len() < 25 {
                        samples.push((
                            j.prefab_token.clone(),
                            comps.len(),
                            j.node_uids.len(),
                            j.movements.len(),
                        ));
                    }
                }
                if j.movements.is_empty() {
                    no_mv += 1;
                }
            }
            println!(
                "DIAG-JUNC total={total} active_nodes_in_prefabs={active_nodes_total} no_movements={no_mv} spanning_components={spanning} no_mv_and_spanning={no_mv_spanning} sum_comps_of_spanning={spanning_comp_pairs}"
            );
            for (t, c, nn, m) in &samples {
                println!("DIAG-JUNC-SAMPLE token={t} comps={c} nodes={nn} movements={m}");
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn print_frag(
    rg: &nav_dataset::RoutingGraph,
    dsu: &Dsu,
    comp_points: &HashMap<u32, Vec<u32>>,
    comp_edges: &HashMap<u32, Vec<usize>>,
    prefab_of: &HashMap<u64, Vec<(String, usize)>>,
    c: u32,
    s: u32,
    gap: Option<f64>,
) {
    let _ = dsu;
    let eids = comp_edges.get(&c).cloned().unwrap_or_default();
    let mut road = 0;
    let mut mv = 0;
    let mut other = 0;
    for &ei in &eids {
        match rg.edges[ei].kind {
            nav_dataset::EdgeKind::Road => road += 1,
            nav_dataset::EdgeKind::JunctionMovement => mv += 1,
            _ => other += 1,
        }
    }
    let pts = &comp_points[&c];
    let mut tokens: HashMap<String, (usize, usize)> = HashMap::new();
    let mut no_prefab = 0usize;
    for &i in pts {
        let uid = rg.nodes[i as usize].uid;
        match prefab_of.get(&uid) {
            Some(v) => {
                for (t, m) in v {
                    let e = tokens.entry(t.clone()).or_insert((0, 0));
                    e.0 += 1;
                    e.1 = e.1.max(*m);
                }
            }
            None => no_prefab += 1,
        }
    }
    let mut toklist: Vec<(String, usize, usize)> =
        tokens.into_iter().map(|(t, (nn, m))| (t, nn, m)).collect();
    toklist.sort_by_key(|&(_, nn, _)| std::cmp::Reverse(nn));
    let toks: Vec<String> = toklist
        .iter()
        .take(6)
        .map(|(t, nn, m)| format!("{t}(n={nn},mv={m})"))
        .collect();
    let p0 = &rg.nodes[pts[0] as usize];
    let gapstr = gap.map(|g| format!(" gap={g:.1}m")).unwrap_or_default();
    println!(
        "DIAG-FRAG size={s} edges={} road={road} movement={mv} other={other} nodes_not_in_prefab={no_prefab} at=({:.0},{:.0}){gapstr} prefabs=[{}]",
        eids.len(),
        p0.x,
        p0.z,
        toks.join(" ")
    );
}

/// movement 端点映射检验（--scale）：几何弧长 / movement.Length = 锚定缩放 s。
/// BuildWorldPolyline 把 prefab 局部曲线链锚定到 entry/exit 世界节点：s = 世界跨度/局部跨度。
/// prefab 局部单位为米 ⇒ 映射正确时 s ≈ 1；系统偏离 ⇒ ControlNode→NodeUid 映射错误。
fn scale_check(rg: &nav_dataset::RoutingGraph) {
    let mut buckets = [0usize; 8]; // <0.2 / 0.2-0.5 / 0.5-0.8 / 0.8-1.25 / 1.25-2 / 2-5 / 5-20 / >20
    let mut total = 0usize;
    let mut samples: Vec<(usize, f32, f64, f64, u64)> = Vec::new();
    for (i, e) in rg.edges.iter().enumerate() {
        if !matches!(e.kind, nav_dataset::EdgeKind::JunctionMovement) {
            continue;
        }
        let pts = &e.geometry;
        if pts.len() < 2 || e.length <= 1e-6 {
            continue;
        }
        let mut arc = 0.0;
        for w in pts.windows(2) {
            let dx = w[1].0 - w[0].0;
            let dz = w[1].2 - w[0].2;
            arc += (dx * dx + dz * dz).sqrt();
        }
        let s = arc / e.length as f64;
        total += 1;
        let bi = if s < 0.2 {
            0
        } else if s < 0.5 {
            1
        } else if s < 0.8 {
            2
        } else if s <= 1.25 {
            3
        } else if s <= 2.0 {
            4
        } else if s <= 5.0 {
            5
        } else if s <= 20.0 {
            6
        } else {
            7
        };
        buckets[bi] += 1;
        if !(0.5..=2.0).contains(&s) && samples.len() < 15 {
            samples.push((i, e.length, arc, s, e.source_uid));
        }
    }
    println!(
        "DIAG-SCALE movements_with_geom={} s_lt0.2={} s0.2to0.5={} s0.5to0.8={} s0.8to1.25={} s1.25to2={} s2to5={} s5to20={} s_gt20={}",
        total, buckets[0], buckets[1], buckets[2], buckets[3], buckets[4], buckets[5], buckets[6], buckets[7]
    );
    let in_range = buckets[2] + buckets[3] + buckets[4];
    let pct = 100.0 * in_range as f64 / total.max(1) as f64;
    println!(
        "DIAG-SCALE-SUMMARY in_range_0.5to2={} pct={:.2}",
        in_range, pct
    );
    for (i, len, arc, s, uid) in &samples {
        println!(
            "DIAG-SCALE-SAMPLE edge={} length={:.1} arc={:.1} s={:.3} junction={:016x}",
            i, len, arc, s, uid
        );
    }
}

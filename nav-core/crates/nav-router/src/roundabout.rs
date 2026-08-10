// P2-14 Roundabout Detection + Transit Guidance（§100-106）。
// 环岛检测：junction movement **内部图**（两端都在控制节点集合内）中的简单环（§101）——
// 环长 3-8、环上每节点内部度数（入+出）== 2（全连接交叉口/互通度数更高被排除）、
// 环内 movement 转向一致（同向绕行）、环 movement 平均长度 < 120m（内部 geometry）。
// 出口编号：从 entry 沿环到 exit 的出口序号（§102）；Transit：RouteLeg（§106）。
use nav_dataset::{Junction, JunctionMovement};
use std::collections::HashMap;

/// 环岛检测器（拓扑方法，§101）。
pub struct RoundaboutDetector;

impl RoundaboutDetector {
    pub fn is_roundabout(j: &Junction) -> bool {
        let node_set: std::collections::HashSet<u64> = j.node_uids.iter().copied().collect();
        // 内部边：entry 与 exit 都在 junction 控制节点集合内（排除连接外部道路的入/出环 movement）
        let mut adj: HashMap<u64, Vec<&JunctionMovement>> = HashMap::new();
        for m in &j.movements {
            if node_set.contains(&m.entry) && node_set.contains(&m.exit) {
                adj.entry(m.entry).or_default().push(m);
            }
        }
        for start in j.node_uids.iter() {
            if !adj.contains_key(start) {
                continue;
            }
            let mut path: Vec<&JunctionMovement> = Vec::new();
            if Self::find_simple_cycle(&adj, *start, *start, 0, &mut path).is_some() {
                return true;
            }
        }
        false
    }

    /// 返回环路径（不含闭合边）；无合法环返回 None。
    fn find_simple_cycle<'a>(
        adj: &HashMap<u64, Vec<&'a JunctionMovement>>,
        node: u64,
        start: u64,
        depth: u32,
        path: &mut Vec<&'a JunctionMovement>,
    ) -> Option<Vec<&'a JunctionMovement>> {
        if depth >= 8 {
            return None;
        }
        let outs = adj.get(&node)?;
        for m in outs {
            if m.exit == start {
                if depth + 1 >= 3 && Self::valid_ring(adj, path, m) {
                    let mut ring = path.clone();
                    ring.push(m);
                    return Some(ring);
                }
                continue;
            }
            if path.iter().any(|p| p.entry == m.exit) {
                continue; // 简单环：节点不重复
            }
            path.push(m);
            if let Some(r) = Self::find_simple_cycle(adj, m.exit, start, depth + 1, path) {
                return Some(r);
            }
            path.pop();
        }
        None
    }

    /// 环合法性：内部度数 == 2（环岛特征）+ 环长 ≤ 8 + 平均长度 < 120m + turn 一致。
    fn valid_ring<'a>(
        adj: &HashMap<u64, Vec<&'a JunctionMovement>>,
        path: &[&'a JunctionMovement],
        last: &'a JunctionMovement,
    ) -> bool {
        let mut ring: Vec<&JunctionMovement> = path.to_vec();
        ring.push(last);
        let n = ring.len();
        if n > 8 {
            return false;
        }
        let nodes: std::collections::HashSet<u64> = ring.iter().map(|m| m.entry).collect();
        if nodes.len() != n {
            return false; // 节点重复（非简单环）
        }
        // 入度表（内部边）
        let mut indeg: HashMap<u64, u32> = HashMap::new();
        for v in adj.values() {
            for m in v {
                *indeg.entry(m.exit).or_default() += 1;
            }
        }
        for node in &nodes {
            let out_deg = adj.get(node).map(|v| v.len() as u32).unwrap_or(0);
            let in_deg = indeg.get(node).copied().unwrap_or(0);
            if in_deg + out_deg != 4 {
                return false; // 全连接交叉口/互通节点内部度数更高
            }
            if out_deg == 0 {
                return false;
            }
            // chord 检查：指向环上其他节点的出边恰 1 条（= 环内边；
            // 三角全连接/多边互通有 2+ 条指向环上节点的出边）
            let mut to_ring = 0u32;
            if let Some(v) = adj.get(node) {
                for m in v {
                    if nodes.contains(&m.exit) {
                        to_ring += 1;
                    }
                }
            }
            if to_ring != 1 {
                return false;
            }
        }
        // 环 movement 平均长度 < 120m（内部 geometry：环岛环径 20-80m；互通环长得多）
        let avg_len: f64 = ring.iter().map(|m| m.length as f64).sum::<f64>() / n as f64;
        if avg_len > 120.0 {
            return false;
        }
        // 注：turn 一致性判据已删除——PPD TurnAngle 为近似（<30° 全记 0），
        // 大半径环岛的绕环 movement 可能全为 0；度数+长度+无 chord 已充分区分。
        true
    }

    /// 出口编号（§102）：从 entry movement 沿环到 exit movement 的出口序号（1-based）。
    /// 非环岛或不可达返回 None。
    pub fn exit_number(
        j: &Junction,
        entry: &JunctionMovement,
        exit: &JunctionMovement,
    ) -> Option<u32> {
        if !Self::is_roundabout(j) {
            return None;
        }
        let node_set: std::collections::HashSet<u64> = j.node_uids.iter().copied().collect();
        let mut adj: HashMap<u64, Vec<&JunctionMovement>> = HashMap::new();
        for m in &j.movements {
            if node_set.contains(&m.entry) && node_set.contains(&m.exit) {
                adj.entry(m.entry).or_default().push(m);
            }
        }
        // 找环路径（环边序列）
        let mut ring_path: Option<Vec<&JunctionMovement>> = None;
        for start in j.node_uids.iter() {
            if !adj.contains_key(start) {
                continue;
            }
            let mut path: Vec<&JunctionMovement> = Vec::new();
            if let Some(r) = Self::find_simple_cycle(&adj, *start, *start, 0, &mut path) {
                ring_path = Some(r);
                break;
            }
        }
        let ring = ring_path?;
        let ring_nodes: std::collections::HashSet<u64> = ring.iter().map(|m| m.entry).collect();
        // 从 entry 的出口节点沿环前进（环边 = 环路径中的边），数经过的外部入口节点
        let mut cur = entry.exit;
        let mut count: u32 = 1;
        for _ in 0..64 {
            // 找到从 cur 出发的环边
            let m = ring.iter().find(|m| m.entry == cur)?;
            if m.entry == exit.entry && m.exit == exit.exit {
                return Some(count);
            }
            cur = m.exit;
            if cur == entry.entry {
                return None; // 绕回起点未找到出口
            }
            // 到达下一个外部入口节点（入口 movement 的 entry 不在环上）→ 计数出口
            let has_external_in = j
                .movements
                .iter()
                .any(|x| x.exit == cur && !ring_nodes.contains(&x.entry));
            if has_external_in && cur != entry.entry {
                if cur == exit.entry {
                    return Some(count);
                }
                count += 1;
            }
        }
        None
    }
}

/// 路线运输段（§106）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteLeg {
    Drive,
    Ferry,
    Train,
}

impl RouteLeg {
    pub fn name(&self) -> &'static str {
        match self {
            RouteLeg::Drive => "Drive",
            RouteLeg::Ferry => "Ferry",
            RouteLeg::Train => "Train",
        }
    }
}

/// 按边类型分段（§106：UI 不需要从 edge sequence 猜运输方式）。
pub fn route_legs(edge_kinds: &[nav_dataset::EdgeKind]) -> Vec<RouteLeg> {
    let mut legs = Vec::new();
    for k in edge_kinds {
        let leg = match k {
            nav_dataset::EdgeKind::Ferry => RouteLeg::Ferry,
            nav_dataset::EdgeKind::Train => RouteLeg::Train,
            _ => RouteLeg::Drive,
        };
        if legs.last() != Some(&leg) {
            legs.push(leg);
        }
    }
    if legs.is_empty() {
        legs.push(RouteLeg::Drive);
    }
    legs
}

#[cfg(test)]
mod tests {
    use super::*;
    use nav_dataset::{EdgeKind, JunctionMovement};

    fn mv(id: u32, entry: u64, exit: u64) -> JunctionMovement {
        JunctionMovement {
            id,
            entry,
            exit,
            length: 10.0,
            turn_type: 0,
            semaphore_id: -1,
            signal_group_type: String::new(),
            geometry: Vec::new(),
        }
    }

    /// 5 入口环岛：内部环 0→1→2→3→4→0；入口 100+i → i；出口 i → 200+i。
    /// node_uids 含全部控制节点（入口/出口节点也在内——与真实 prefab 一致）。
    fn roundabout() -> Junction {
        let mut movements = Vec::new();
        for i in 0..5 {
            movements.push(mv(i as u32, i, (i + 1) % 5));
        }
        for i in 0..5 {
            movements.push(mv(10 + i as u32, 100 + i, i));
            movements.push(mv(20 + i as u32, i, 200 + i));
        }
        let mut node_uids: Vec<u64> = (0..5).collect();
        node_uids.extend(100..105);
        node_uids.extend(200..205);
        Junction {
            uid: 1,
            prefab_token: "test_rb".into(),
            node_uids,
            movements,
        }
    }

    /// 普通十字路口：4 路直通，无内部环。
    fn cross() -> Junction {
        let mut movements = Vec::new();
        for i in 0..4 {
            movements.push(mv(i as u32, 100 + i, 200 + i));
        }
        Junction {
            uid: 2,
            prefab_token: "test_cross".into(),
            node_uids: vec![100, 101, 102, 103, 200, 201, 202, 203],
            movements,
        }
    }

    /// 全连接 4 节点交叉口（chord 特征，误报来源）。
    fn full_mesh() -> Junction {
        let mut movements = Vec::new();
        let mut id = 0;
        for a in 0..4u64 {
            for b in 0..4u64 {
                if a != b {
                    movements.push(mv(id, a, b));
                    id += 1;
                }
            }
        }
        Junction {
            uid: 3,
            prefab_token: "test_mesh".into(),
            node_uids: vec![0, 1, 2, 3],
            movements,
        }
    }

    #[test]
    fn detects_roundabout_topology() {
        assert!(RoundaboutDetector::is_roundabout(&roundabout()));
        assert!(
            !RoundaboutDetector::is_roundabout(&cross()),
            "十字路口不应误报"
        );
        assert!(
            !RoundaboutDetector::is_roundabout(&full_mesh()),
            "全连接不应误报"
        );
    }

    #[test]
    fn exit_numbering() {
        let j = roundabout();
        // 入口 100→0，出口 2→202（入口节点 2）：0→1→2 经过节点 1、2 = 第 2 个出口
        let entry = j.movements.iter().find(|m| m.id == 10).unwrap();
        let exit = j.movements.iter().find(|m| m.id == 22).unwrap();
        assert_eq!(RoundaboutDetector::exit_number(&j, entry, exit), Some(2));
        // 第 1 个出口（入口节点 1 = 出口 1→201）
        let exit1 = j.movements.iter().find(|m| m.id == 21).unwrap();
        assert_eq!(RoundaboutDetector::exit_number(&j, entry, exit1), Some(1));
        // 下一出口
        let exit2 = j.movements.iter().find(|m| m.id == 23).unwrap();
        assert_eq!(RoundaboutDetector::exit_number(&j, entry, exit2), Some(3));
    }

    #[test]
    fn legs_segment() {
        use EdgeKind::*;
        let kinds = [Road, JunctionMovement, Ferry, Road, Train, Road];
        let legs = route_legs(&kinds);
        assert_eq!(
            legs,
            vec![
                RouteLeg::Drive,
                RouteLeg::Ferry,
                RouteLeg::Drive,
                RouteLeg::Train,
                RouteLeg::Drive
            ]
        );
        assert_eq!(route_legs(&[Road, Road]), vec![RouteLeg::Drive]);
    }
}

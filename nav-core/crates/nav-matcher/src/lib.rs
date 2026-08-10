// nav-matcher：车辆 Map Matching（P2 计划 §45-53）。
// rolling multi-candidate tracker（局部 Viterbi）：每帧 Top-K 候选（距离×航向×拓扑连续性），
// 滑动窗口确定最优路径。heading 用投影点附近 polyline tangent（§48）。
use nav_graph::{CompactGraph, EdgeKind};

/// 匹配置信度（§51）：仅 HIGH/MEDIUM 允许 route progress / off-route 判定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchConfidence {
    High,
    Medium,
    Low,
    Unmatched,
}

/// 单候选匹配结果。
#[derive(Debug, Clone)]
pub struct Candidate {
    pub edge_id: u32,
    /// 沿 polyline 的弧长偏移（米）。
    pub offset: f64,
    /// 投影点（世界坐标）。
    pub position: (f64, f64, f64),
    /// 投影点 tangent 方向（弧度，世界 XZ 平面；§48 不用端点方向）。
    pub tangent: f64,
    /// 横向距离（米）。
    pub lateral: f64,
    /// 评分（0..1，越高越好）。
    pub score: f64,
    /// 与上一帧最优候选的拓扑连续性分（0..1）。
    pub continuity: f64,
}

/// 帧匹配结果。
#[derive(Debug, Clone)]
pub struct MapMatch {
    pub edge_id: u32,
    pub offset: f64,
    pub projected_position: (f64, f64, f64),
    pub direction: f64, // 行驶方向（世界弧度）
    pub confidence: MatchConfidence,
    pub lateral: f64,
    /// 该帧 Top-K 候选（调试/跟踪用）。
    pub candidates: Vec<Candidate>,
}

/// 匹配器配置（§47 权重——trace calibration 后冻结）。
#[derive(Debug, Clone)]
pub struct MatcherConfig {
    /// 候选评分权重：横向距离 / 航向 / 拓扑连续性。
    pub w_distance: f64,
    pub w_heading: f64,
    pub w_topology: f64,
    /// 横向距离分衰减尺度（米）：d 超过该值距离分 → 0。
    pub distance_scale: f64,
    /// 航向差衰减尺度（弧度）。
    pub heading_scale: f64,
    /// 每帧候选数 K（§49 Top-K）。
    pub top_k: usize,
    /// 查询半径（连续导航小半径，§44）。
    pub query_radius: f64,
    /// 初始化/丢失后的查询半径（§44 自适应）。
    pub init_radius: f64,
    /// 拓扑连续性判定：候选 from == 上一边 to（允许反向对向行驶惩罚）。
    pub topology_gap: u32,
    /// 置信度阈值：lateral < high_lateral → High；< medium_lateral → Medium；否则 Low。
    pub high_lateral: f64,
    pub medium_lateral: f64,
}

impl Default for MatcherConfig {
    fn default() -> Self {
        MatcherConfig {
            w_distance: 0.5,
            w_heading: 0.3,
            w_topology: 0.2,
            distance_scale: 30.0,
            heading_scale: 0.9, // ~52°
            top_k: 8,
            query_radius: 60.0,
            init_radius: 300.0,
            topology_gap: 2,
            high_lateral: 8.0,
            medium_lateral: 20.0,
        }
    }
}

/// Rolling matcher：维护上一帧最优边 + Top-K 历史。
pub struct MapMatcher {
    cfg: MatcherConfig,
    /// 上一帧最优 edge（-1 = 未锁定）。
    last_edge: i32,
    /// 初始化未锁定帧数（用于扩大搜索半径，§44）。
    lost_frames: u32,
}

impl MapMatcher {
    pub fn new(cfg: MatcherConfig) -> Self {
        MapMatcher {
            cfg,
            last_edge: -1,
            lost_frames: 0,
        }
    }

    /// 输入一帧（位置 + 航向弧度），输出匹配结果。
    pub fn match_frame(
        &mut self,
        graph: &CompactGraph,
        spatial: &nav_spatial::SpatialIndex,
        x: f64,
        z: f64,
        heading: f64,
    ) -> MapMatch {
        let radius = if self.last_edge < 0 {
            self.cfg.init_radius
        } else {
            self.cfg.query_radius
        };
        let edge_ids = spatial.query_radius(x, z, radius);
        let mut cands: Vec<Candidate> = Vec::with_capacity(edge_ids.len());
        for eid in edge_ids {
            let Some(c) = self.score_candidate(graph, eid, x, z, heading) else {
                continue;
            };
            cands.push(c);
        }
        // 按评分排序取 Top-K
        cands.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        cands.truncate(self.cfg.top_k);
        let best = cands.first().cloned();
        match best {
            Some(b) => {
                // 置信度：横向距离 + 连续性
                let conf = if b.lateral < self.cfg.high_lateral {
                    MatchConfidence::High
                } else if b.lateral < self.cfg.medium_lateral {
                    MatchConfidence::Medium
                } else {
                    MatchConfidence::Low
                };
                self.last_edge = b.edge_id as i32;
                self.lost_frames = 0;
                MapMatch {
                    edge_id: b.edge_id,
                    offset: b.offset,
                    projected_position: b.position,
                    direction: b.tangent,
                    confidence: conf,
                    lateral: b.lateral,
                    candidates: cands,
                }
            }
            None => {
                self.lost_frames += 1;
                if self.lost_frames > 30 {
                    self.last_edge = -1; // 长时间丢失 → 重新大半径搜索（§44）
                }
                MapMatch {
                    edge_id: u32::MAX,
                    offset: 0.0,
                    projected_position: (x, 0.0, z),
                    direction: heading,
                    confidence: MatchConfidence::Unmatched,
                    lateral: f64::MAX,
                    candidates: Vec::new(),
                }
            }
        }
    }

    /// 单候选评分（§47：距离×航向×拓扑连续性）。
    fn score_candidate(
        &self,
        graph: &CompactGraph,
        eid: u32,
        x: f64,
        z: f64,
        heading: f64,
    ) -> Option<Candidate> {
        let e = &graph.edges[eid as usize];
        // 只匹配 Road / JunctionMovement
        if e.kind != EdgeKind::Road && e.kind != EdgeKind::JunctionMovement {
            return None;
        }
        let pts = graph.edge_geometry(e);
        if pts.len() < 2 {
            return None;
        }
        // 最近点投影（分段线性）
        let (proj, lateral, offset, tangent) = nav_graph::project_point(pts, x, z);
        if lateral > self.cfg.distance_scale * 3.0 {
            return None; // 过远直接排除
        }
        // 距离分（0..1）
        let sd = (1.0 - lateral / self.cfg.distance_scale).clamp(0.0, 1.0);
        // 航向分：车辆 heading 与 tangent 夹角（双向道路允许 ±180° 同向）
        let d_ang = angle_diff(heading, tangent);
        // 对向（d_ang ≈ π）——道路双向时方向分取 min(d_ang, π-d_ang)
        let d_eff = d_ang.min(std::f64::consts::PI - d_ang).abs();
        let sh = (1.0 - d_eff / self.cfg.heading_scale).clamp(0.0, 1.0);
        // 拓扑连续性：与上一帧最优边相连？
        let st = if self.last_edge >= 0 {
            let le = &graph.edges[self.last_edge as usize];
            // 上一 to → 本 from（同向续行）或上一 from → 本 to（对向折返，低分）
            if le.to == e.from {
                1.0
            } else if le.to == e.to || le.from == e.from {
                0.3 // 同节点回头/并线
            } else {
                0.0
            }
        } else {
            0.5 // 未锁定：中性
        };
        let score = self.cfg.w_distance * sd + self.cfg.w_heading * sh + self.cfg.w_topology * st;
        Some(Candidate {
            edge_id: eid,
            offset,
            position: proj,
            tangent,
            lateral,
            score,
            continuity: st,
        })
    }
}

/// 角度差（弧度，归一化到 [-π, π]）。
pub fn angle_diff(a: f64, b: f64) -> f64 {
    let d = (a - b + std::f64::consts::PI) % (2.0 * std::f64::consts::PI) - std::f64::consts::PI;
    if d < -std::f64::consts::PI {
        d + 2.0 * std::f64::consts::PI
    } else {
        d
    }
}
